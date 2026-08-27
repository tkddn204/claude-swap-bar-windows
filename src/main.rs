#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use claude_swap_bar::cswap::{self, CswapError, Switch};
use claude_swap_bar::menu::{self, MenuModel, View};
use claude_swap_bar::model::ListResponse;

use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem};
use tauri::tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

/// Everything the refresh loop and the menu builder share.
struct State {
    exe: Option<PathBuf>,
    snapshot: Option<ListResponse>,
    fetched_at: Option<Instant>,
    error: Option<String>,
    /// One-shot message, cleared by the next successful refresh.
    notice: Option<String>,
    /// True while a `cswap list` call is in flight, so requests coalesce.
    /// Only ever touched while the mutex is held.
    busy: bool,
    /// Set when a request arrives while one is already in flight. The running
    /// refresh fetches once more before finishing, so the late caller's result
    /// still reaches the UI. Dropping the request instead would strand a
    /// just-completed account switch: the check mark would not move until the
    /// next five-minute tick.
    pending: bool,
}

/// Clears the in-flight flag if a refresh unwinds.
///
/// Without this, a panic inside the fetch would leave `busy` stuck true and
/// silently freeze every future refresh — the tooltip would stop changing and
/// `Refresh now` would do nothing, with no symptom visible from the tray.
///
/// On the normal path `refresh` clears the flag itself, under the same lock it
/// uses to decide it is finished, and then disarms this guard. Letting the
/// guard clear the flag on that path instead would open a window: another
/// refresh could claim the flag in between, and this guard would then clear a
/// flag it no longer owns, admitting two concurrent `cswap list` calls.
struct BusyGuard {
    state: Shared,
    armed: bool,
}

impl BusyGuard {
    fn new(state: Shared) -> Self {
        Self { state, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut guard = match self.state.lock() {
            Ok(guard) => guard,
            // The payload is plain data, so a poisoned lock is still usable
            // and clearing the flag matters more than propagating the panic.
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.busy = false;
        guard.pending = false;
    }
}

impl State {
    fn view(&self) -> MenuModel {
        menu::build(&View {
            snapshot: self.snapshot.as_ref(),
            age_secs: self.fetched_at.map(|t| t.elapsed().as_secs()),
            error: self.error.as_deref(),
            notice: self.notice.as_deref(),
        })
    }
}

type Shared = Arc<Mutex<State>>;

fn main() {
    let state: Shared = Arc::new(Mutex::new(State {
        exe: cswap::find_exe(),
        snapshot: None,
        fetched_at: None,
        error: None,
        notice: None,
        busy: false,
        pending: false,
    }));

    {
        // One acquisition, not two: `std::sync::Mutex` is not reentrant, so
        // locking in the condition and again in the body is a self-deadlock
        // waiting for someone to rewrite it as `if let`.
        let mut guard = state.lock().unwrap();
        if guard.exe.is_none() {
            guard.error = Some(CswapError::NotFound.summary());
        }
    }

    let for_setup = state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // A second launch just nudges the running instance to refresh.
            let app = app.clone();
            std::thread::spawn(move || {
                if let Some(state) = app.try_state::<Shared>() {
                    refresh(&app, &state.inner().clone());
                }
            });
        }))
        .setup(move |app| {
            app.manage(for_setup.clone());

            let menu = build_menu(app.handle(), &for_setup.lock().unwrap().view())?;
            let tray = TrayIconBuilder::with_id("main")
                .icon(tauri::include_image!("icons/tray.png"))
                .tooltip("Claude Swap Bar")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .build(app)?;

            tray.on_menu_event(move |app, event| {
                on_menu_event(app, event.id().as_ref().to_string());
            });

            tray.on_tray_icon_event(|tray, event| {
                on_tray_event(tray.app_handle(), event);
            });

            // First refresh immediately, then every REFRESH_INTERVAL.
            let loop_handle = app.handle().clone();
            let loop_state = for_setup.clone();
            std::thread::spawn(move || loop {
                refresh(&loop_handle, &loop_state);
                std::thread::sleep(REFRESH_INTERVAL);
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start Claude Swap Bar");
}

/// Rebuild the native menu from a `MenuModel`.
fn build_menu(app: &AppHandle, model: &MenuModel) -> tauri::Result<Menu<tauri::Wry>> {
    let mut builder = MenuBuilder::new(app);

    if let Some(notice) = model.notice.as_ref() {
        let item = MenuItem::with_id(app, "notice", notice, false, None::<&str>)?;
        builder = builder.item(&item).separator();
    }

    let mut rows = Vec::new();
    for row in &model.accounts {
        rows.push(CheckMenuItem::with_id(
            app,
            &row.id,
            &row.label,
            row.enabled,
            row.checked,
            None::<&str>,
        )?);
    }
    for row in &rows {
        builder = builder.item(row);
    }
    if !rows.is_empty() {
        builder = builder.separator();
    }

    let autostart_on = claude_swap_bar::autostart::is_enabled();
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Start with Windows",
        true,
        autostart_on,
        None::<&str>,
    )?;

    builder
        .text("rotate", "Rotate")
        .text("best", "Best Quota")
        .separator()
        .text("refresh", &model.refresh_label)
        .text("dashboard", "Open Dashboard")
        .item(&autostart)
        .separator()
        .text("quit", "Quit")
        .build()
}

/// Push the current state into the tray icon: menu plus tooltip.
fn render(app: &AppHandle, state: &Shared) {
    let model = state.lock().unwrap().view();
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(tray) = app.tray_by_id("main") {
            if let Ok(menu) = build_menu(&app, &model) {
                let _ = tray.set_menu(Some(menu));
            }
            let _ = tray.set_tooltip(Some(&model.tooltip));
        }
    });
}

/// Fetch a fresh snapshot.
///
/// If a fetch is already running, this records that another one is wanted and
/// returns; the running fetch picks the request up rather than dropping it.
fn refresh(app: &AppHandle, state: &Shared) {
    {
        let mut guard = state.lock().unwrap();
        if guard.busy {
            guard.pending = true;
            return;
        }
        guard.busy = true;
    }
    let mut busy = BusyGuard::new(state.clone());

    loop {
        // Re-resolve the executable while we have none: `cswap` may have been
        // installed since startup, and the alternative is showing "not found"
        // until the app is restarted.
        //
        // `find_exe` spawns a process to probe, and that probe has no timeout,
        // so it must never run under the lock — a wedged probe would hold the
        // state mutex indefinitely and freeze every menu interaction behind it.
        let known = {
            let guard = state.lock().unwrap();
            guard.exe.clone()
        };
        let exe = match known {
            Some(exe) => Some(exe),
            None => {
                let found = cswap::find_exe();
                if found.is_some() {
                    state.lock().unwrap().exe = found.clone();
                }
                found
            }
        };

        let outcome = match exe {
            Some(exe) => cswap::list(&exe),
            None => Err(CswapError::NotFound),
        };

        {
            let mut guard = state.lock().unwrap();
            match outcome {
                Ok(list) => {
                    guard.snapshot = Some(list);
                    guard.fetched_at = Some(Instant::now());
                    guard.error = None;
                    guard.notice = None;
                }
                Err(err) => {
                    guard.error = Some(err.summary());
                }
            }
        }

        render(app, state);

        // Read `pending` AFTER rendering, and clear `busy` in the same lock
        // acquisition that decides to stop. Reading it earlier would lose a
        // request that arrived while `render` was running, and clearing the
        // flag in a separate step would let a request slip into the gap and
        // then be discarded by this refresh.
        let mut guard = state.lock().unwrap();
        if !std::mem::take(&mut guard.pending) {
            guard.busy = false;
            busy.disarm();
            break;
        }
    }
}

/// Opening the tray menu refreshes, which spec section 5 lists as one of the
/// three refresh triggers alongside the timer and `Refresh now`.
///
/// The filter is the whole of this function. `Move`, `Enter` and `Leave` fire
/// as the pointer travels across the icon — acting on those would spawn a
/// `cswap list` storm. `Click` itself arrives twice per click, once for the
/// press and once for the release, so it needs the button state too.
///
/// `MouseButtonState::Down` is the press. Tauri's doc comments on that enum
/// have the two meanings swapped, so read the mapping rather than the prose:
/// `tray-icon`'s Windows handler builds `Down` from `WM_*BUTTONDOWN` and `Up`
/// from `WM_*BUTTONUP`, and Tauri converts one to one. Reacting to the press
/// also gets the work started before `WM_LBUTTONUP` pops the menu open.
fn on_tray_event(app: &AppHandle, event: TrayIconEvent) {
    if !matches!(
        event,
        TrayIconEvent::Click {
            button_state: MouseButtonState::Down,
            ..
        }
    ) {
        return;
    }
    let Some(state) = app.try_state::<Shared>().map(|s| s.inner().clone()) else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        // Repaint from what we already hold before fetching. The menu Windows
        // is about to draw is built from the current state, and the fetch below
        // takes on the order of a second — without this the freshness label
        // would still read as of the last refresh, which is the bug that made
        // `Refresh now   · 방금 갱신` permanent.
        render(&app, &state);
        refresh(&app, &state);
    });
}

fn on_menu_event(app: &AppHandle, id: String) {
    let Some(state) = app.try_state::<Shared>().map(|s| s.inner().clone()) else {
        return;
    };
    let app = app.clone();

    match id.as_str() {
        "quit" => app.exit(0),
        // Unreachable in practice: the notice row is built disabled, so it
        // never emits an event. Present so the id is accounted for.
        "notice" => {}
        "refresh" => {
            std::thread::spawn(move || refresh(&app, &state));
        }
        "rotate" => spawn_switch(app, state, Switch::Rotate),
        "best" => spawn_switch(app, state, Switch::Best),
        "dashboard" => {
            let exe = state.lock().unwrap().exe.clone();
            if let Some(exe) = exe {
                if let Err(err) = cswap::open_dashboard(&exe) {
                    state.lock().unwrap().notice =
                        Some(format!("대시보드 실행 실패: {}", err.summary()));
                    render(&app, &state);
                }
            }
        }
        "autostart" => {
            let want = !claude_swap_bar::autostart::is_enabled();
            if let Err(err) = claude_swap_bar::autostart::set(want) {
                state.lock().unwrap().notice =
                    Some(format!("시작프로그램 설정 실패: {err}"));
            }
            render(&app, &state);
        }
        other => {
            if let Some(num) = other.strip_prefix("acct:").and_then(|n| n.parse().ok()) {
                spawn_switch(app, state, Switch::Account(num));
            }
        }
    }
}

fn spawn_switch(app: AppHandle, state: Shared, target: Switch) {
    std::thread::spawn(move || {
        let exe = state.lock().unwrap().exe.clone();
        let result = match exe {
            Some(exe) => cswap::switch(&exe, target),
            None => Err(CswapError::NotFound),
        };
        if let Err(err) = result {
            state.lock().unwrap().notice = Some(format!("전환 실패: {}", err.summary()));
            render(&app, &state);
            return;
        }
        refresh(&app, &state);
    });
}

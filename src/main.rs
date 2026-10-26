#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use claude_swap_bar::badge::{self, Badge};
use claude_swap_bar::cswap::{self, CswapError, Switch};
use claude_swap_bar::menu::{self, MenuModel, View};
use claude_swap_bar::model::ListResponse;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem};
use tauri::tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

/// Everything the refresh loop and the menu builder share.
struct State {
    exe: Option<PathBuf>,
    snapshot: Option<ListResponse>,
    error: Option<String>,
    /// True while the last attempt could not find `cswap.exe` at all. Kept
    /// apart from `exe`, which can still hold a path to an executable that has
    /// since been deleted, and apart from `error`, which flattens every failure
    /// to one string. Only this state offers the install guide.
    cswap_missing: bool,
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
    ///
    /// It carries the caller's own `MenuUpdate` rather than inheriting the
    /// running refresh's. A switch that lands while a click-initiated refresh
    /// is in flight still needs the menu rebuilt, and folding it into a `Skip`
    /// would strand the check mark exactly as dropping the request would.
    pending: Option<MenuUpdate>,
    /// Mirror of the registry setting, so building a view never reads it.
    usage_icon: bool,
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
        guard.pending = None;
    }
}

impl State {
    fn view(&self) -> MenuModel {
        menu::build(&View {
            snapshot: self.snapshot.as_ref(),
            error: self.error.as_deref(),
            notice: self.notice.as_deref(),
            cswap_missing: self.cswap_missing,
            usage_icon: self.usage_icon,
        })
    }
}

type Shared = Arc<Mutex<State>>;

fn main() {
    let state: Shared = Arc::new(Mutex::new(State {
        exe: cswap::find_exe(),
        snapshot: None,
        error: None,
        cswap_missing: false,
        notice: None,
        busy: false,
        pending: None,
        usage_icon: claude_swap_bar::settings::usage_icon(),
    }));

    {
        // One acquisition, not two: `std::sync::Mutex` is not reentrant, so
        // locking in the condition and again in the body is a self-deadlock
        // waiting for someone to rewrite it as `if let`.
        let mut guard = state.lock().unwrap();
        if guard.exe.is_none() {
            guard.error = Some(CswapError::NotFound.summary());
            guard.cswap_missing = true;
        }
    }

    let for_setup = state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // A second launch just nudges the running instance to refresh.
            let app = app.clone();
            std::thread::spawn(move || {
                if let Some(state) = app.try_state::<Shared>() {
                    refresh(&app, &state.inner().clone(), MenuUpdate::Rebuild);
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
                refresh(&loop_handle, &loop_state, MenuUpdate::Rebuild);
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

    // Built independently rather than nesting the link inside the notice: today
    // `install_help` only ever comes with a notice, but hiding the link behind
    // that coincidence means a later change to `notice_line` silently deletes
    // the only thing a user with no cswap can click.
    if let Some(notice) = model.notice.as_ref() {
        let item = MenuItem::with_id(app, "notice", notice, false, None::<&str>)?;
        builder = builder.item(&item);
    }
    if model.install_help {
        builder = builder.text("install", "Install cswap");
    }
    if model.notice.is_some() || model.install_help {
        builder = builder.separator();
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
    let usage_icon = CheckMenuItem::with_id(
        app,
        "usage_icon",
        "Show usage on icon",
        true,
        model.usage_icon,
        None::<&str>,
    )?;

    builder
        .text("rotate", "Rotate")
        .text("best", "Best Quota")
        .separator()
        .text("refresh", "Refresh now")
        .text("dashboard", "Open Dashboard")
        .item(&usage_icon)
        .item(&autostart)
        .separator()
        .text("quit", "Quit")
        .build()
}

/// Whether a refresh may rebuild the menu when it finishes.
///
/// Rebuilding replaces the menu, and replacing it destroys the `HMENU` that
/// `TrackPopupMenu` holds while a popup is on screen. A refresh started by a
/// tray click is forbidden from touching it: that popup is open right now, by
/// definition. Every other caller runs either with the popup already dismissed
/// or on a timer whose odds of landing inside an open menu are small.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuUpdate {
    Rebuild,
    Skip,
}

impl MenuUpdate {
    /// `Rebuild` wins when two requests coalesce.
    ///
    /// A caller that needs the menu rebuilt must not be silently downgraded to
    /// a tooltip update because it happened to arrive while a click-initiated
    /// refresh was in flight.
    fn strongest(self, other: Self) -> Self {
        if self == Self::Rebuild || other == Self::Rebuild {
            Self::Rebuild
        } else {
            Self::Skip
        }
    }
}

/// Replace the tray menu and tooltip. Must run on the main thread.
///
/// Only safe when no popup is on screen. `set_menu` drops the menu it
/// replaces, and dropping a `muda::Menu` calls `DestroyMenu` on its `HMENU` —
/// which is the very handle `TrackPopupMenu` holds while displaying. Doing
/// that mid-display tears the popup down under the cursor and drops the click
/// already in flight, since the old menu's `WM_COMMAND` subclass has been
/// detached and the new menu cannot resolve the old item ids.
///
/// Reaching the main thread is not enough to make it safe: `TrackPopupMenu`
/// blocks there in a modal loop that still pumps posted messages, which is
/// exactly how `run_on_main_thread` delivers work. Safety comes from the
/// caller knowing the popup is dismissed — see [`MenuUpdate`].
fn apply_menu(app: &AppHandle, model: &MenuModel) {
    if let Some(tray) = app.tray_by_id("main") {
        if let Ok(menu) = build_menu(app, model) {
            let _ = tray.set_menu(Some(menu));
        }
        let _ = tray.set_tooltip(Some(&model.tooltip));
        let _ = tray.set_icon(Some(tray_image(&model.badge)));
    }
}

/// Update the tooltip and icon, leaving the menu alone.
///
/// Safe from any thread at any time: the tooltip and icon live on the
/// notification-area entry, not on a menu Windows might be displaying. See
/// [`apply_menu`] for why the menu cannot be touched here.
fn render_status(app: &AppHandle, state: &Shared) {
    let model = state.lock().unwrap().view();
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = handle.tray_by_id("main") {
            let _ = tray.set_tooltip(Some(&model.tooltip));
            let _ = tray.set_icon(Some(tray_image(&model.badge)));
        }
    });
}

/// The static logo, or the usage badge drawn at the size the taskbar uses.
///
/// Drawn at the system's small-icon size rather than one fixed size: Windows
/// would otherwise resample the bitmap, and resampling pixel glyphs by a
/// non-integer factor (32px down to 24px at 150%) smears them.
fn tray_image(badge: &Badge) -> Image<'static> {
    let size = small_icon_size();
    match badge::render(badge, size) {
        Some(rgba) => Image::new_owned(rgba, size, size),
        None => tauri::include_image!("icons/tray.png"),
    }
}

/// `SM_CXSMICON`: 16 at 100% scaling, 24 at 150%, 32 at 200%.
fn small_icon_size() -> u32 {
    #[link(name = "user32")]
    extern "system" {
        fn GetSystemMetrics(index: i32) -> i32;
    }
    const SM_CXSMICON: i32 = 49;
    // SAFETY: takes an integer, touches no memory of ours.
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
    // Zero means the call failed; anything outside this range is not a tray.
    u32::try_from(size).map_or(16, |s| s.clamp(16, 64))
}

/// Rebuild the menu and tooltip from a background thread.
///
/// Callers must have good reason to believe no popup is on screen. The failure
/// notices use this because a notice reaches the user *only* through the menu —
/// `menu::tooltip` never reads `notice` — and they run after a menu item was
/// clicked, which dismisses the popup before the event is delivered.
///
/// "Good reason" is not a guarantee, and pretending otherwise is how this area
/// went wrong before. `spawn_switch` is the honest example: `cswap switch` takes
/// seconds, and nothing stops the user reopening the tray menu while it runs, so
/// its notice and its follow-up refresh can still land on a live popup. Windows
/// emits no event when a menu is dismissed, so there is no seam to close that
/// with. It is a known residual, recorded in spec section 5.
fn render_menu(app: &AppHandle, state: &Shared) {
    let model = state.lock().unwrap().view();
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || apply_menu(&handle, &model));
}

/// Fetch a fresh snapshot.
///
/// If a fetch is already running, this records that another one is wanted and
/// returns; the running fetch picks the request up rather than dropping it.
fn refresh(app: &AppHandle, state: &Shared, mut menu: MenuUpdate) {
    {
        let mut guard = state.lock().unwrap();
        if guard.busy {
            guard.pending = Some(match guard.pending {
                Some(queued) => queued.strongest(menu),
                None => menu,
            });
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
                    guard.error = None;
                    guard.cswap_missing = false;
                    guard.notice = None;
                }
                Err(err) => {
                    // A `cswap` deleted after we found it fails here as
                    // `NotFound` too, which is the honest answer: the install
                    // guide is exactly what that user needs.
                    guard.cswap_missing = matches!(err, CswapError::NotFound);
                    guard.error = Some(err.summary());
                }
            }
        }

        match menu {
            MenuUpdate::Rebuild => render_menu(app, state),
            MenuUpdate::Skip => render_status(app, state),
        }

        // Read `pending` AFTER rendering, and clear `busy` in the same lock
        // acquisition that decides to stop. Reading it earlier would lose a
        // request that arrived while `render` was running, and clearing the
        // flag in a separate step would let a request slip into the gap and
        // then be discarded by this refresh.
        let mut guard = state.lock().unwrap();
        match guard.pending.take() {
            Some(queued) => menu = queued,
            None => {
                guard.busy = false;
                busy.disarm();
                break;
            }
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
    // Fetch, but do NOT touch the menu on this path.
    //
    // The work this starts finishes about a second and a half later, when
    // `cswap list` returns — and by then the popup is certainly open, because
    // it opened on the button-up that followed the press we are handling.
    // Rebuilding the menu at that point destroys the `HMENU` Windows is
    // displaying. That is the bug this shape exists to avoid; it made the
    // menu flash shut and take two clicks to open.
    //
    // Rebuilding synchronously here would usually land before the popup —
    // Tauri posts tray events (`tauri::app` hands `TrayIconEvent` to
    // `proxy.send_event`), and Win32 hands posted messages to the loop ahead of
    // pending mouse input, so this normally runs before the button-up that
    // opens the menu. But "usually" is not a guarantee worth building on when
    // losing the race destroys a live menu, and it would only buy a fresher
    // first paint.
    //
    // So the click refreshes data, the tooltip and the icon. The menu the user is
    // reading was built by the last rebuild, and the next one picks up what
    // this fetch found.
    let app = app.clone();
    std::thread::spawn(move || refresh(&app, &state, MenuUpdate::Skip));
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
            std::thread::spawn(move || refresh(&app, &state, MenuUpdate::Rebuild));
        }
        "rotate" => spawn_switch(app, state, Switch::Rotate),
        "best" => spawn_switch(app, state, Switch::Best),
        "dashboard" => {
            let exe = state.lock().unwrap().exe.clone();
            // No `if let` without an else: a click that produces neither a
            // window nor a message is the silent failure spec section 8 rules
            // out. `spawn_switch` handles the same case the same way.
            let result = match exe {
                Some(exe) => cswap::open_dashboard(&exe),
                None => Err(CswapError::NotFound),
            };
            if let Err(err) = result {
                state.lock().unwrap().notice =
                    Some(format!("대시보드 실행 실패: {}", err.summary()));
                render_menu(&app, &state);
            }
        }
        "install" => {
            if let Err(err) = cswap::open_url(cswap::INSTALL_GUIDE_URL) {
                state.lock().unwrap().notice = Some(format!("설치 안내 열기 실패: {err}"));
                render_menu(&app, &state);
            }
        }
        "usage_icon" => {
            let want = !state.lock().unwrap().usage_icon;
            // The registry is the source of truth: on a failed write the check
            // mark stays where it was, as `autostart` does.
            match claude_swap_bar::settings::set_usage_icon(want) {
                Ok(()) => state.lock().unwrap().usage_icon = want,
                Err(err) => {
                    state.lock().unwrap().notice = Some(format!("아이콘 설정 저장 실패: {err}"))
                }
            }
            render_menu(&app, &state);
        }
        "autostart" => {
            let want = !claude_swap_bar::autostart::is_enabled();
            if let Err(err) = claude_swap_bar::autostart::set(want) {
                state.lock().unwrap().notice = Some(format!("시작프로그램 설정 실패: {err}"));
            }
            render_menu(&app, &state);
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
            render_menu(&app, &state);
            return;
        }
        refresh(&app, &state, MenuUpdate::Rebuild);
    });
}

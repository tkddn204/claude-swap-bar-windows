#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use claude_swap_bar::cswap::{self, CswapError, Switch};
use claude_swap_bar::menu::{self, MenuModel, View};
use claude_swap_bar::model::ListResponse;

use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem};
use tauri::tray::TrayIconBuilder;
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
    }));

    if state.lock().unwrap().exe.is_none() {
        state.lock().unwrap().error = Some(CswapError::NotFound.summary());
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

            let handle = app.handle().clone();
            tray.on_menu_event(move |app, event| {
                on_menu_event(app, event.id().as_ref().to_string());
            });

            // First refresh immediately, then every REFRESH_INTERVAL.
            let loop_handle = handle.clone();
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

/// Fetch a fresh snapshot. Returns immediately if one is already in flight.
fn refresh(app: &AppHandle, state: &Shared) {
    let exe = {
        let mut guard = state.lock().unwrap();
        if guard.busy {
            return;
        }
        guard.busy = true;
        guard.exe.clone()
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
        guard.busy = false;
    }

    render(app, state);
}

fn on_menu_event(app: &AppHandle, id: String) {
    let Some(state) = app.try_state::<Shared>().map(|s| s.inner().clone()) else {
        return;
    };
    let app = app.clone();

    match id.as_str() {
        "quit" => app.exit(0),
        "notice" => {}
        "refresh" => {
            std::thread::spawn(move || refresh(&app, &state));
        }
        "rotate" => spawn_switch(app, state, Switch::Rotate),
        "best" => spawn_switch(app, state, Switch::Best),
        "dashboard" => {
            // Filled in by Task 7.
        }
        "autostart" => {
            // Filled in by Task 6.
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

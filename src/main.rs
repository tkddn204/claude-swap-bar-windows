#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::menu::MenuBuilder;
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let menu = MenuBuilder::new(app).text("quit", "Quit").build()?;

            TrayIconBuilder::with_id("main")
                .icon(tauri::include_image!("icons/tray.png"))
                .tooltip("Claude Swap Bar")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| {
                    if event.id().as_ref() == "quit" {
                        app.exit(0);
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start Claude Swap Bar");
}

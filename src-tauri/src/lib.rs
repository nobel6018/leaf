//! leaf Mac 앱. 문서 보관함은 앱 데이터 폴더의 `library/`이고, 문서 하나가 폴더 하나다.
//!
//! @see docs/architecture.md

pub mod busy;
pub mod inbox;
mod library;
pub mod store;
pub mod transfer;
mod update;

use std::sync::Arc;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            update::add_update_menu(app)?;
            let app_data_dir = app.path().app_data_dir()?;
            let store = Arc::new(store::Store::new(&app_data_dir));
            store.clean_stale_parts();
            transfer::setup(app.handle(), store.clone(), &app_data_dir);
            inbox::setup(app.handle(), store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            library::list_documents,
            busy::busy_tasks,
            transfer::transfer_status,
            transfer::start_pairing,
            transfer::cancel_pairing,
            transfer::remove_paired_device,
            inbox::icloud_inbox_status,
        ])
        .run(tauri::generate_context!())
        .expect("leaf 앱을 시작하지 못했다");
}

//! leaf Mac 앱. 문서 보관함은 앱 데이터 폴더의 `library/`이고, 문서 하나가 폴더 하나다.
//!
//! @see docs/architecture.md

pub mod busy;
mod library;
mod update;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            update::add_update_menu(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![library::list_documents, busy::busy_tasks])
        .run(tauri::generate_context!())
        .expect("leaf 앱을 시작하지 못했다");
}

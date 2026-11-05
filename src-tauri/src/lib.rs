//! leaf Mac 앱. 문서 보관함은 앱 데이터 폴더의 `library/`이고, 문서 하나가 폴더 하나다.
//!
//! @see docs/architecture.md

pub mod busy;
mod export;
pub mod inbox;
mod library;
mod ocr;
mod pipeline;
mod search;
pub mod store;
pub mod transfer;
mod update;

use std::sync::Arc;

use tauri::{Listener, Manager};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(pipeline::Pipeline::default())
        .setup(|app| {
            update::add_update_menu(app)?;
            let app_data_dir = app.path().app_data_dir()?;
            let store = Arc::new(store::Store::new(&app_data_dir));
            store.clean_stale_parts();
            transfer::setup(app.handle(), store.clone(), &app_data_dir);
            inbox::setup(app.handle(), store);

            let handle = app.handle().clone();
            match search::SearchIndex::open(&app_data_dir.join("search.sqlite3")) {
                Ok(index) => {
                    app.manage(index);
                }
                Err(error) => eprintln!("{error}"),
            }
            let pipeline = app.state::<pipeline::Pipeline>().inner().clone();
            pipeline.start(handle.clone());
            // 전송(Wi-Fi, iCloud)이 새 문서나 페이지를 받으면 알린다. 받은 문서를 바로 처리한다
            let on_change = pipeline.clone();
            app.listen(transfer::EVENT_LIBRARY_CHANGED, move |event| {
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Changed {
                    document_id: String,
                }
                if let Ok(changed) = serde_json::from_str::<Changed>(event.payload()) {
                    on_change.enqueue(&changed.document_id, false);
                }
            });
            std::thread::spawn(move || {
                search::refresh_all(&handle);
                pipeline.enqueue_stale(&handle);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            busy::busy_tasks,
            transfer::transfer_status,
            transfer::start_pairing,
            transfer::cancel_pairing,
            transfer::remove_paired_device,
            inbox::icloud_inbox_status,
            library::list_documents,
            library::get_document,
            library::save_edits,
            library::set_processing_options,
            library::reprocess_document,
            library::raw_preview,
            library::get_ocr,
            library::get_summary,
            library::save_summary_field,
            pipeline::worker::processing_states,
            search::search_library,
            export::export_document,
            export::list_exports,
            export::reveal_export,
            export::share_export,
            export::save_export_as,
        ])
        .run(tauri::generate_context!())
        .expect("leaf 앱을 시작하지 못했다");
}

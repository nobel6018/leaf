//! 팜플렛 AI 정리와 AI 관련 내보내기 명령. 구현은 crates/scanshelf-ai, 여기서는 문서 위치, 작업 등록, 이벤트만 붙인다.
//!
//! scanshelf의 핵심은 스캔이다. AI 정리와 노션 CSV는 종류가 팜플렛인 문서에만 쓰는 선택 기능이고,
//! 책과 일반 문서는 사람이 누르지 않는 한 외부(구독 CLI)로 보내지 않는다.
//!
//! @see docs/ai.md

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use scanshelf_ai::{BundleOptions, DocxOptions, LlmConfig, NameSuggestion};
use scanshelf_core::DocumentKind;
use scanshelf_core::summary::PamphletSummary;
use serde::Serialize;
use tauri::Emitter;

use crate::export::{ExportFormat, export_to_dir, safe_file_name};
use crate::library::{document_dir, library_dir, load_applied};

fn document_name(dir: &Path) -> String {
    load_applied(dir).map(|(m, _)| m.name).unwrap_or_else(|_| dir.display().to_string())
}

async fn blocking<T: Send + 'static>(task: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(task).await.map_err(|e| format!("작업이 중간에 멈췄다: {e}"))?
}

#[tauri::command]
pub async fn summarize_pamphlet(app: tauri::AppHandle, document_id: String) -> Result<PamphletSummary, String> {
    let dir = document_dir(&app, &document_id)?;
    let summary = blocking(move || summarize_dir(&dir)).await?;
    let _ = app.emit("summary-changed", serde_json::json!({ "documentId": document_id }));
    Ok(summary)
}

fn summarize_dir(dir: &Path) -> Result<PamphletSummary, String> {
    let _busy = crate::busy::start(format!("AI 정리: {}", document_name(dir)));
    scanshelf_ai::summarize_pamphlet(dir, &LlmConfig::default()).map_err(|e| e.to_string())
}

/// 처리가 끝난 팜플렛에 정리가 아직 없으면 정리한다. 팜플렛만, 한 번만(다시 정리는 사람이 누른다)
pub fn summarize_if_pamphlet(app: &tauri::AppHandle, document_id: &str) {
    let Ok(dir) = document_dir(app, document_id) else { return };
    let Ok((manifest, _)) = load_applied(&dir) else { return };
    if manifest.kind != DocumentKind::Pamphlet || dir.join(scanshelf_core::summary::SUMMARY_FILE).exists() {
        return;
    }
    let app = app.clone();
    let document_id = document_id.to_string();
    std::thread::spawn(move || {
        if let Err(error) = summarize_dir(&dir) {
            eprintln!("팜플렛 자동 정리 실패 ({document_id}): {error}");
            return;
        }
        let _ = app.emit("summary-changed", serde_json::json!({ "documentId": document_id }));
        // 요약 뒤 AI 태그(팜플렛만, 사람이 고친 태그는 그대로). @see docs/organize.md
        crate::tagger::organize_after_summary(&app, &document_id);
    });
}

#[tauri::command]
pub async fn suggest_document_name(app: tauri::AppHandle, document_id: String) -> Result<Option<NameSuggestion>, String> {
    let dir = document_dir(&app, &document_id)?;
    blocking(move || {
        let _busy = crate::busy::start(format!("이름 추천: {}", document_name(&dir)));
        scanshelf_ai::suggest_document_name(&dir, &LlmConfig::default()).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn export_docx(app: tauri::AppHandle, document_id: String) -> Result<String, String> {
    let dir = document_dir(&app, &document_id)?;
    blocking(move || {
        let _busy = crate::busy::start(format!("Word 내보내기: {}", document_name(&dir)));
        let path = scanshelf_ai::export_docx(&dir, &DocxOptions::default()).map_err(|e| e.to_string())?;
        Ok(path.display().to_string())
    })
    .await
}

#[tauri::command]
pub async fn export_ai_bundle(app: tauri::AppHandle, document_id: String) -> Result<String, String> {
    let dir = document_dir(&app, &document_id)?;
    blocking(move || {
        let _busy = crate::busy::start(format!("AI 묶음 내보내기: {}", document_name(&dir)));
        let path = scanshelf_ai::export_ai_bundle(&dir, &BundleOptions::default()).map_err(|e| e.to_string())?;
        Ok(path.display().to_string())
    })
    .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotionExport {
    /// CSV와 PDF가 함께 든 폴더
    pub folder: String,
    pub csv: String,
    pub rows: usize,
    pub pdfs: usize,
    pub skipped: Vec<String>,
}

/// 세션(박람회명) 하나를 노션 CSV로. PDF를 같이 만들어 한 폴더에 두고, "원본 파일명" 칸에 PDF 이름을 넣는다
#[tauri::command]
pub async fn export_notion_csv(app: tauri::AppHandle, session: String) -> Result<NotionExport, String> {
    let library = library_dir(&app)?;
    let base = app_exports_dir(&app)?;
    let result = blocking(move || {
        let _busy = crate::busy::start(format!("노션 CSV 내보내기: {session}"));
        notion_export(&library, &base, &session)
    })
    .await?;
    let _ = tauri_plugin_opener::reveal_item_in_dir(&result.csv);
    Ok(result)
}

fn app_exports_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(base.join("exports").join("notion"))
}

pub fn notion_export(library: &Path, exports_base: &Path, session: &str) -> Result<NotionExport, String> {
    let dirs = scanshelf_ai::notion::session_document_dirs(library, session);
    if dirs.is_empty() {
        return Err(format!("'{session}' 묶음에 문서가 없다"));
    }
    let folder = exports_base.join(safe_file_name(session));
    std::fs::create_dir_all(&folder).map_err(|e| format!("{} 만들기 실패: {e}", folder.display()))?;

    let mut skipped = Vec::new();
    let mut pdf_names = std::collections::HashMap::new();
    for dir in &dirs {
        match fresh_pdf(dir) {
            Ok(pdf) => {
                let name = pdf.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                std::fs::copy(&pdf, folder.join(&name)).map_err(|e| format!("{name} 복사 실패: {e}"))?;
                pdf_names.insert(dir.clone(), name);
            }
            Err(error) => skipped.push(format!("{}: PDF를 만들지 못함({error})", document_name(dir))),
        }
    }
    let csv = folder.join(format!("{}.csv", safe_file_name(session)));
    let report = scanshelf_ai::export_notion_csv_with_files(&dirs, &csv, &|dir| pdf_names.get(dir).cloned())
        .map_err(|e| e.to_string())?;
    skipped.extend(report.skipped.iter().map(|s| format!("{}: {}", document_name(&s.dir), s.reason)));
    Ok(NotionExport {
        folder: folder.display().to_string(),
        csv: csv.display().to_string(),
        rows: report.rows,
        pdfs: pdf_names.len(),
        skipped,
    })
}

/// 내보낸 PDF가 처리 결과보다 오래됐거나 없으면 새로 만든다
fn fresh_pdf(dir: &Path) -> Result<PathBuf, String> {
    let (manifest, _) = load_applied(dir)?;
    let pdf = dir.join(crate::export::EXPORTS_DIR).join(format!("{}.pdf", safe_file_name(&manifest.name)));
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let processed = modified(&dir.join(scanshelf_core::processing::PROCESSING_FILE)).unwrap_or(SystemTime::UNIX_EPOCH);
    match modified(&pdf) {
        Some(made) if made >= processed => Ok(pdf),
        _ => export_to_dir(dir, ExportFormat::Pdf),
    }
}

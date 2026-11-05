//! 내보내기: 텍스트 레이어 PDF, JPG 묶음(zip), 텍스트. 결과는 문서 폴더 `exports/`에 두고,
//! 화면에서 Finder로 보기, 공유 시트, 다른 이름으로 저장을 고른다. 개수 제한은 없다.
//!
//! @see docs/export.md

#[cfg(target_os = "macos")]
pub mod pdf_macos;
#[cfg(target_os = "macos")]
mod share_macos;

use std::io::Write;
use std::path::{Path, PathBuf};

use leaf_core::ocr::OcrPage;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

use crate::library::{document_dir, load_applied, load_processing, read_json, write_atomic};

pub const EXPORTS_DIR: &str = "exports";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Pdf,
    Jpg,
    Txt,
}

pub struct PdfPage {
    pub image: PathBuf,
    pub ocr: Option<OcrPage>,
}

/// 파일 이름에 못 쓰는 글자를 바꾼다. 문서 이름은 사람이 붙여 `/`나 `:`가 들어갈 수 있다
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim_start_matches('.').to_string();
    if cleaned.is_empty() { "leaf".to_string() } else { cleaned }
}

/// 처리된 페이지를 문서 순서대로. 처리 전 문서면 비어 있다
pub fn collect_pages(dir: &Path) -> Vec<PdfPage> {
    load_processing(dir)
        .outputs
        .iter()
        .map(|o| PdfPage {
            image: dir.join(&o.image),
            ocr: o.ocr.as_ref().and_then(|p| read_json(&dir.join(p)).ok().flatten()),
        })
        .filter(|p| p.image.exists())
        .collect()
}

pub fn build_text(pages: &[PdfPage]) -> String {
    let mut out = String::new();
    for (index, page) in pages.iter().enumerate() {
        if index > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&format!("[{}쪽]\n", index + 1));
        if let Some(ocr) = &page.ocr {
            out.push_str(ocr.text.trim());
        }
        out.push('\n');
    }
    out
}

/// JPEG는 이미 압축돼 있어 zip은 저장만 한다(다시 압축해도 줄지 않고 느리기만 하다)
pub fn write_jpg_zip(path: &Path, base_name: &str, pages: &[PdfPage]) -> Result<(), String> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let width = pages.len().to_string().len().max(2);
        for (index, page) in pages.iter().enumerate() {
            let bytes = std::fs::read(&page.image).map_err(|e| format!("{} 읽기 실패: {e}", page.image.display()))?;
            zip.start_file(format!("{base_name}_{:0width$}.jpg", index + 1), options)
                .map_err(|e| e.to_string())?;
            zip.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    write_atomic(path, buffer.get_ref()).map_err(|e| e.to_string())
}

/// 내보내 문서 폴더에 저장하고 경로를 돌려준다. 같은 형식은 덮어쓴다
pub fn export_to_dir(dir: &Path, format: ExportFormat) -> Result<PathBuf, String> {
    let (manifest, _) = load_applied(dir)?;
    let pages = collect_pages(dir);
    if pages.is_empty() {
        return Err("처리된 페이지가 없다. 처리가 끝난 뒤 내보낸다".into());
    }
    let base = safe_file_name(&manifest.name);
    let exports = dir.join(EXPORTS_DIR);
    std::fs::create_dir_all(&exports).map_err(|e| e.to_string())?;
    match format {
        ExportFormat::Pdf => {
            let path = exports.join(format!("{base}.pdf"));
            #[cfg(target_os = "macos")]
            {
                // CoreGraphics는 경로에 바로 쓰므로 임시 이름에 쓴 뒤 바꾼다
                let temp = exports.join(format!(".{base}.pdf.tmp"));
                pdf_macos::write_pdf(&temp, &manifest.name, &pages)?;
                std::fs::rename(&temp, &path).map_err(|e| e.to_string())?;
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = &path;
                return Err("PDF 내보내기는 아직 macOS만 된다".into());
            }
            #[allow(unreachable_code)]
            Ok(path)
        }
        ExportFormat::Jpg => {
            let path = exports.join(format!("{base}_jpg.zip"));
            write_jpg_zip(&path, &base, &pages)?;
            Ok(path)
        }
        ExportFormat::Txt => {
            let path = exports.join(format!("{base}.txt"));
            write_atomic(&path, build_text(&pages).as_bytes()).map_err(|e| e.to_string())?;
            Ok(path)
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified_at: String,
}

#[tauri::command]
pub async fn export_document(app: tauri::AppHandle, document_id: String, format: ExportFormat) -> Result<ExportFile, String> {
    let dir = document_dir(&app, &document_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = crate::busy::start(format!("내보내기: {document_id}"));
        let path = export_to_dir(&dir, format)?;
        describe(&path).ok_or_else(|| "내보낸 파일을 찾지 못했다".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn describe(path: &Path) -> Option<ExportFile> {
    let meta = std::fs::metadata(path).ok()?;
    let modified: chrono::DateTime<chrono::Utc> = meta.modified().ok()?.into();
    Some(ExportFile {
        path: path.to_string_lossy().into_owned(),
        name: path.file_name()?.to_string_lossy().into_owned(),
        size: meta.len(),
        modified_at: modified.to_rfc3339(),
    })
}

#[tauri::command]
pub fn list_exports(app: tauri::AppHandle, document_id: String) -> Result<Vec<ExportFile>, String> {
    let dir = document_dir(&app, &document_id)?.join(EXPORTS_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut files: Vec<ExportFile> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .filter_map(|p| describe(&p))
        .collect();
    files.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(files)
}

/// 화면이 준 경로가 보관함 안의 exports/ 파일인지 확인한다
fn checked_export_path(app: &tauri::AppHandle, path: &str) -> Result<PathBuf, String> {
    let library = crate::library::library_dir(app)?;
    let canonical = std::fs::canonicalize(path).map_err(|e| format!("파일을 찾지 못했다: {e}"))?;
    let library = std::fs::canonicalize(&library).map_err(|e| e.to_string())?;
    let in_exports = canonical
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == EXPORTS_DIR);
    if !canonical.starts_with(&library) || !in_exports {
        return Err("내보낸 파일만 열 수 있다".into());
    }
    Ok(canonical)
}

#[tauri::command]
pub fn reveal_export(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let path = checked_export_path(&app, &path)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| e.to_string())
}

/// 공유 시트를 단추 자리(창 기준 x, y)에 띄운다
#[tauri::command]
pub fn share_export(app: tauri::AppHandle, window: tauri::WebviewWindow, path: String, x: f64, y: f64) -> Result<(), String> {
    let path = checked_export_path(&app, &path)?;
    #[cfg(target_os = "macos")]
    {
        let view = window.ns_view().map_err(|e| e.to_string())? as usize;
        app.run_on_main_thread(move || share_macos::show_share_picker(view as *mut std::ffi::c_void, &path, x, y))
            .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, path, x, y);
        Err("공유 시트는 macOS만 된다".into())
    }
}

/// 저장 창을 띄워 고른 곳에 복사한다. 취소하면 None
#[tauri::command]
pub async fn save_export_as(app: tauri::AppHandle, path: String) -> Result<Option<String>, String> {
    let source = checked_export_path(&app, &path)?;
    let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut dialog = app.dialog().file().set_file_name(&name);
    if let Ok(downloads) = app.path().download_dir() {
        dialog = dialog.set_directory(downloads);
    }
    let Some(target) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    let target = target.into_path().map_err(|e| e.to_string())?;
    std::fs::copy(&source, &target).map_err(|e| format!("복사 실패: {e}"))?;
    Ok(Some(target.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 파일_이름에_못_쓰는_글자를_바꾼다() {
        assert_eq!(safe_file_name("2026 박람회/한빛:로보틱스"), "2026 박람회_한빛_로보틱스");
        assert_eq!(safe_file_name("..숨김"), "숨김");
        assert_eq!(safe_file_name("  "), "leaf");
    }

    #[test]
    fn 텍스트는_쪽마다_머리를_단다() {
        let page = |text: &str| PdfPage {
            image: PathBuf::new(),
            ocr: Some(OcrPage {
                engine: "test".into(),
                image_width: 1,
                image_height: 1,
                text: text.into(),
                blocks: vec![],
            }),
        };
        assert_eq!(build_text(&[page("첫 쪽"), page("둘째 쪽")]), "[1쪽]\n첫 쪽\n\n\n[2쪽]\n둘째 쪽\n");
    }
}

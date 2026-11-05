//! 문서 보관함 조회. 형식은 leaf-core가 정한다(docs/document-format.md)

use std::fs;
use std::path::{Path, PathBuf};

use leaf_core::{DocumentKind, Manifest};
use serde::Serialize;
use tauri::Manager;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSummary {
    pub id: String,
    pub name: String,
    pub kind: DocumentKind,
    pub session: Option<String>,
    pub page_count: usize,
    pub created_at: String,
}

pub fn library_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let base = app.path().app_data_dir().map_err(|e| format!("앱 데이터 폴더를 찾지 못했다: {e}"))?;
    Ok(base.join("library"))
}

#[tauri::command]
pub fn list_documents(app: tauri::AppHandle) -> Result<Vec<DocumentSummary>, String> {
    let dir = library_dir(&app)?;
    Ok(scan_library(&dir))
}

/// 읽지 못한 문서는 건너뛴다. 전송 중인 문서 하나 때문에 보관함 전체가 안 보이면 안 된다
fn scan_library(dir: &Path) -> Vec<DocumentSummary> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut documents: Vec<DocumentSummary> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| match Manifest::load(&entry.path()) {
            Ok(manifest) => Some(summarize(&manifest)),
            Err(error) => {
                eprintln!("문서를 건너뜀: {error}");
                None
            }
        })
        .collect();
    documents.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    documents
}

fn summarize(manifest: &Manifest) -> DocumentSummary {
    DocumentSummary {
        id: manifest.id.to_string(),
        name: manifest.name.clone(),
        kind: manifest.kind,
        session: manifest.session.clone(),
        page_count: manifest.pages.len(),
        created_at: manifest.created_at.to_rfc3339(),
    }
}

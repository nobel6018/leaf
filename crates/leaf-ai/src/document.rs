//! 문서 폴더 읽기. manifest에 Mac 수정(edits.json)을 덮고, 출력 페이지를 문서 순서대로 OCR과 짝짓는다.
//!
//! @see docs/ai.md

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Local;
use leaf_core::Manifest;
use leaf_core::ocr::OcrPage;
use leaf_core::overrides::{EDITS_FILE, Edits};
use leaf_core::processing::{OutputPage, PROCESSING_FILE, Processing};
use leaf_core::summary::{PamphletSummary, SUMMARY_FILE};
use serde::de::DeserializeOwned;

use crate::error::{AiError, io_error};
use crate::fsutil::safe_join;

#[derive(Debug, Clone)]
pub struct DocumentPage {
    /// 1부터 센다
    pub number: usize,
    pub output: OutputPage,
    pub image_path: PathBuf,
    /// OCR 전이거나 결과 파일이 없으면 없음
    pub ocr: Option<OcrPage>,
}

#[derive(Debug, Clone)]
pub struct LoadedDocument {
    pub dir: PathBuf,
    /// edits.json을 덮은 결과
    pub manifest: Manifest,
    pub pages: Vec<DocumentPage>,
    pub summary: Option<PamphletSummary>,
}

impl LoadedDocument {
    pub fn load(document_dir: &Path) -> Result<LoadedDocument, AiError> {
        let manifest = Manifest::load(document_dir)?;
        let edits: Edits = read_json_optional(&document_dir.join(EDITS_FILE))?.unwrap_or_default();
        let manifest = edits.apply(&manifest);

        let processing_path = document_dir.join(PROCESSING_FILE);
        let processing: Processing = read_json_optional(&processing_path)?
            .ok_or_else(|| AiError::NotProcessed(document_dir.to_path_buf()))?;

        // 처리 뒤 Mac에서 순서를 바꾸거나 페이지를 지웠을 수 있어 manifest 순서를 따른다
        let mut outputs: Vec<(usize, OutputPage)> = processing
            .outputs
            .into_iter()
            .filter_map(|output| {
                let position = manifest
                    .pages
                    .iter()
                    .position(|p| p.id == output.source_page)?;
                Some((position, output))
            })
            .collect();
        outputs.sort_by_key(|(position, _)| *position);

        let mut pages = Vec::with_capacity(outputs.len());
        for (index, (_, output)) in outputs.into_iter().enumerate() {
            let image_path = safe_join(document_dir, &output.image)?;
            let ocr = match &output.ocr {
                Some(relative) => read_json_optional(&safe_join(document_dir, relative)?)?,
                None => None,
            };
            pages.push(DocumentPage {
                number: index + 1,
                output,
                image_path,
                ocr,
            });
        }

        let summary = read_json_optional(&document_dir.join(SUMMARY_FILE))?;
        Ok(LoadedDocument {
            dir: document_dir.to_path_buf(),
            manifest,
            pages,
            summary,
        })
    }

    /// 촬영한 날(이 Mac의 시간대). 박람회 방문일로 쓴다
    pub fn capture_date(&self) -> String {
        self.manifest
            .created_at
            .with_timezone(&Local)
            .format("%Y-%m-%d")
            .to_string()
    }

    pub fn has_ocr_text(&self) -> bool {
        self.pages
            .iter()
            .any(|p| p.ocr.as_ref().is_some_and(|o| !o.text.trim().is_empty()))
    }
}

pub(crate) fn read_json_optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, AiError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(path)(e)),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|source| AiError::Json {
            path: path.to_path_buf(),
            source,
        })
}

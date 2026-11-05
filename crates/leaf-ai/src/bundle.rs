//! AI에 넘길 묶음(팜플렛용 내보내기). `document.md` 하나와 `images/` 폴더로, 채팅 창에 그대로 올리면 된다.
//!
//! 이미지는 긴 변을 줄인 JPEG다. 원본 크기 사진은 업로드 한도를 넘기 쉽고, 모델도 내부에서 줄여 읽는다.
//!
//! @see docs/ai.md

use std::fs;
use std::path::{Path, PathBuf};

use leaf_core::DocumentKind;
use leaf_core::summary::PamphletSummary;

use crate::document::LoadedDocument;
use crate::error::{AiError, io_error};
use crate::fsutil::sanitize_file_name;
use crate::images::{encode_jpeg, load_downscaled};
use crate::notion::relevance_label;

pub const BUNDLE_MARKDOWN: &str = "document.md";
pub const BUNDLE_IMAGES_DIR: &str = "images";

#[derive(Debug, Clone)]
pub struct BundleOptions {
    pub include_images: bool,
    /// 긴 변 상한(px). Claude는 긴 변 1568px 안팎으로 줄여 읽으므로 2000이면 손해가 없다
    pub image_max_edge: u32,
    pub jpeg_quality: u8,
}

impl Default for BundleOptions {
    fn default() -> Self {
        Self {
            include_images: true,
            image_max_edge: 2_000,
            jpeg_quality: 85,
        }
    }
}

/// `exports/<문서 이름>-ai/` 폴더를 새로 만들고 경로를 돌려준다. 같은 이름 폴더가 있으면 바꿔 쓴다
pub fn export_ai_bundle(document_dir: &Path, options: &BundleOptions) -> Result<PathBuf, AiError> {
    let document = LoadedDocument::load(document_dir)?;
    let exports = document_dir.join("exports");
    let folder = format!(
        "{}-ai",
        sanitize_file_name(&document.manifest.name, &document.manifest.id.to_string())
    );
    let target = exports.join(&folder);
    // 다 만든 뒤 이름을 바꿔, 만드는 도중의 폴더를 AI에 올리는 일이 없게 한다
    let staging = exports.join(format!(".{folder}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = write_bundle(&document, options, &staging).and_then(|()| {
        if target.exists() {
            fs::remove_dir_all(&target).map_err(io_error(&target))?;
        }
        fs::rename(&staging, &target).map_err(io_error(&target))
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result.map(|()| target)
}

fn write_bundle(
    document: &LoadedDocument,
    options: &BundleOptions,
    dir: &Path,
) -> Result<(), AiError> {
    let images_dir = dir.join(BUNDLE_IMAGES_DIR);
    fs::create_dir_all(&images_dir).map_err(io_error(&images_dir))?;
    let mut image_names = Vec::with_capacity(document.pages.len());
    for page in &document.pages {
        let name = format!("page-{:02}.jpg", page.number);
        if options.include_images && page.image_path.is_file() {
            let image = load_downscaled(&page.image_path, options.image_max_edge)?;
            let bytes = encode_jpeg(&image, options.jpeg_quality, &page.image_path)?;
            let path = images_dir.join(&name);
            fs::write(&path, bytes).map_err(io_error(&path))?;
            image_names.push(Some(name));
        } else {
            image_names.push(None);
        }
    }
    let markdown = bundle_markdown(document, &image_names);
    let path = dir.join(BUNDLE_MARKDOWN);
    fs::write(&path, markdown).map_err(io_error(&path))
}

pub fn bundle_markdown(document: &LoadedDocument, image_names: &[Option<String>]) -> String {
    let manifest = &document.manifest;
    let kind = match manifest.kind {
        DocumentKind::Pamphlet => "팜플렛",
        DocumentKind::Book => "책",
        DocumentKind::Document => "문서",
    };
    let mut md = format!("# {}\n\n| 항목 | 값 |\n|---|---|\n", cell(&manifest.name));
    md += &format!("| 종류 | {kind} |\n");
    md += &format!(
        "| 박람회(세션) | {} |\n",
        cell(manifest.session.as_deref().unwrap_or(""))
    );
    md += &format!("| 촬영일 | {} |\n", document.capture_date());
    md += &format!("| 페이지 수 | {} |\n", document.pages.len());
    md += &format!("| 문서 id | {} |\n\n", manifest.id);

    if let Some(summary) = &document.summary {
        md += &summary_section(summary);
    }

    md += "## 페이지별 OCR 글자\n\n";
    md += "인쇄물을 OCR로 읽은 글자입니다. 오탈자가 있을 수 있고, 같은 번호의 이미지가 `images/` 폴더에 있습니다.\n\n";
    for (page, image) in document.pages.iter().zip(image_names) {
        md += &format!(
            "<!-- page {} -->\n### 페이지 {}\n\n",
            page.number, page.number
        );
        if let Some(image) = image {
            md += &format!("![페이지 {}]({BUNDLE_IMAGES_DIR}/{image})\n\n", page.number);
        }
        let text = page.ocr.as_ref().map(|o| o.text.trim()).unwrap_or("");
        if text.is_empty() {
            md += "(OCR 글자 없음)\n\n";
        } else {
            md += text;
            md += "\n\n";
        }
    }
    md
}

fn summary_section(summary: &PamphletSummary) -> String {
    let relevance = summary
        .relevance
        .iter()
        .map(|r| relevance_label(*r))
        .collect::<Vec<_>>()
        .join(", ");
    let rows = [
        ("회사명", summary.company.as_str()),
        ("국가", &summary.country),
        ("박람회명", &summary.event),
        ("날짜", &summary.date),
        ("주력 제품/기술", &summary.products),
        ("주요 스펙", &summary.key_specs),
        ("타깃 시장/고객", &summary.target_market),
        ("레퍼런스", &summary.references),
        ("우리 회사와의 관련성", &relevance),
        ("관련성 근거", &summary.relevance_note),
        ("연락처", &summary.contact),
    ];
    let mut md = format!(
        "## AI 정리\n\n{} 생성. NULL SPACE(데이터센터 구축 로봇) 기준 관련성입니다.\n\n| 항목 | 내용 |\n|---|---|\n",
        summary.generated_by
    );
    for (label, value) in rows {
        md += &format!("| {label} | {} |\n", cell(value));
    }
    md += "\n";
    md
}

/// 마크다운 표 칸 안에서 `|`와 줄바꿈이 표를 깨지 않게 한다
fn cell(text: &str) -> String {
    text.trim()
        .replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace('\n', "<br>")
}

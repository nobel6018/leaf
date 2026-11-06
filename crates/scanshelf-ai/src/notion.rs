//! 노션 데이터베이스로 가져올 CSV. 팜플렛 한 부가 한 줄이다.
//!
//! RFC 4180 따옴표 규칙, CRLF 줄 끝, UTF-8 BOM(Excel이 한글을 깨지 않게). 칸 안의 줄바꿈은 따옴표 안에 그대로 둔다.
//!
//! @see docs/ai.md

use std::fs;
use std::path::{Path, PathBuf};

use scanshelf_core::Manifest;
use scanshelf_core::summary::{PamphletSummary, Relevance, SUMMARY_FILE};
use serde::Serialize;

use crate::document::read_json_optional;
use crate::error::AiError;
use crate::fsutil::write_atomic;

/// 노션 속성 이름이자 열 순서. 바꾸면 사용자가 만든 노션 데이터베이스와 어긋난다
pub const NOTION_COLUMNS: [&str; 12] = [
    "회사명",
    "국가",
    "박람회명",
    "날짜",
    "주력 제품/기술",
    "주요 스펙",
    "타깃 시장/고객",
    "레퍼런스",
    "우리 회사와의 관련성",
    // 분류만으로는 나중에 조사할 때 거를 수 없어 판단 이유를 따로 둔다(2026-11-05 사용자 결정)
    "관련성 근거",
    "연락처",
    "원본 파일명",
];

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedDocument {
    pub dir: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotionCsvReport {
    pub path: PathBuf,
    pub rows: usize,
    /// AI 정리가 없거나 읽지 못한 문서. 한 부 때문에 전체 내보내기를 막지 않는다
    pub skipped: Vec<SkippedDocument>,
}

/// 노션 다중 선택 값. 노션은 가져올 때 쉼표로 나눈다
pub fn relevance_label(relevance: Relevance) -> &'static str {
    match relevance {
        Relevance::Competitor => "경쟁사",
        Relevance::PartnerCandidate => "협력 후보",
        Relevance::Supplier => "공급사",
        Relevance::Unrelated => "무관",
    }
}

pub fn export_notion_csv(
    document_dirs: &[PathBuf],
    out_path: &Path,
) -> Result<NotionCsvReport, AiError> {
    export_notion_csv_with_files(document_dirs, out_path, &|_| None)
}

/// source_file: 문서 폴더마다 "원본 파일명" 칸에 넣을 값. 앱은 함께 내보낸 PDF 이름을 준다.
/// None이면 summary.json의 값(문서 이름)을 쓴다
pub fn export_notion_csv_with_files(
    document_dirs: &[PathBuf],
    out_path: &Path,
    source_file: &dyn Fn(&Path) -> Option<String>,
) -> Result<NotionCsvReport, AiError> {
    let mut summaries = Vec::new();
    let mut skipped = Vec::new();
    for dir in document_dirs {
        match read_json_optional::<PamphletSummary>(&dir.join(SUMMARY_FILE)) {
            Ok(Some(mut summary)) => {
                if let Some(file) = source_file(dir) {
                    summary.source_file = file;
                }
                summaries.push(summary)
            }
            Ok(None) => skipped.push(SkippedDocument {
                dir: dir.clone(),
                reason: "AI 정리(summary.json)가 없다".into(),
            }),
            Err(error) => skipped.push(SkippedDocument {
                dir: dir.clone(),
                reason: error.to_string(),
            }),
        }
    }
    let bytes = notion_csv_bytes(&summaries)?;
    write_atomic(out_path, &bytes)?;
    Ok(NotionCsvReport {
        path: out_path.to_path_buf(),
        rows: summaries.len(),
        skipped,
    })
}

pub fn notion_csv_bytes(summaries: &[PamphletSummary]) -> Result<Vec<u8>, AiError> {
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .quote_style(csv::QuoteStyle::Necessary)
        .from_writer(UTF8_BOM.to_vec());
    writer.write_record(NOTION_COLUMNS)?;
    for summary in summaries {
        writer.write_record(row(summary))?;
    }
    writer
        .into_inner()
        .map_err(|e| AiError::Csv(csv::Error::from(e.into_error())))
}

fn row(summary: &PamphletSummary) -> [String; 12] {
    let relevance = summary
        .relevance
        .iter()
        .map(|r| relevance_label(*r))
        .collect::<Vec<_>>()
        .join(", ");
    [
        &summary.company,
        &summary.country,
        &summary.event,
        &summary.date,
        &summary.products,
        &summary.key_specs,
        &summary.target_market,
        &summary.references,
        &relevance,
        &summary.relevance_note,
        &summary.contact,
        &summary.source_file,
    ]
    .map(|field| normalize_newlines(field))
}

/// 칸 안 줄바꿈은 LF로 맞춘다. 줄 끝 CRLF와 섞이면 일부 프로그램이 칸을 잘못 나눈다
fn normalize_newlines(field: &str) -> String {
    field.replace("\r\n", "\n").replace('\r', "\n")
}

/// 보관함에서 세션(박람회명)이 같은 문서 폴더를 찾는다. Mac에서 고친 세션(edits.json)을 따른다.
/// 읽지 못한 폴더는 건너뛴다. 촬영 순서로 정렬한다
pub fn session_document_dirs(library_dir: &Path, session: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(library_dir) else {
        return Vec::new();
    };
    let mut found: Vec<(chrono::DateTime<chrono::Utc>, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| {
            let manifest = Manifest::load(&path).ok()?;
            let edits = scanshelf_core::overrides::Edits::load(&path).ok()?;
            let merged = edits.apply(&manifest);
            (merged.session.as_deref() == Some(session)).then_some((merged.created_at, path))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, path)| path).collect()
}

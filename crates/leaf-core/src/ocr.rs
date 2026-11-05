//! 페이지별 OCR 결과(`ocr/<출력 페이지 id>.json`). Mac이 쓰고, 검색, 텍스트 레이어 PDF, Word, AI 정리가 읽는다.
//!
//! @see docs/document-format.md

use serde::{Deserialize, Serialize};

/// 이미지 기준 정규화 사각형(0~1). 원점은 왼쪽 위
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockKind {
    Title,
    Paragraph,
    List,
    Table,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrLine {
    pub text: String,
    pub bbox: Rect,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrBlock {
    pub kind: BlockKind,
    pub text: String,
    pub bbox: Rect,
    pub lines: Vec<OcrLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrPage {
    /// 예: `vision-recognize-documents`, `vision-recognize-text`
    pub engine: String,
    pub image_width: u32,
    pub image_height: u32,
    /// 읽는 순서대로 이은 전체 글자. 블록 사이는 빈 줄
    pub text: String,
    pub blocks: Vec<OcrBlock>,
}

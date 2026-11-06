//! 팜플렛 AI 정리(`summary.json`). 노션 데이터베이스 속성 = CSV 열 순서와 같다.
//!
//! @see docs/features.md

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const SUMMARY_FILE: &str = "summary.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Relevance {
    Competitor,
    PartnerCandidate,
    Supplier,
    Unrelated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PamphletSummary {
    pub company: String,
    pub country: String,
    pub event: String,
    /// YYYY-MM-DD. 모르면 빈 문자열
    pub date: String,
    pub products: String,
    pub key_specs: String,
    pub target_market: String,
    pub references: String,
    pub relevance: Vec<Relevance>,
    /// 관련성 판단 근거 한두 문장(NULL SPACE: 데이터센터 구축 로봇 기준)
    pub relevance_note: String,
    pub contact: String,
    pub source_file: String,
    /// 예: `claude -p`
    pub generated_by: String,
    pub generated_at: DateTime<Utc>,
    /// 사람이 고친 필드. 다시 생성할 때 덮지 않는다.
    /// 값은 summary.json의 키 이름(camelCase) 그대로다. 예: `"company"`, `"keySpecs"`. 화면도 이 이름으로 기록한다
    #[serde(default)]
    pub edited_fields: Vec<String>,
}

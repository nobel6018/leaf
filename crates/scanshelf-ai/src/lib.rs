//! 팜플렛 AI 정리, 노션 CSV, Word 내보내기, AI에 넘길 묶음.
//!
//! AI 호출은 API 키 대신 구독 로그인된 CLI(`claude -p`, 대안 `codex exec`)를 하위 프로세스로 부른다.
//! 공개 저장소라 비밀 값을 두지 않기 위해서다. 함수는 모두 동기 함수이고 문서 폴더 경로를 받는다.
//! Tauri 명령에서 부를 때는 `spawn_blocking`으로 감싼다.
//!
//! @see docs/ai.md

pub mod bundle;
pub mod cli;
pub mod document;
pub mod docx;
mod error;
mod fsutil;
mod images;
pub mod json;
pub mod naming;
pub mod notion;
pub mod summarize;

pub use bundle::{BundleOptions, export_ai_bundle};
pub use cli::{LlmConfig, Provider};
pub use docx::{DocxOptions, export_docx};
pub use error::AiError;
pub use naming::{NameSuggestion, suggest_document_name};
pub use notion::{NotionCsvReport, export_notion_csv, export_notion_csv_with_files};
pub use summarize::summarize_pamphlet;

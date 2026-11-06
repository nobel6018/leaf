use std::path::PathBuf;

use scanshelf_core::ManifestError;

/// 화면에 그대로 보여 줄 수 있게 메시지는 한국어로 쓴다
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("파일 입출력 실패 ({path}): {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("JSON 형식 오류 ({path}): {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("문서 폴더 밖을 가리키는 경로라 읽지 않는다: {0}")]
    UnsafePath(String),
    #[error("아직 처리하지 않은 문서다(processing.json 없음): {0}")]
    NotProcessed(PathBuf),
    #[error("OCR 글자가 없다. OCR을 먼저 돌려 주세요: {0}")]
    NoOcrText(PathBuf),
    #[error("{program} CLI를 찾을 수 없습니다. 설치하고 로그인했는지 확인해 주세요")]
    CliNotFound { program: String },
    #[error(
        "{program} CLI에 로그인되어 있지 않습니다. 터미널에서 `{program}`을 실행해 로그인해 주세요: {detail}"
    )]
    NotLoggedIn { program: String, detail: String },
    #[error("{program} 실행이 실패했습니다(종료 코드 {code:?}): {detail}")]
    CliFailed {
        program: String,
        code: Option<i32>,
        detail: String,
    },
    #[error("{program}가 {secs}초 안에 끝나지 않아 멈췄습니다")]
    Timeout { program: String, secs: u64 },
    #[error("{program}가 빈 응답을 돌려줬습니다")]
    EmptyResponse { program: String },
    #[error("AI 응답을 JSON으로 읽지 못했습니다(다시 요청해도 실패): {detail}")]
    InvalidResponse { detail: String },
    #[error("CSV 쓰기 실패: {0}")]
    Csv(#[from] csv::Error),
    #[error("이미지 처리 실패 ({path}): {source}")]
    Image {
        path: PathBuf,
        source: image::ImageError,
    },
    #[error("Word 파일 쓰기 실패: {0}")]
    Docx(String),
}

pub(crate) fn io_error(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> AiError {
    let path = path.into();
    move |source| AiError::Io { path, source }
}

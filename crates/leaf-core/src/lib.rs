//! leaf 문서 형식. 문서 하나는 폴더 하나이고, 그 안의 `manifest.json`이 페이지 목록과 촬영 정보를 담는다.
//!
//! iPhone은 원본 사진과 감지한 꼭짓점만 기록하고, 자르기와 보정은 Mac이 한다.
//! 원본을 지우지 않으므로 Mac에서 처리 방식을 바꾼 뒤 예전 문서도 다시 처리할 수 있다.
//!
//! @see docs/document-format.md

pub mod fsio;
pub mod ocr;
pub mod overrides;
pub mod processing;
pub mod summary;
pub mod transfer;

use std::fs;
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MANIFEST_FILE: &str = "manifest.json";

/// 형식이 바뀌면 올린다. 모르는 버전은 읽지 않는다(새 iPhone 앱이 보낸 문서를 옛 Mac 앱이 망가뜨리지 않게)
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("manifest 입출력 실패 ({path}): {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("manifest 형식 오류 ({path}): {source}")]
    Parse { path: PathBuf, source: serde_json::Error },
    #[error("지원하지 않는 schemaVersion {found} (이 앱은 {SCHEMA_VERSION}까지)")]
    UnsupportedVersion { found: u32 },
    #[error("페이지 {page_id}: {reason}")]
    InvalidPage { page_id: Uuid, reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DocumentKind {
    /// 박람회 팜플렛. Mac이 OCR 뒤 AI 정리와 노션 CSV까지 만든다
    Pamphlet,
    /// 펼친 책. 두 페이지 분리와 곡면 보정 대상
    Book,
    /// 그 밖의 낱장 문서
    Document,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageLayout {
    /// 사진 한 장 = 페이지 한 장
    Single,
    /// 펼친 두 페이지. Mac이 가운데 접힌 선에서 나눈다
    Spread,
}

/// 원본 사진 기준 정규화 좌표(0~1). 원점은 왼쪽 위, 사진의 EXIF 회전을 적용한 뒤의 방향이다
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// 문서 테두리 네 꼭짓점. iPhone 감지값이 처음 들어가고, Mac에서 손으로 고치면 덮인다
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quad {
    pub top_left: Point,
    pub top_right: Point,
    pub bottom_right: Point,
    pub bottom_left: Point,
}

impl Quad {
    pub fn points(&self) -> [Point; 4] {
        [self.top_left, self.top_right, self.bottom_right, self.bottom_left]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: Uuid,
    /// 문서 폴더 기준 상대 경로. 예: `raw/0b4f….heic`
    pub raw: String,
    pub captured_at: DateTime<Utc>,
    pub layout: PageLayout,
    /// 감지 실패면 없음. Mac이 원본 해상도에서 다시 감지한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quad: Option<Quad>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: u32,
    pub id: Uuid,
    /// 예: `2026 데이터센터코리아_한빛로보틱스`. 팜플렛은 Mac AI가 표지에서 회사명을 추천한다
    pub name: String,
    pub kind: DocumentKind,
    /// 박람회 이름처럼 여러 문서를 묶는 촬영 세션. iPhone에서 한 번 정하면 이어지는 문서에 붙는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub created_at: DateTime<Utc>,
    pub pages: Vec<Page>,
}

impl Manifest {
    pub fn load(document_dir: &Path) -> Result<Manifest, ManifestError> {
        let path = document_dir.join(MANIFEST_FILE);
        let bytes = fs::read(&path).map_err(|source| ManifestError::Io {
            path: path.clone(),
            source,
        })?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|source| ManifestError::Parse {
            path: path.clone(),
            source,
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// 임시 파일에 쓴 뒤 이름을 바꾼다. 전송이나 iCloud 동기화 중에 반쯤 쓴 manifest를 읽는 일을 막는다
    /// 전송이나 iCloud 동기화 중에 반쯤 쓴 manifest를 읽지 않도록 원자적으로 쓴다
    pub fn save(&self, document_dir: &Path) -> Result<(), ManifestError> {
        self.validate()?;
        fsio::write_json(&document_dir.join(MANIFEST_FILE), self)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version > SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedVersion {
                found: self.schema_version,
            });
        }
        for page in &self.pages {
            validate_page(page)?;
        }
        Ok(())
    }
}

fn validate_page(page: &Page) -> Result<(), ManifestError> {
    let invalid = |reason: &str| ManifestError::InvalidPage {
        page_id: page.id,
        reason: reason.to_string(),
    };
    // 다른 기기가 보낸 파일이라 경로를 믿지 않는다. 문서 폴더 밖을 가리키면 거부한다
    let raw = Path::new(&page.raw);
    let escapes = raw
        .components()
        .any(|component| !matches!(component, Component::Normal(_)));
    if page.raw.is_empty() || escapes {
        return Err(invalid("raw 경로는 문서 폴더 안의 상대 경로여야 한다"));
    }
    if let Some(quad) = page.quad {
        let in_range = quad
            .points()
            .iter()
            .all(|p| (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y));
        if !in_range {
            return Err(invalid("꼭짓점은 0~1 정규화 좌표여야 한다"));
        }
    }
    Ok(())
}

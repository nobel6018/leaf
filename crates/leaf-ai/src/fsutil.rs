use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::{AiError, io_error};

/// 임시 파일에 쓴 뒤 이름을 바꾼다. iCloud 동기화나 앱 종료 중에 반쯤 쓴 파일이 남지 않게 한다
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), AiError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(io_error(dir))?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = dir.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temp, bytes).map_err(io_error(&temp))?;
    fs::rename(&temp, path).map_err(|source| {
        let _ = fs::remove_file(&temp);
        AiError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// processing.json은 다른 기기와 동기화되는 파일이라 경로를 믿지 않는다. 문서 폴더 밖이면 거부한다
pub(crate) fn safe_join(document_dir: &Path, relative: &str) -> Result<PathBuf, AiError> {
    let rel = Path::new(relative);
    let escapes = rel.components().any(|c| !matches!(c, Component::Normal(_)));
    if relative.is_empty() || escapes {
        return Err(AiError::UnsafePath(relative.to_string()));
    }
    Ok(document_dir.join(rel))
}

/// macOS, Windows 모두에서 쓸 수 있는 파일 이름. 빈 이름이면 대체 이름을 쓴다
pub(crate) fn sanitize_file_name(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 폴더_밖_경로는_거부한다() {
        let dir = Path::new("/doc");
        assert!(safe_join(dir, "../x.json").is_err());
        assert!(safe_join(dir, "/etc/passwd").is_err());
        assert!(safe_join(dir, "").is_err());
        assert_eq!(
            safe_join(dir, "ocr/a.json").unwrap(),
            Path::new("/doc/ocr/a.json")
        );
    }

    #[test]
    fn 파일_이름에_못_쓰는_글자를_바꾼다() {
        assert_eq!(sanitize_file_name("A/B:C", "x"), "A_B_C");
        assert_eq!(sanitize_file_name(" .. ", "x"), "x");
    }
}

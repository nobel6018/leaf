//! 문서 폴더 파일 읽기와 원자적 쓰기. Mac 모듈(받기, 처리, AI)이 같은 규칙으로 읽고 쓴다.
//!
//! @see docs/document-format.md

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::ManifestError;

/// 임시 파일에 쓴 뒤 이름을 바꾼다. 전송, iCloud 동기화, 앱 종료 중에 반쯤 쓴 파일을 읽는 일을 막는다.
/// 임시 파일 이름에 무작위 값을 넣어, 같은 파일을 두 작업이 동시에 써도 서로의 임시 파일을 덮지 않는다
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

/// 없으면 None. 있는데 못 읽거나 형식이 틀리면 오류
pub fn read_json_optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, ManifestError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(io_error(path, source)),
    };
    serde_json::from_slice(&bytes).map(Some).map_err(|source| ManifestError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), ManifestError> {
    let json = serde_json::to_vec_pretty(value).expect("문서 파일 직렬화는 실패하지 않는다");
    write_atomic(path, &json).map_err(|source| io_error(path, source))
}

fn io_error(path: &Path, source: io::Error) -> ManifestError {
    ManifestError::Io {
        path: PathBuf::from(path),
        source,
    }
}

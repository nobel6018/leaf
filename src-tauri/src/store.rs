//! 보관함 위치와 문서 가져오기. 전송(Wi-Fi)과 iCloud inbox가 같은 가져오기 경로를 쓴다.
//!
//! 앱 데이터 폴더 아래에 `library/<문서 id>/`(보관함)과 `incoming/<문서 id>/`(받는 중)를 둔다.
//! 두 폴더가 같은 볼륨에 있어 받은 원본은 복사 없이 이름 바꾸기로 옮긴다.
//!
//! @see docs/transfer.md
//! @see docs/document-format.md

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use leaf_core::{MANIFEST_FILE, Manifest, ManifestError};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// 원본은 이 폴더 아래로만 받는다. 다른 파일(edits.json 등)은 Mac만 쓰므로 밖에서 들어오면 안 된다
pub const RAW_DIR: &str = "raw";
const PARTS_DIR: &str = ".parts";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("원본 파일이 아직 오지 않았다: {0:?}")]
    MissingFiles(Vec<String>),
    #[error("manifest 문제: {0}")]
    Manifest(#[from] ManifestError),
    #[error("raw 경로는 raw/ 아래여야 한다: {0}")]
    InvalidRawPath(String),
    #[error("보관함 입출력 실패 ({path}): {source}")]
    Io { path: PathBuf, source: io::Error },
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// 받은 원본을 보관함에 넣는 방식
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    /// 이름 바꾸기로 옮기고 받는 중 폴더를 비운다. Wi-Fi 전송의 `incoming/`용
    Move,
    /// 복사만 한다. 원본 폴더 정리는 부른 쪽이 한다. iCloud inbox용(가져오기가 실패하면 inbox가 그대로 남게)
    Copy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    pub document_id: Uuid,
    /// 이번에 새로 들어간 원본 상대 경로
    pub added_raw: Vec<String>,
    /// 보관함에 없던 새 문서였는지
    pub created: bool,
}

pub struct Store {
    library: PathBuf,
    incoming: PathBuf,
    /// 업로드 마무리와 가져오기가 같은 문서 폴더를 동시에 만지지 않게 한다. 둘 다 이름 바꾸기 몇 번이라 짧다
    lock: Mutex<()>,
}

impl Store {
    pub fn new(app_data_dir: &Path) -> Store {
        Store {
            library: app_data_dir.join("library"),
            incoming: app_data_dir.join("incoming"),
            lock: Mutex::new(()),
        }
    }

    pub fn library_dir(&self) -> &Path {
        &self.library
    }

    pub fn document_dir(&self, id: Uuid) -> PathBuf {
        self.library.join(id.to_string())
    }

    pub fn staging_dir(&self, id: Uuid) -> PathBuf {
        self.incoming.join(id.to_string())
    }

    /// 업로드 중인 파일을 둘 임시 경로. 해시를 확인하기 전에는 `raw/`에 보이지 않게 따로 둔다
    pub fn new_part_path(&self, id: Uuid) -> Result<PathBuf, StoreError> {
        let dir = self.staging_dir(id).join(PARTS_DIR);
        fs::create_dir_all(&dir).map_err(io_err(&dir))?;
        Ok(dir.join(format!("{}.part", Uuid::new_v4())))
    }

    /// 지난 실행에서 끊긴 업로드의 임시 파일을 지운다. 받다 만 파일은 이어 쓰지 않고 처음부터 다시 받는다
    pub fn clean_stale_parts(&self) {
        let Ok(entries) = fs::read_dir(&self.incoming) else {
            return;
        };
        for entry in entries.flatten() {
            let parts = entry.path().join(PARTS_DIR);
            if parts.is_dir() {
                let _ = fs::remove_dir_all(&parts);
            }
        }
    }

    /// 해시를 확인한 업로드 파일을 받는 중 폴더의 제자리에 둔다
    pub fn finalize_upload(&self, id: Uuid, rel: &str, part: &Path) -> Result<(), StoreError> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let target = self.staging_dir(id).join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        fs::rename(part, &target).map_err(io_err(&target))
    }

    /// 이미 받은 원본과 SHA-256. 받는 중 폴더와 보관함 둘 다 본다(보관함에 있는 원본은 다시 보낼 필요가 없다)
    pub fn received_files(&self, id: Uuid) -> Result<BTreeMap<String, String>, StoreError> {
        let mut files = BTreeMap::new();
        for root in [self.document_dir(id), self.staging_dir(id)] {
            for rel in list_raw_files(&root)? {
                if files.contains_key(&rel) {
                    continue;
                }
                let path = root.join(&rel);
                let hash = sha256_file(&path).map_err(io_err(&path))?;
                files.insert(rel, hash);
            }
        }
        Ok(files)
    }

    /// manifest의 원본이 모두 있으면 보관함으로 가져온다. 이미 있는 문서면 새 원본만 더하고 manifest를 바꾼다.
    /// edits.json, processing.json 같은 Mac 쪽 파일은 건드리지 않는다.
    ///
    /// 새 문서는 앱 데이터 안 임시 폴더에 다 만든 뒤 한 번에 이름을 바꿔 보관함에 넣는다.
    /// 보관함 목록이 반쯤 만들어진 문서를 보는 일이 없다.
    pub fn import(&self, manifest: &Manifest, source: &Path, mode: ImportMode) -> Result<ImportOutcome, StoreError> {
        manifest.validate()?;
        for page in &manifest.pages {
            if !is_raw_path(&page.raw) {
                return Err(StoreError::InvalidRawPath(page.raw.clone()));
            }
        }

        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let document = self.document_dir(manifest.id);
        let exists = document.join(MANIFEST_FILE).is_file();

        let mut to_add = Vec::new();
        let mut missing = Vec::new();
        for page in &manifest.pages {
            let in_library = exists && document.join(&page.raw).is_file();
            if in_library {
                continue;
            }
            if source.join(&page.raw).is_file() {
                if !to_add.contains(&page.raw) {
                    to_add.push(page.raw.clone());
                }
            } else {
                missing.push(page.raw.clone());
            }
        }
        if !missing.is_empty() {
            return Err(StoreError::MissingFiles(missing));
        }

        if exists {
            for rel in &to_add {
                transfer_file(&source.join(rel), &document.join(rel), mode)?;
            }
            // manifest를 마지막에 바꾼다. 중간에 멈추면 manifest가 모르는 원본만 남고, 다시 보내면 마저 끝난다
            manifest.save(&document)?;
        } else {
            fs::create_dir_all(&self.incoming).map_err(io_err(&self.incoming))?;
            fs::create_dir_all(&self.library).map_err(io_err(&self.library))?;
            let build = self.incoming.join(format!(".import-{}-{}", manifest.id, Uuid::new_v4()));
            let result = (|| {
                fs::create_dir_all(build.join(RAW_DIR)).map_err(io_err(&build))?;
                for rel in &to_add {
                    transfer_file(&source.join(rel), &build.join(rel), mode)?;
                }
                manifest.save(&build)?;
                fs::rename(&build, &document).map_err(io_err(&document))
            })();
            if let Err(error) = result {
                // Move 방식이면 이미 옮긴 원본을 받는 중 폴더로 되돌려야 다시 보내기 없이 재시도할 수 있다
                if mode == ImportMode::Move {
                    for rel in &to_add {
                        let moved = build.join(rel);
                        if moved.is_file() {
                            let _ = fs::rename(&moved, source.join(rel));
                        }
                    }
                }
                let _ = fs::remove_dir_all(&build);
                return Err(error);
            }
        }

        if mode == ImportMode::Move {
            let _ = fs::remove_dir_all(source.join(RAW_DIR));
            // 다른 업로드가 진행 중이면 .parts가 남아 있어 폴더는 지워지지 않는다. 그대로 둬도 된다
            let _ = fs::remove_dir(source.join(PARTS_DIR));
            let _ = fs::remove_dir(source);
        }

        Ok(ImportOutcome {
            document_id: manifest.id,
            added_raw: to_add,
            created: !exists,
        })
    }
}

/// `raw/` 아래의 상대 경로인지. 다른 기기가 보낸 값이라 `..`, 절대 경로, 숨김 파일, 역슬래시를 모두 거부한다
pub fn is_raw_path(rel: &str) -> bool {
    if rel.contains('\\') || rel.contains(':') || rel.contains('\0') {
        return false;
    }
    // Path::components는 `a/./b`, `a//b`를 조용히 정규화하므로 문자열 그대로 나눠 본다
    let mut segments = rel.split('/');
    if segments.next() != Some(RAW_DIR) {
        return false;
    }
    let rest: Vec<&str> = segments.collect();
    !rest.is_empty() && rest.iter().all(|s| !s.is_empty() && !s.starts_with('.'))
}

/// 문서 폴더(또는 받는 중 폴더)의 `raw/` 아래 파일을 `raw/...` 상대 경로로 돌려준다. 숨김 파일은 뺀다
pub fn list_raw_files(root: &Path) -> Result<Vec<String>, StoreError> {
    let mut found = Vec::new();
    let mut stack = vec![(root.join(RAW_DIR), RAW_DIR.to_string())];
    while let Some((dir, prefix)) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(io_err(&dir)(error)),
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let rel = format!("{prefix}/{name}");
            let path = entry.path();
            if path.is_dir() {
                stack.push((path, rel));
            } else if path.is_file() {
                found.push(rel);
            }
        }
    }
    found.sort();
    Ok(found)
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 대상과 같은 폴더의 임시 이름에 둔 뒤 이름을 바꾼다. 보관함 안에서 반쯤 쓴 원본이 보이지 않게 한다
fn transfer_file(from: &Path, to: &Path, mode: ImportMode) -> Result<(), StoreError> {
    let parent = to.parent().expect("문서 폴더 안의 경로라 부모가 있다");
    fs::create_dir_all(parent).map_err(io_err(parent))?;
    if mode == ImportMode::Move && fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // 다른 볼륨이면 이름 바꾸기가 안 되므로 복사한다
    let temp = parent.join(format!(".{}.importing", Uuid::new_v4()));
    let result = fs::copy(from, &temp)
        .and_then(|_| fs::rename(&temp, to))
        .map_err(io_err(to));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    if mode == ImportMode::Move {
        let _ = fs::remove_file(from);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_raw_path;

    #[test]
    fn raw_아래_상대_경로만_받는다() {
        assert!(is_raw_path("raw/a.heic"));
        assert!(is_raw_path("raw/sub/a.heic"));
        for bad in [
            "raw",
            "raw/",
            "edits.json",
            "manifest.json",
            "raw/../edits.json",
            "raw/./a.heic",
            "/raw/a.heic",
            "raw/.a.heic.part",
            "raw\\a.heic",
            "raw/a:b",
            "raw//a.heic",
            "../raw/a.heic",
        ] {
            assert!(!is_raw_path(bad), "{bad}");
        }
    }
}

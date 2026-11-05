//! iCloud 폴더 받기(보조 경로). iPhone이 `iCloud Drive/leaf/inbox/<문서 id>/`에 원본을 먼저, manifest를
//! 마지막에 복사하면, Mac은 manifest가 있고 원본이 모두 받아진 문서를 보관함으로 가져온 뒤 inbox에서 지운다.
//!
//! 폴더 감시는 5초 간격 훑기로 한다. iCloud 동기화 자체가 수 초 걸리고, 파일 이벤트는 iCloud가 자리표시를
//! 실제 파일로 바꾸는 순간을 놓치기 쉬워 훑기가 더 단순하고 확실하다.
//!
//! 훑기와 가져오기(`scan_inbox`)는 플랫폼과 무관하게 두어 임시 폴더로 시험한다. iCloud 경로와 내려받기
//! 요청만 macOS 전용이다.
//!
//! @see docs/transfer.md

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use leaf_core::{MANIFEST_FILE, Manifest};
use serde::Serialize;
use uuid::Uuid;

use crate::store::{ImportMode, Store, StoreError, is_raw_path};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct InboxScan {
    pub imported: Vec<Uuid>,
    /// 아직 가져오지 못한 문서 폴더 수(동기화 중, 내려받는 중, 오류)
    pub pending: u32,
    /// 가져오기에 실패한 문서와 이유. 감시 고리가 같은 오류를 5초마다 되풀이해 남기지 않도록 부른 쪽이 거른다
    pub failed: Vec<(Uuid, String)>,
}

/// 파일이 실제로 받아져 있는지. iCloud는 아직 받지 않은 파일을 두 가지로 보인다:
/// 옛 방식은 `.이름.icloud` 자리표시 파일, macOS 14 이후 FileProvider 방식은 크기만 있고 내용이 없는(dataless) 파일
#[derive(Debug, PartialEq, Eq)]
enum FileState {
    Ready,
    /// 자리표시만 있다. 내려받기를 요청해야 한다
    NotDownloaded,
    /// 흔적이 없다. 아직 iCloud가 목록을 동기화하지 않았다
    Absent,
}

fn file_state(path: &Path) -> FileState {
    if let Ok(metadata) = fs::metadata(path)
        && metadata.is_file()
    {
        return if is_dataless(&metadata) {
            FileState::NotDownloaded
        } else {
            FileState::Ready
        };
    }
    if placeholder_path(path).is_some_and(|p| p.exists()) {
        FileState::NotDownloaded
    } else {
        FileState::Absent
    }
}

fn placeholder_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy();
    Some(path.with_file_name(format!(".{name}.icloud")))
}

#[cfg(target_os = "macos")]
fn is_dataless(metadata: &fs::Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    // <sys/stat.h> SF_DATALESS: 내용이 아직 내려오지 않은 FileProvider 파일
    const SF_DATALESS: u32 = 0x4000_0000;
    metadata.st_flags() & SF_DATALESS != 0
}

#[cfg(not(target_os = "macos"))]
fn is_dataless(_metadata: &fs::Metadata) -> bool {
    false
}

/// inbox를 한 번 훑는다. 준비된 문서는 보관함으로 복사한 뒤 inbox에서 지우고, 받지 않은 파일은 `request_download`로 요청한다
pub fn scan_inbox(inbox: &Path, store: &Store, request_download: &mut dyn FnMut(&Path)) -> InboxScan {
    let mut scan = InboxScan::default();
    let Ok(entries) = fs::read_dir(inbox) else {
        return scan;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !dir.is_dir() {
            continue;
        }
        let Ok(id) = Uuid::parse_str(&name) else {
            continue;
        };
        match import_one(&dir, id, store, request_download) {
            Ok(true) => scan.imported.push(id),
            Ok(false) => scan.pending += 1,
            Err(error) => {
                scan.failed.push((id, error));
                scan.pending += 1;
            }
        }
    }
    scan
}

/// 가져왔으면 true, 아직 준비가 안 됐으면 false
fn import_one(dir: &Path, id: Uuid, store: &Store, request_download: &mut dyn FnMut(&Path)) -> Result<bool, String> {
    let manifest_path = dir.join(MANIFEST_FILE);
    match file_state(&manifest_path) {
        FileState::Ready => {}
        FileState::NotDownloaded => {
            request_download(&manifest_path);
            return Ok(false);
        }
        FileState::Absent => return Ok(false),
    }
    let manifest = Manifest::load(dir).map_err(|e| e.to_string())?;
    if manifest.id != id {
        return Err(format!("폴더 이름과 manifest id({})가 다르다", manifest.id));
    }

    let mut ready = true;
    for page in &manifest.pages {
        if !is_raw_path(&page.raw) {
            return Err(format!("raw 경로는 raw/ 아래여야 한다: {}", page.raw));
        }
        let path = dir.join(&page.raw);
        match file_state(&path) {
            FileState::Ready => {}
            FileState::NotDownloaded => {
                request_download(&path);
                ready = false;
            }
            FileState::Absent => {
                // 보관함에 이미 있는 원본이면 iPhone이 다시 올리지 않았을 수 있다. 가져오기가 판단한다
                if !store.document_dir(id).join(&page.raw).is_file() {
                    ready = false;
                }
            }
        }
    }
    if !ready {
        return Ok(false);
    }

    match store.import(&manifest, dir, ImportMode::Copy) {
        Ok(outcome) => {
            eprintln!("문서 받음 (iCloud): {id} (새 원본 {}개)", outcome.added_raw.len());
            if let Err(error) = fs::remove_dir_all(dir) {
                // 보관함에는 들어갔다. 다음 훑기에서 다시 가져와도 같은 결과라 문제없다
                eprintln!("iCloud inbox에서 지우지 못했다 ({id}): {error}");
            }
            Ok(true)
        }
        Err(StoreError::MissingFiles(_)) => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxStatus {
    pub path: String,
    pub available: bool,
    pub pending: u32,
}

pub struct InboxState {
    path: Option<PathBuf>,
    pending: Arc<AtomicU32>,
}

#[tauri::command]
pub fn icloud_inbox_status(state: tauri::State<'_, InboxState>) -> InboxStatus {
    InboxStatus {
        path: state
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        available: state.path.as_ref().is_some_and(|p| p.is_dir()),
        pending: state.pending.load(Ordering::SeqCst),
    }
}

/// `~/Library/Mobile Documents/com~apple~CloudDocs/leaf/inbox`. iCloud Drive가 꺼져 있으면 None
#[cfg(target_os = "macos")]
pub fn icloud_inbox_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let drive = PathBuf::from(home).join("Library/Mobile Documents/com~apple~CloudDocs");
    drive.is_dir().then(|| drive.join("leaf").join("inbox"))
}

#[cfg(not(target_os = "macos"))]
pub fn icloud_inbox_dir() -> Option<PathBuf> {
    None
}

/// 같은 파일에 내려받기를 너무 자주 요청하지 않는다. 큰 사진은 받는 데 수십 초가 걸린다
#[cfg(target_os = "macos")]
fn download_requester() -> impl FnMut(&Path) {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    let mut requested: HashMap<PathBuf, Instant> = HashMap::new();
    move |path: &Path| {
        let now = Instant::now();
        requested.retain(|_, at| now.duration_since(*at) < Duration::from_secs(60));
        if requested.contains_key(path) {
            return;
        }
        requested.insert(path.to_path_buf(), now);
        // brctl은 요청만 넣고 바로 끝난다. 실제 내려받기는 iCloud 데몬이 한다
        match std::process::Command::new("brctl").arg("download").arg(path).spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
            }
            Err(error) => eprintln!("brctl download 실패: {error}"),
        }
    }
}

/// iCloud inbox 감시를 시작한다. iCloud Drive가 없거나 macOS가 아니면 상태만 등록한다
pub fn setup(app: &tauri::AppHandle, store: Arc<Store>) {
    use tauri::Manager;

    let path = icloud_inbox_dir();
    let pending = Arc::new(AtomicU32::new(0));
    app.manage(InboxState {
        path: path.clone(),
        pending: pending.clone(),
    });

    #[cfg(target_os = "macos")]
    if let Some(inbox) = path {
        // iPhone 파일 앱에서 고를 폴더가 보이도록 미리 만든다
        if let Err(error) = fs::create_dir_all(&inbox) {
            eprintln!("iCloud inbox 폴더를 만들지 못했다: {error}");
        }
        use std::collections::HashSet;

        let app = app.clone();
        std::thread::spawn(move || {
            let mut request = download_requester();
            let mut reported: HashSet<(Uuid, String)> = HashSet::new();
            loop {
                let scan = scan_inbox(&inbox, &store, &mut request);
                pending.store(scan.pending, Ordering::SeqCst);
                for id in scan.imported {
                    crate::transfer::emit_library_changed(&app, id);
                }
                for failure in scan.failed {
                    if !reported.contains(&failure) {
                        eprintln!("iCloud inbox 문서를 가져오지 못했다 ({}): {}", failure.0, failure.1);
                        reported.insert(failure);
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (store, pending);
}

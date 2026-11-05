//! 앱을 다시 켜면 끊기는 작업의 등록부. 업데이트는 이 목록이 비어 있을 때만 설치한다.
//!
//! 작업은 시작할 때 `busy::start("처리: 문서 이름")`으로 등록하고, 돌려받은 BusyGuard가 사라지면 빠진다.
//! 오류나 panic으로 끝나도 guard가 drop되므로 목록에 남지 않는다.
//!
//! @see docs/update.md

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

static TASKS: Mutex<BTreeMap<u64, String>> = Mutex::new(BTreeMap::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[must_use = "guard가 바로 사라지면 작업이 등록되지 않은 것과 같다"]
pub struct BusyGuard {
    id: u64,
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        TASKS.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.id);
    }
}

pub fn start(label: impl Into<String>) -> BusyGuard {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    TASKS.lock().unwrap_or_else(|e| e.into_inner()).insert(id, label.into());
    BusyGuard { id }
}

/// 시작한 순서대로
pub fn current() -> Vec<String> {
    TASKS.lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect()
}

/// 화면이 업데이트 버튼을 누를 때와 설치 직전에 두 번 묻는다
#[tauri::command]
pub fn busy_tasks() -> Vec<String> {
    current()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard가_사라지면_목록에서_빠진다() {
        let label = "처리: 테스트 문서 guard";
        {
            let _guard = start(label);
            assert!(current().iter().any(|t| t == label));
        }
        assert!(!current().iter().any(|t| t == label));
    }

    #[test]
    fn panic으로_끝나도_목록에_남지_않는다() {
        let label = "처리: 테스트 문서 panic";
        let result = std::panic::catch_unwind(|| {
            let _guard = start(label);
            panic!("처리 실패");
        });
        assert!(result.is_err());
        assert!(!current().iter().any(|t| t == label));
    }
}

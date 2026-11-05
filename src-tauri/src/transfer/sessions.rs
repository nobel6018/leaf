//! 받는 중인 문서를 busy 등록부에 올린다. 앱 안 업데이트가 전송 도중에 앱을 다시 켜지 않게 한다.
//!
//! 요청 단위가 아니라 문서 단위로 잡는다. 원본 사이사이(iPhone이 다음 파일을 준비하는 몇 초)에 업데이트가
//! 끼어들면 manifest가 오기 전에 끊기기 때문이다. 첫 원본 PUT에 열고, manifest 가져오기가 끝나면 닫는다.
//! iPhone이 Wi-Fi를 벗어나 그대로 멈추면 요청이 없는 채로 `IDLE_TIMEOUT`이 지난 뒤 닫는다
//! (받다 만 원본은 `incoming/`에 남아 다음에 이어 보낸다).
//!
//! @see docs/transfer.md
//! @see docs/update.md

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::busy::{self, BusyGuard};

pub const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

struct Session {
    _busy: BusyGuard,
    /// 진행 중인 요청 수. 큰 원본 하나를 받는 동안은 시간이 지나도 닫지 않는다
    active: usize,
    last_activity: Instant,
}

#[derive(Default)]
pub struct UploadSessions {
    sessions: Mutex<HashMap<Uuid, Session>>,
}

/// 요청 하나가 끝나면(성공, 실패, 끊김 모두) 진행 중 요청 수를 내린다
pub struct RequestGuard<'a> {
    sessions: &'a UploadSessions,
    document_id: Uuid,
}

impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        let mut sessions = self.sessions.lock();
        if let Some(session) = sessions.get_mut(&self.document_id) {
            session.active = session.active.saturating_sub(1);
            session.last_activity = Instant::now();
        }
    }
}

impl UploadSessions {
    /// 원본 요청을 시작한다. 문서가 처음이면 busy 등록부에 올린다
    pub fn begin(&self, document_id: Uuid, label: impl FnOnce() -> String) -> RequestGuard<'_> {
        let mut sessions = self.lock();
        let session = sessions.entry(document_id).or_insert_with(|| Session {
            _busy: busy::start(label()),
            active: 0,
            last_activity: Instant::now(),
        });
        session.active += 1;
        session.last_activity = Instant::now();
        RequestGuard {
            sessions: self,
            document_id,
        }
    }

    /// manifest 가져오기가 끝나면 닫는다
    pub fn finish(&self, document_id: Uuid) {
        self.lock().remove(&document_id);
    }

    /// 요청 없이 오래 멈춘 문서를 닫는다. 서버가 주기적으로 부른다
    pub fn sweep(&self, now: Instant) {
        self.lock()
            .retain(|_, s| s.active > 0 || now.duration_since(s.last_activity) < IDLE_TIMEOUT);
    }

    pub fn is_open(&self, document_id: Uuid) -> bool {
        self.lock().contains_key(&document_id)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Session>> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registered(label: &str) -> bool {
        busy::current().iter().any(|t| t == label)
    }

    #[test]
    fn 요청_사이에도_열려_있고_manifest_뒤에_닫힌다() {
        let sessions = UploadSessions::default();
        let id = Uuid::new_v4();
        let label = format!("세션 테스트 {id}");
        drop(sessions.begin(id, || label.clone()));
        assert!(registered(&label));
        drop(sessions.begin(id, || unreachable!("이미 열린 문서")));
        sessions.finish(id);
        assert!(!registered(&label));
    }

    #[test]
    fn 멈춘_문서는_시간이_지나면_닫고_받는_중이면_두다() {
        let sessions = UploadSessions::default();
        let (idle, busy_doc) = (Uuid::new_v4(), Uuid::new_v4());
        drop(sessions.begin(idle, || format!("멈춤 {idle}")));
        let _in_flight = sessions.begin(busy_doc, || format!("받는 중 {busy_doc}"));
        let later = Instant::now() + IDLE_TIMEOUT + Duration::from_secs(1);
        sessions.sweep(later);
        assert!(!sessions.is_open(idle));
        assert!(sessions.is_open(busy_doc));
    }
}

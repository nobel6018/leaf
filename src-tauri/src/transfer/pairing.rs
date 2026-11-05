//! 짝짓기 창. Mac에서 "iPhone 연결"을 누르면 6자리 코드가 2분 동안 열리고, 5번 틀리면 닫힌다.
//!
//! 코드가 10^6가지뿐이라 시도 횟수 제한이 보안의 전부다. 창은 한 번에 하나만 열린다.
//!
//! @see docs/transfer.md

use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

pub const PAIRING_WINDOW_SECONDS: i64 = 120;
pub const MAX_PAIRING_FAILURES: u32 = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingCode {
    pub code: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairError {
    /// 창이 열려 있지 않거나, 시간이 지났거나, 너무 많이 틀려 닫혔다
    Closed,
    WrongCode,
}

struct Window {
    code: String,
    expires_at: DateTime<Utc>,
    failures: u32,
}

#[derive(Default)]
pub struct Pairing {
    window: Mutex<Option<Window>>,
}

impl Pairing {
    /// 새 코드로 창을 연다. 열려 있던 창은 버린다
    pub fn start(&self, now: DateTime<Utc>) -> PairingCode {
        let code = random_code();
        let expires_at = now + Duration::seconds(PAIRING_WINDOW_SECONDS);
        *self.lock() = Some(Window {
            code: code.clone(),
            expires_at,
            failures: 0,
        });
        PairingCode { code, expires_at }
    }

    pub fn cancel(&self) {
        *self.lock() = None;
    }

    pub fn is_open(&self, now: DateTime<Utc>) -> bool {
        self.lock().as_ref().is_some_and(|w| now < w.expires_at)
    }

    /// 맞으면 창을 닫고 Ok. 틀리면 실패 횟수를 세고, 다섯 번째에 창을 닫는다
    pub fn attempt(&self, code: &str, now: DateTime<Utc>) -> Result<(), PairError> {
        let mut guard = self.lock();
        let Some(window) = guard.as_mut() else {
            return Err(PairError::Closed);
        };
        if now >= window.expires_at {
            *guard = None;
            return Err(PairError::Closed);
        }
        if constant_time_eq(window.code.as_bytes(), code.as_bytes()) {
            *guard = None;
            return Ok(());
        }
        window.failures += 1;
        if window.failures >= MAX_PAIRING_FAILURES {
            *guard = None;
        }
        Err(PairError::WrongCode)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Window>> {
        self.window.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 짝짓기 토큰. 32바이트 무작위 값
pub fn random_token() -> [u8; 32] {
    let mut token = [0u8; 32];
    getrandom::fill(&mut token).expect("OS 난수를 얻지 못했다");
    token
}

fn random_code() -> String {
    // 2^32를 10^6으로 나눈 나머지의 치우침은 0.03% 미만이라 무시한다
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).expect("OS 난수를 얻지 못했다");
    format!("{:06}", u32::from_le_bytes(bytes) % 1_000_000)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 코드는_6자리_숫자다() {
        let pairing = Pairing::default();
        let code = pairing.start(Utc::now()).code;
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn 다섯_번_틀리면_창이_닫힌다() {
        let pairing = Pairing::default();
        let now = Utc::now();
        let code = pairing.start(now).code;
        let wrong = if code == "000000" { "111111" } else { "000000" };
        for _ in 0..MAX_PAIRING_FAILURES {
            assert_eq!(pairing.attempt(wrong, now), Err(PairError::WrongCode));
        }
        assert_eq!(pairing.attempt(&code, now), Err(PairError::Closed));
    }

    #[test]
    fn 시간이_지나면_맞는_코드도_거부한다() {
        let pairing = Pairing::default();
        let now = Utc::now();
        let code = pairing.start(now).code;
        let later = now + Duration::seconds(PAIRING_WINDOW_SECONDS);
        assert_eq!(pairing.attempt(&code, later), Err(PairError::Closed));
    }

    #[test]
    fn 맞는_코드는_한_번만_쓴다() {
        let pairing = Pairing::default();
        let now = Utc::now();
        let code = pairing.start(now).code;
        assert_eq!(pairing.attempt(&code, now), Ok(()));
        assert_eq!(pairing.attempt(&code, now), Err(PairError::Closed));
    }
}

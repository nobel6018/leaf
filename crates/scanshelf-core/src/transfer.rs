//! iPhone에서 Mac으로 보내는 HTTP 요청 서명. iPhone(CryptoKit)과 Mac이 같은 값을 만들어야 하므로
//! fixtures/transfer-signature.json 예시를 두 쪽 테스트가 같이 확인한다.
//!
//! @see docs/transfer.md

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

/// 요청 시각과 Mac 시각 차이 허용 범위. 넘으면 재전송 공격으로 보고 거부한다
pub const MAX_CLOCK_SKEW_SECONDS: i64 = 300;

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// 서명할 문자열: `METHOD\nPATH\nTIMESTAMP\nBODY_SHA256_HEX`
pub fn canonical_request(method: &str, path: &str, timestamp: i64, body_sha256_hex: &str) -> String {
    format!("{method}\n{path}\n{timestamp}\n{body_sha256_hex}")
}

pub fn sign(token: &[u8], method: &str, path: &str, timestamp: i64, body_sha256_hex: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(token).expect("HMAC은 모든 키 길이를 받는다");
    mac.update(canonical_request(method, path, timestamp, body_sha256_hex).as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// 상수 시간 비교로 서명을 확인한다
pub fn verify(token: &[u8], method: &str, path: &str, timestamp: i64, body_sha256_hex: &str, signature_hex: &str) -> bool {
    let Ok(signature) = hex::decode(signature_hex) else {
        return false;
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(token).expect("HMAC은 모든 키 길이를 받는다");
    mac.update(canonical_request(method, path, timestamp, body_sha256_hex).as_bytes());
    mac.verify_slice(&signature).is_ok()
}

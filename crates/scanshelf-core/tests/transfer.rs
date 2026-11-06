use std::fs;
use std::path::Path;

use scanshelf_core::transfer::{sha256_hex, sign, verify};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Vector {
    token_hex: String,
    method: String,
    path: String,
    timestamp: i64,
    body: String,
    body_sha256: String,
    signature: String,
}

fn vector() -> Vector {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/transfer-signature.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn 공용_예시와_같은_서명을_만든다() {
    let v = vector();
    let token = hex::decode(&v.token_hex).unwrap();

    assert_eq!(sha256_hex(v.body.as_bytes()), v.body_sha256);
    assert_eq!(sign(&token, &v.method, &v.path, v.timestamp, &v.body_sha256), v.signature);
}

#[test]
fn 경로가_다르면_서명이_맞지_않는다() {
    let v = vector();
    let token = hex::decode(&v.token_hex).unwrap();

    assert!(verify(&token, &v.method, &v.path, v.timestamp, &v.body_sha256, &v.signature));
    assert!(!verify(&token, &v.method, "/v1/other", v.timestamp, &v.body_sha256, &v.signature));
}

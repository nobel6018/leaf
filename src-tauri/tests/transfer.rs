//! Wi-Fi 전송 서버를 localhost에 띄우고, scanshelf_core::transfer로 서명한 요청을 보내 확인한다.
//! 원본은 합성 바이트, manifest는 fixtures/manifest-v1.json만 쓴다.
//!
//! @see docs/transfer.md

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use scanshelf_core::transfer::{sha256_hex, sign};
use scanshelf_core::{Manifest, Page, PageLayout};
use scanshelf_lib::store::Store;
use scanshelf_lib::transfer::devices::{Devices, MemoryTokenStore, PairedDevice};
use scanshelf_lib::transfer::pairing::Pairing;
use scanshelf_lib::transfer::server::{
    HEADER_BODY_SHA256, HEADER_DEVICE, HEADER_SIGNATURE, HEADER_TIMESTAMP, TransferEvents, TransferService, serve,
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Default)]
struct Recorder {
    library_changed: Mutex<Vec<Uuid>>,
    paired: Mutex<Vec<PairedDevice>>,
}

impl TransferEvents for Recorder {
    fn library_changed(&self, document_id: Uuid) {
        self.library_changed.lock().unwrap().push(document_id);
    }

    fn pairing_completed(&self, device: &PairedDevice) {
        self.paired.lock().unwrap().push(device.clone());
    }
}

struct Harness {
    base: String,
    service: Arc<TransferService>,
    events: Arc<Recorder>,
    data: tempfile::TempDir,
    client: reqwest::Client,
}

struct Device {
    id: String,
    token: Vec<u8>,
}

fn fixture_manifest() -> Manifest {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/manifest-v1.json");
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// 원본마다 다른 합성 바이트. 실제 스캔 사진은 쓰지 않는다
fn synthetic_raw(rel: &str) -> Vec<u8> {
    format!("synthetic raw bytes for {rel}").repeat(100).into_bytes()
}

async fn start() -> Harness {
    // 앱 안 업데이트 플러그인이 reqwest를 rustls 공급자 없이 켜 둬서, 테스트 클라이언트도 공급자를 직접 고른다
    let _ = rustls::crypto::ring::default_provider().install_default();
    let data = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(data.path()));
    let devices = Arc::new(Devices::load(
        &data.path().join("devices.json"),
        Arc::new(MemoryTokenStore::default()),
    ));
    let events = Arc::new(Recorder::default());
    let service = Arc::new(TransferService {
        store,
        devices,
        pairing: Arc::new(Pairing::default()),
        mac_name: "테스트 Mac".to_string(),
        events: events.clone(),
        uploads: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(serve(listener, service.clone()));
    Harness {
        base,
        service,
        events,
        data,
        client: reqwest::Client::new(),
    }
}

impl Harness {
    async fn pair(&self) -> Device {
        let code = self.service.pairing.start(Utc::now()).code;
        let id = Uuid::new_v4().to_string();
        let response = self
            .client
            .post(format!("{}/v1/pair", self.base))
            .json(&json!({ "code": code, "deviceId": id, "deviceName": "테스트 iPhone" }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["macName"], "테스트 Mac");
        let token = hex::decode(body["token"].as_str().unwrap()).unwrap();
        assert_eq!(token.len(), 32);
        Device { id, token }
    }

    /// iPhone이 하는 그대로 서명한다. 본문 해시 헤더를 따로 줄 수 있게 해 변조 시험에 쓴다
    async fn send(
        &self,
        device: &Device,
        method: Method,
        path: &str,
        body: Vec<u8>,
        declared_sha256: Option<String>,
        timestamp: i64,
    ) -> reqwest::Response {
        let body_sha256 = declared_sha256.unwrap_or_else(|| sha256_hex(&body));
        let signature = sign(&device.token, method.as_str(), path, timestamp, &body_sha256);
        self.client
            .request(method, format!("{}{}", self.base, path))
            .header(HEADER_DEVICE, &device.id)
            .header(HEADER_TIMESTAMP, timestamp.to_string())
            .header(HEADER_BODY_SHA256, body_sha256)
            .header(HEADER_SIGNATURE, signature)
            .body(body)
            .send()
            .await
            .unwrap()
    }

    async fn signed(&self, device: &Device, method: Method, path: &str, body: Vec<u8>) -> reqwest::Response {
        self.send(device, method, path, body, None, Utc::now().timestamp()).await
    }

    async fn upload(&self, device: &Device, id: Uuid, rel: &str) -> reqwest::Response {
        let path = format!("/v1/documents/{id}/files/{rel}");
        self.signed(device, Method::PUT, &path, synthetic_raw(rel)).await
    }

    async fn put_manifest(&self, device: &Device, manifest: &Manifest) -> reqwest::Response {
        let path = format!("/v1/documents/{}/manifest", manifest.id);
        self.signed(device, Method::PUT, &path, serde_json::to_vec(manifest).unwrap()).await
    }

    async fn status(&self, device: &Device, id: Uuid) -> Value {
        let path = format!("/v1/documents/{id}/status");
        let response = self.signed(device, Method::GET, &path, Vec::new()).await;
        assert_eq!(response.status(), StatusCode::OK);
        response.json().await.unwrap()
    }

    fn library_doc(&self, id: Uuid) -> std::path::PathBuf {
        self.data.path().join("library").join(id.to_string())
    }
}

/// 테스트들이 한 프로세스의 busy 등록부를 같이 쓰므로 문서 id로 가려 본다
fn busy_for(id: Uuid) -> bool {
    scanshelf_lib::busy::current().iter().any(|t| t.contains(&id.to_string()))
}

#[tokio::test]
async fn info는_인증_없이_맥_이름을_돌려준다() {
    let h = start().await;
    let body: Value = h
        .client
        .get(format!("{}/v1/info", h.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["name"], "테스트 Mac");
    assert!(body["version"].is_string());
}

#[tokio::test]
async fn 페어링에_성공하면_기기가_목록에_들어가고_코드는_다시_못_쓴다() {
    let h = start().await;
    let code = h.service.pairing.start(Utc::now()).code;
    let id = Uuid::new_v4().to_string();
    let request = json!({ "code": code, "deviceId": id, "deviceName": "테스트 iPhone" });

    let first = h.client.post(format!("{}/v1/pair", h.base)).json(&request).send().await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let again = h.client.post(format!("{}/v1/pair", h.base)).json(&request).send().await.unwrap();
    assert_eq!(again.status(), StatusCode::FORBIDDEN);

    let devices = h.service.devices.list();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].device_id, id);
    assert_eq!(h.events.paired.lock().unwrap()[0].device_name, "테스트 iPhone");
    // 목록 파일에는 토큰이 없다
    let saved = fs::read_to_string(h.data.path().join("devices.json")).unwrap();
    assert!(!saved.contains("token"));
}

#[tokio::test]
async fn 코드를_다섯_번_틀리면_맞는_코드도_거부한다() {
    let h = start().await;
    let code = h.service.pairing.start(Utc::now()).code;
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let id = Uuid::new_v4().to_string();
    for _ in 0..5 {
        let response = h
            .client
            .post(format!("{}/v1/pair", h.base))
            .json(&json!({ "code": wrong, "deviceId": id, "deviceName": "x" }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let response = h
        .client
        .post(format!("{}/v1/pair", h.base))
        .json(&json!({ "code": code, "deviceId": id, "deviceName": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(h.service.devices.list().is_empty());
}

#[tokio::test]
async fn 원본과_manifest를_보내면_보관함에_문서가_생긴다() {
    let h = start().await;
    let device = h.pair().await;
    // busy 등록부는 다른 테스트와 같이 쓰므로 이 테스트만의 문서 id를 쓴다
    let mut manifest = fixture_manifest();
    manifest.id = Uuid::new_v4();

    for page in &manifest.pages {
        let response = h.upload(&device, manifest.id, &page.raw).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    // 원본 사이에도 앱 안 업데이트가 끼어들지 않게 문서 단위로 busy에 올라 있다
    assert!(busy_for(manifest.id));
    let status = h.status(&device, manifest.id).await;
    for page in &manifest.pages {
        assert_eq!(status["files"][&page.raw], sha256_hex(&synthetic_raw(&page.raw)));
    }

    let response = h.put_manifest(&device, &manifest).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["created"], true);

    assert!(!busy_for(manifest.id));
    let doc = h.library_doc(manifest.id);
    assert_eq!(Manifest::load(&doc).unwrap(), manifest);
    for page in &manifest.pages {
        assert_eq!(fs::read(doc.join(&page.raw)).unwrap(), synthetic_raw(&page.raw));
    }
    assert!(!h.data.path().join("incoming").join(manifest.id.to_string()).exists());
    assert_eq!(*h.events.library_changed.lock().unwrap(), vec![manifest.id]);
}

#[tokio::test]
async fn 끊긴_뒤에는_status를_보고_남은_원본만_이어_보낸다() {
    let h = start().await;
    let device = h.pair().await;
    let manifest = fixture_manifest();
    let (first, second) = (&manifest.pages[0].raw, &manifest.pages[1].raw);

    assert_eq!(h.upload(&device, manifest.id, first).await.status(), StatusCode::OK);
    let status = h.status(&device, manifest.id).await;
    let received = status["files"].as_object().unwrap();
    assert_eq!(received.len(), 1);
    assert!(received.contains_key(first));

    // 원본이 덜 왔으면 manifest를 받지 않고 빠진 파일을 알려 준다
    let response = h.put_manifest(&device, &manifest).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["missing"], json!([second]));
    assert!(!h.library_doc(manifest.id).exists());

    assert_eq!(h.upload(&device, manifest.id, second).await.status(), StatusCode::OK);
    assert_eq!(h.put_manifest(&device, &manifest).await.status(), StatusCode::OK);
    assert!(h.library_doc(manifest.id).join(second).is_file());
}

#[tokio::test]
async fn manifest를_다시_보내면_새_원본만_더하고_맥_편집은_그대로_둔다() {
    let h = start().await;
    let device = h.pair().await;
    let mut manifest = fixture_manifest();
    for page in &manifest.pages {
        h.upload(&device, manifest.id, &page.raw).await;
    }
    assert_eq!(h.put_manifest(&device, &manifest).await.status(), StatusCode::OK);

    let doc = h.library_doc(manifest.id);
    let edits = r#"{"name":"Mac에서 고친 이름"}"#.as_bytes();
    fs::write(doc.join("edits.json"), edits).unwrap();
    fs::write(doc.join("processing.json"), b"{}").unwrap();

    // 이미 보관함에 있는 원본도 status에 나와 iPhone이 다시 보내지 않는다
    let status = h.status(&device, manifest.id).await;
    assert_eq!(status["files"].as_object().unwrap().len(), 2);

    let page_id = Uuid::new_v4();
    let extra = format!("raw/{page_id}.heic");
    manifest.pages.push(Page {
        extra_rotation: 0,
        id: page_id,
        raw: extra.clone(),
        captured_at: manifest.created_at + Duration::seconds(30),
        layout: PageLayout::Single,
        quad: None,
    });
    assert_eq!(h.upload(&device, manifest.id, &extra).await.status(), StatusCode::OK);
    let response = h.put_manifest(&device, &manifest).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["created"], false);
    assert_eq!(body["added"], json!([extra]));

    assert_eq!(Manifest::load(&doc).unwrap().pages.len(), 3);
    assert_eq!(fs::read(doc.join("edits.json")).unwrap(), edits);
    assert_eq!(fs::read(doc.join("processing.json")).unwrap(), b"{}");
    assert_eq!(h.events.library_changed.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn 서명이_틀리거나_시각이_멀거나_모르는_기기면_401() {
    let h = start().await;
    let device = h.pair().await;
    let id = Uuid::new_v4();
    let path = format!("/v1/documents/{id}/status");

    let forged = Device {
        id: device.id.clone(),
        token: vec![7u8; 32],
    };
    let response = h.signed(&forged, Method::GET, &path, Vec::new()).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let stale = (Utc::now() - Duration::seconds(301)).timestamp();
    let response = h.send(&device, Method::GET, &path, Vec::new(), None, stale).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let stranger = Device {
        id: Uuid::new_v4().to_string(),
        token: device.token.clone(),
    };
    let response = h.signed(&stranger, Method::GET, &path, Vec::new()).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = h.client.get(format!("{}{}", h.base, path)).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn 본문이_서명한_해시와_다르면_401이고_파일을_남기지_않는다() {
    let h = start().await;
    let device = h.pair().await;
    let id = Uuid::new_v4();
    let path = format!("/v1/documents/{id}/files/raw/a.heic");
    let claimed = sha256_hex(b"what the phone signed");

    let response = h
        .send(&device, Method::PUT, &path, b"tampered".to_vec(), Some(claimed), Utc::now().timestamp())
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(h.status(&device, id).await["files"], json!({}));
    let parts = h.data.path().join("incoming").join(id.to_string()).join(".parts");
    assert_eq!(fs::read_dir(parts).map(|d| d.count()).unwrap_or(0), 0);
}

#[tokio::test]
async fn 페어링을_해제한_기기는_더_보내지_못한다() {
    let h = start().await;
    let device = h.pair().await;
    h.service.devices.remove(&device.id).unwrap();
    let response = h.upload(&device, Uuid::new_v4(), "raw/a.heic").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn raw_밖으로_나가는_경로는_거부한다() {
    let h = start().await;
    let device = h.pair().await;
    let id = Uuid::new_v4();
    for rel in ["edits.json", "raw/..%2Fedits.json", "raw/%2E%2E%2F%2E%2E%2Fescape", "raw/.hidden", "raw%5C..%5Cx"] {
        let path = format!("/v1/documents/{id}/files/{rel}");
        let response = h.signed(&device, Method::PUT, &path, b"x".to_vec()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{rel}");
    }
    assert!(!h.data.path().join("incoming").join(id.to_string()).join("edits.json").exists());
    assert!(!h.data.path().join("incoming").join("escape").exists());
}

#[tokio::test]
async fn manifest의_raw가_raw_밖이거나_id가_다르면_400() {
    let h = start().await;
    let device = h.pair().await;

    let mut outside = fixture_manifest();
    outside.pages[0].raw = "edits.json".to_string();
    assert_eq!(h.put_manifest(&device, &outside).await.status(), StatusCode::BAD_REQUEST);

    let manifest = fixture_manifest();
    let other = Uuid::new_v4();
    let path = format!("/v1/documents/{other}/manifest");
    let response = h
        .signed(&device, Method::PUT, &path, serde_json::to_vec(&manifest).unwrap())
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn 모르는_형식_버전은_받지_않는다() {
    let h = start().await;
    let device = h.pair().await;
    let mut manifest = fixture_manifest();
    manifest.schema_version = scanshelf_core::SCHEMA_VERSION + 1;
    for page in &manifest.pages {
        h.upload(&device, manifest.id, &page.raw).await;
    }
    assert_eq!(h.put_manifest(&device, &manifest).await.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!h.library_doc(manifest.id).exists());
}

# iPhone에서 Mac으로 보내기

2026-11-05 결정: 같은 Wi-Fi면 직접 보내고, 떨어져 있으면 iCloud 폴더로 보냅니다.

코드: 서명 `crates/leaf-core/src/transfer.rs`, 예시 `fixtures/transfer-signature.json`

## Wi-Fi 직접 전송 (기본)

Mac 앱이 HTTP 서버를 열고 Bonjour `_leaf._tcp`로 알립니다. iPhone은 Network.framework `NWBrowser`로 찾습니다.

### 짝짓기

1. Mac 설정에서 "iPhone 연결"을 누르면 6자리 코드가 2분 동안 표시됩니다
2. iPhone에 코드를 넣으면 `POST /v1/pair`로 보냅니다
3. Mac은 32바이트 무작위 토큰을 만들어 돌려주고 키체인에 기기 id와 같이 둡니다. iPhone도 키체인에 둡니다
4. 이후 요청은 토큰을 보내지 않고 서명만 보냅니다

코드를 5번 틀리면 짝짓기 창을 닫습니다.

### 요청 서명

모든 요청(짝짓기, `info` 제외)에 다음 헤더를 붙입니다.

| 헤더 | 값 |
|---|---|
| `X-Leaf-Device` | iPhone 기기 id(UUID) |
| `X-Leaf-Timestamp` | 유닉스 초 |
| `X-Leaf-Body-Sha256` | 본문 SHA-256 hex (본문 없으면 빈 바이트의 해시) |
| `X-Leaf-Signature` | `HMAC-SHA256(토큰, "METHOD\nPATH\nTIMESTAMP\nBODY_SHA256")` hex |

Mac은 시각 차이가 300초를 넘거나, 본문 해시가 다르거나, 서명이 맞지 않으면 401을 돌려줍니다. 경로는 쿼리 없는 경로입니다.

알려진 한계: 본문(사진)은 암호화하지 않습니다. 같은 Wi-Fi 안에서만 오가고, 서명으로 변조와 무단 업로드는 막습니다.

### API

| 요청 | 설명 |
|---|---|
| `GET /v1/info` | `{ "name": "Mac 이름", "version": "0.2.0" }`. 인증 없음 |
| `POST /v1/pair` | 본문 `{ "code", "deviceId", "deviceName" }` → `{ "token": "<hex>", "macName" }` |
| `GET /v1/documents/{id}/status` | `{ "files": { "raw/x.heic": "<sha256>" } }`. 이미 받은 파일 목록(이어 보내기용) |
| `PUT /v1/documents/{id}/files/{상대 경로}` | 원본 파일. Mac은 해시를 확인하고 `incoming/<id>/`에 둔다 |
| `PUT /v1/documents/{id}/manifest` | manifest.json. 모든 `raw` 파일이 와 있어야 200. 이때 보관함으로 옮긴다 |

원본을 먼저, manifest를 마지막에 보냅니다. Mac은 manifest가 도착해야 문서로 인식합니다. 이미 있는 문서면 새 원본만 더하고 manifest를 바꿉니다(`edits.json`은 건드리지 않음). 상대 경로는 `raw/` 아래만 받습니다.

iPhone은 보내기에 성공한 문서를 "Mac에 있음"으로 표시합니다. 원본 삭제는 설정에서 고릅니다(기본: 7일 뒤 지움).

## iCloud 폴더 (보조)

sori와 같이 무료 Apple ID 서명이라 CloudKit은 못 쓰고 iCloud Drive 폴더를 씁니다.

- iPhone: 처음 한 번 파일 앱 폴더 선택기로 `iCloud Drive/leaf`를 고르고 보안 범위 북마크로 기억합니다(sori `docs/sync.md` 방식)
- iPhone은 `leaf/inbox/<문서 id>/`에 원본을 먼저, manifest를 마지막에 복사합니다
- Mac은 `~/Library/Mobile Documents/com~apple~CloudDocs/leaf/inbox`를 지켜보다가 manifest가 있고 모든 원본이 받아진 문서를 보관함으로 가져온 뒤 inbox에서 지웁니다
- 아직 받지 않은 파일(`.icloud` 자리표시)은 `brctl download`나 `NSFileCoordinator`로 받게 한 뒤 다시 확인합니다

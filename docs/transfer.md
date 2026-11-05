# iPhone에서 Mac으로 보내기

2026-11-05 결정: 같은 Wi-Fi면 직접 보내고, 떨어져 있으면 iCloud 폴더로 보냅니다.

코드: 서명 `crates/leaf-core/src/transfer.rs`, 예시 `fixtures/transfer-signature.json`, Mac 서버 `src-tauri/src/transfer/`, 보관함 가져오기 `src-tauri/src/store.rs`(`Store::import`), iCloud inbox `src-tauri/src/inbox.rs`(`scan_inbox`)

## Wi-Fi 직접 전송 (기본)

Mac 앱이 HTTP 서버를 열고 Bonjour `_leaf._tcp`로 알립니다. iPhone은 Network.framework `NWBrowser`로 찾습니다.

- 서버는 앱을 켤 때 `0.0.0.0`의 OS가 고른 포트에 엽니다(`transfer::setup`). 포트는 실행마다 바뀌고 iPhone은 Bonjour로 알아냅니다
- Bonjour는 순수 Rust `mdns-sd`로 알립니다(Windows에도 그대로 쓰려고). 인스턴스 이름은 Mac의 컴퓨터 이름, TXT는 `api=1`, `version=<앱 버전>`입니다. 서버가 IPv4에만 열려 있어 IPv4 주소만 알립니다(`transfer/bonjour.rs`)

### 짝짓기

1. Mac 설정에서 "iPhone 연결"을 누르면 6자리 코드가 2분 동안 표시됩니다
2. iPhone에 코드를 넣으면 `POST /v1/pair`로 보냅니다
3. Mac은 32바이트 무작위 토큰을 만들어 돌려주고 키체인에 기기 id와 같이 둡니다(서비스 `com.nobel6018.leaf.transfer`, 계정은 기기 id). iPhone도 키체인에 둡니다. 기기 목록(id, 이름, 짝지은 시각)은 토큰 없이 앱 데이터 `devices.json`에 둡니다(`transfer/devices.rs`)
4. 이후 요청은 토큰을 보내지 않고 서명만 보냅니다

코드를 5번 틀리면 짝짓기 창을 닫습니다. 맞는 코드는 한 번만 쓰이고 바로 창을 닫습니다. 같은 기기 id가 다시 짝지으면 토큰을 새로 바꿉니다(`transfer/pairing.rs`).

### 요청 서명

모든 요청(짝짓기, `info` 제외)에 다음 헤더를 붙입니다.

| 헤더 | 값 |
|---|---|
| `X-Leaf-Device` | iPhone 기기 id(UUID) |
| `X-Leaf-Timestamp` | 유닉스 초 |
| `X-Leaf-Body-Sha256` | 본문 SHA-256 hex (본문 없으면 빈 바이트의 해시) |
| `X-Leaf-Signature` | `HMAC-SHA256(토큰, "METHOD\nPATH\nTIMESTAMP\nBODY_SHA256")` hex |

Mac은 시각 차이가 300초를 넘거나, 본문 해시가 다르거나, 서명이 맞지 않으면 401을 돌려줍니다. 경로는 쿼리 없는 경로이고, 퍼센트 인코딩을 풀지 않은 요청 줄 그대로입니다(파일 이름이 UUID라 실제로는 인코딩할 글자가 없습니다).

원본 업로드는 서명을 먼저 확인한 뒤 본문을 디스크에 흘려 쓰면서 해시를 구하고, 끝에서 `X-Leaf-Body-Sha256`과 비교합니다. 다르면 임시 파일을 지우고 401입니다.

알려진 한계: 본문(사진)은 암호화하지 않습니다. 같은 Wi-Fi 안에서만 오가고, 서명으로 변조와 무단 업로드는 막습니다.

### API

| 요청 | 설명 |
|---|---|
| `GET /v1/info` | `{ "name": "Mac 이름", "version": "0.2.0" }`. 인증 없음 |
| `POST /v1/pair` | 본문 `{ "code", "deviceId", "deviceName" }` → `{ "token": "<hex>", "macName" }` |
| `GET /v1/documents/{id}/status` | `{ "files": { "raw/x.heic": "<sha256>" } }`. 이미 받은 파일 목록(이어 보내기용) |
| `PUT /v1/documents/{id}/files/{상대 경로}` | 원본 파일. Mac은 해시를 확인하고 `incoming/<id>/`에 둔다 |
| `PUT /v1/documents/{id}/manifest` | manifest.json. 모든 `raw` 파일이 와 있어야 200. 이때 보관함으로 옮긴다 |

오류 응답은 `{ "error": "<코드>", "message": "<설명>" }`입니다.

| 상태 | `error` | 언제 |
|---|---|---|
| 400 | `bad_request` | 형식 오류, `raw/` 밖 경로, manifest id가 경로와 다름, manifest의 `raw`가 `raw/` 밖 |
| 401 | `unauthorized` | 서명, 시각, 본문 해시, 모르는(또는 짝을 끊은) 기기 |
| 401 | `wrong_code` | 짝짓기 코드가 틀림 |
| 403 | `pairing_closed` | 짝짓기 창이 닫혀 있음(안 열림, 2분 지남, 5번 틀림, 이미 씀) |
| 409 | `missing_files` | manifest의 원본이 덜 옴. `missing`에 빠진 상대 경로 목록 |
| 413 | `too_large` | 원본 한 장 200MB, manifest 1MB 초과 |
| 422 | `unsupported_version` | manifest `schemaVersion`이 이 앱보다 큼 |

성공 응답: 원본 `PUT`은 `{ "path", "sha256" }`, manifest `PUT`은 `{ "documentId", "created", "added": [상대 경로] }`입니다.

원본을 먼저, manifest를 마지막에 보냅니다. Mac은 manifest가 도착해야 문서로 인식합니다. 이미 있는 문서면 새 원본만 더하고 manifest를 바꿉니다(`edits.json`은 건드리지 않음). 상대 경로는 `raw/` 아래만 받습니다.

이어 보내기는 파일 단위입니다. `status`는 받는 중 폴더(`incoming/<id>/`)와 보관함에 이미 있는 원본을 함께 돌려주므로, iPhone은 목록에 없거나 해시가 다른 파일만 다시 보냅니다. 받다 끊긴 파일은 이어 쓰지 않고 처음부터 다시 받습니다(임시 파일은 `incoming/<id>/.parts/`에 두고 앱을 켤 때 지웁니다).

가져오기(`Store::import`)는 원자적입니다. 새 문서는 `incoming/` 안 임시 폴더에 원본과 manifest를 다 만든 뒤 이름 바꾸기 한 번으로 `library/<id>/`에 넣습니다. 이미 있는 문서는 새 원본을 하나씩 임시 이름에서 이름 바꾸기로 넣고 manifest를 마지막에 바꿉니다. 보관함에 이미 있는 원본은 덮어쓰지 않습니다.

받는 동안에는 문서 단위로 busy 등록부(`src-tauri/src/busy.rs`)에 올라 앱 안 업데이트가 앱을 다시 켜지 않습니다(`transfer/sessions.rs`). 첫 원본 `PUT`에 올리고 manifest 가져오기가 끝나면 내립니다. 원본 사이 몇 초의 틈에 업데이트가 끼어들지 않게 요청 단위가 아니라 문서 단위로 잡았습니다. 요청 없이 2분이 지나면(iPhone이 Wi-Fi를 벗어남) 내립니다. 받다 만 원본은 `incoming/`에 남아 다음에 이어 보냅니다. iCloud inbox 가져오기도 문서 하나를 복사하는 동안 등록합니다.

iPhone은 보내기에 성공한 문서를 "Mac에 있음"으로 표시합니다. 원본 삭제는 설정에서 고릅니다(기본: 7일 뒤 지움).

## iCloud 폴더 (보조)

무료 Apple ID 서명이라 CloudKit은 못 쓰고 iCloud Drive 폴더를 씁니다.

- iPhone: 처음 한 번 파일 앱 폴더 선택기로 `iCloud Drive/leaf`를 고르고 보안 범위 북마크로 기억합니다
- iPhone은 `leaf/inbox/<문서 id>/`에 원본을 먼저, manifest를 마지막에 복사합니다
- Mac은 `~/Library/Mobile Documents/com~apple~CloudDocs/leaf/inbox`를 지켜보다가 manifest가 있고 모든 원본이 받아진 문서를 보관함으로 가져온 뒤 inbox에서 지웁니다
- 아직 받지 않은 파일(`.icloud` 자리표시)은 `brctl download`나 `NSFileCoordinator`로 받게 한 뒤 다시 확인합니다

Mac 구현(`src-tauri/src/inbox.rs`):

- 5초마다 inbox를 훑습니다. 파일 이벤트는 iCloud가 자리표시를 실제 파일로 바꾸는 순간을 놓치기 쉬워 훑기를 골랐습니다
- 받지 않은 파일은 두 모양으로 봅니다. 옛 방식의 `.이름.icloud` 자리표시와, macOS 14 이후 FileProvider의 내용 없는(dataless, `SF_DATALESS`) 파일입니다. 둘 다 `brctl download <경로>`로 요청하고 같은 파일은 60초에 한 번만 요청합니다
- 폴더 이름이 문서 id(UUID)가 아니면 건너뛰고, manifest id와 다르면 가져오지 않습니다
- 이미 보관함에 있는 원본은 iPhone이 inbox에 다시 올리지 않아도 됩니다
- 보관함에는 복사로 넣고, 가져오기가 끝난 뒤에 inbox 폴더를 지웁니다. 가져오기가 실패하면 inbox는 그대로 남습니다
- iCloud Drive가 켜져 있으면 앱을 켤 때 `leaf/inbox`를 만들어 둡니다. macOS가 아니면 감시하지 않습니다

## Mac 앱 화면 쪽 연결 (Tauri)

화면은 아래 명령과 이벤트만 씁니다. 필드 이름은 JSON 그대로입니다.

| 명령 | 인자 | 돌려주는 값 |
|---|---|---|
| `transfer_status` | 없음 | `{ listening: bool, port: u16, macName: string, pairedDevices: [{ deviceId, deviceName, pairedAt }] }` |
| `start_pairing` | 없음 | `{ code: string, expiresAt: string(RFC 3339) }`. 새 창을 열고 열려 있던 창은 버린다 |
| `cancel_pairing` | 없음 | 없음 |
| `remove_paired_device` | `{ deviceId: string }` | 없음. 목록과 키체인에서 지운다. 실패하면 오류 문자열 |
| `icloud_inbox_status` | 없음 | `{ path: string, available: bool, pending: u32 }`. `pending`은 마지막 훑기에서 아직 못 가져온 문서 수 |

| 이벤트 | 내용 | 언제 |
|---|---|---|
| `library-changed` | `{ documentId: string }` | Wi-Fi나 iCloud로 문서를 가져온 뒤 |
| `pairing-completed` | `{ deviceId: string, deviceName: string }` | 짝짓기에 성공한 뒤 |

`pairedAt`, `expiresAt`은 RFC 3339 UTC 문자열입니다. 서버 포트를 못 열면 앱은 그대로 뜨고 `listening: false`, `port: 0`으로 보입니다.

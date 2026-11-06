# iPhone에서 Mac으로 보내기

2026-11-05 결정: 같은 Wi-Fi면 직접 보내고, 떨어져 있으면 iCloud 폴더로 보냅니다.

2026-11-06 결정: 찍은 장은 곧바로 iCloud 폴더에도 복사합니다(아래 "iCloud 백업"). iOS는 앱을 지우면 샌드박스도 지우므로 `Documents/library`에만 있는 스캔은 앱과 함께 사라집니다. iCloud 저장 공간을 쓰는 것과 회사 자료가 iCloud를 거치는 것은 사용자가 받아들였습니다.

코드: 서명 `crates/scanshelf-core/src/transfer.rs`, 예시 `fixtures/transfer-signature.json`, Mac 서버 `src-tauri/src/transfer/`, 보관함 가져오기 `src-tauri/src/store.rs`(`Store::import`), iCloud inbox `src-tauri/src/inbox.rs`(`scan_inbox`), iPhone 백업 `ios/Scanshelf/Transfer/ICloudBackup.swift`, `ios/Scanshelf/Library/BackupQueue.swift`

## Wi-Fi 직접 전송 (기본)

Mac 앱이 HTTP 서버를 열고 Bonjour `_scanshelf._tcp`로 알립니다. iPhone은 Network.framework `NWBrowser`로 찾습니다.

macOS에서는 시스템 mDNSResponder에 등록합니다(`DNSServiceRegister`). v0.1.0은 자체 mDNS 응답기(mdns-sd)를 썼는데, mDNSResponder가 5353 포트를 쥐고 있어 광고가 루프백에만 보이고 Wi-Fi로 나가지 않았습니다. 확인은 `dns-sd -B _scanshelf._tcp`의 `if` 열에 Wi-Fi 인터페이스 번호가 보이는지로 합니다.

- 서버는 앱을 켤 때 `0.0.0.0`의 OS가 고른 포트에 엽니다(`transfer::setup`). 포트는 실행마다 바뀌고 iPhone은 Bonjour로 알아냅니다
- Bonjour는 순수 Rust `mdns-sd`로 알립니다(Windows에도 그대로 쓰려고). 인스턴스 이름은 Mac의 컴퓨터 이름, TXT는 `api=1`, `version=<앱 버전>`입니다. 서버가 IPv4에만 열려 있어 IPv4 주소만 알립니다(`transfer/bonjour.rs`)

### 페어링

1. Mac 설정에서 "iPhone 연결"을 누르면 6자리 코드가 2분 동안 표시됩니다
2. iPhone에 코드를 넣으면 `POST /v1/pair`로 보냅니다
3. Mac은 32바이트 무작위 토큰을 만들어 돌려주고 키체인에 기기 id와 같이 둡니다(서비스 `com.nobel6018.scanshelf.transfer`, 계정은 기기 id). iPhone도 키체인에 둡니다. 기기 목록(id, 이름, 페어링한 시각)은 토큰 없이 앱 데이터 `devices.json`에 둡니다(`transfer/devices.rs`)
4. 이후 요청은 토큰을 보내지 않고 서명만 보냅니다

코드를 5번 틀리면 페어링 창을 닫습니다. 맞는 코드는 한 번만 쓰이고 바로 창을 닫습니다. 같은 기기 id가 다시 페어링하면 토큰을 새로 바꿉니다(`transfer/pairing.rs`).

### 요청 서명

모든 요청(페어링, `info` 제외)에 다음 헤더를 붙입니다.

| 헤더 | 값 |
|---|---|
| `X-Scanshelf-Device` | iPhone 기기 id(UUID) |
| `X-Scanshelf-Timestamp` | 유닉스 초 |
| `X-Scanshelf-Body-Sha256` | 본문 SHA-256 hex (본문 없으면 빈 바이트의 해시) |
| `X-Scanshelf-Signature` | `HMAC-SHA256(토큰, "METHOD\nPATH\nTIMESTAMP\nBODY_SHA256")` hex |

Mac은 시각 차이가 300초를 넘거나, 본문 해시가 다르거나, 서명이 맞지 않으면 401을 돌려줍니다. 경로는 쿼리 없는 경로이고, 퍼센트 인코딩을 풀지 않은 요청 줄 그대로입니다(파일 이름이 UUID라 실제로는 인코딩할 글자가 없습니다).

원본 업로드는 서명을 먼저 확인한 뒤 본문을 디스크에 흘려 쓰면서 해시를 구하고, 끝에서 `X-Scanshelf-Body-Sha256`과 비교합니다. 다르면 임시 파일을 지우고 401입니다.

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
| 401 | `unauthorized` | 서명, 시각, 본문 해시, 모르는(또는 페어링을 해제한) 기기 |
| 401 | `wrong_code` | 페어링 코드가 틀림 |
| 403 | `pairing_closed` | 페어링 창이 닫혀 있음(안 열림, 2분 지남, 5번 틀림, 이미 씀) |
| 409 | `missing_files` | manifest의 원본이 덜 옴. `missing`에 빠진 상대 경로 목록 |
| 413 | `too_large` | 원본 한 장 200MB, manifest 1MB 초과 |
| 422 | `unsupported_version` | manifest `schemaVersion`이 이 앱보다 큼 |

성공 응답: 원본 `PUT`은 `{ "path", "sha256" }`, manifest `PUT`은 `{ "documentId", "created", "added": [상대 경로] }`입니다.

원본을 먼저, manifest를 마지막에 보냅니다. Mac은 manifest가 도착해야 문서로 인식합니다. 이미 있는 문서면 새 원본만 더하고 manifest를 바꿉니다(`edits.json`은 건드리지 않음). 상대 경로는 `raw/` 아래만 받습니다.

이어 보내기는 파일 단위입니다. `status`는 받는 중 폴더(`incoming/<id>/`)와 보관함에 이미 있는 원본을 함께 돌려주므로, iPhone은 목록에 없거나 해시가 다른 파일만 다시 보냅니다. 받다 끊긴 파일은 이어 쓰지 않고 처음부터 다시 받습니다(임시 파일은 `incoming/<id>/.parts/`에 두고 앱을 켤 때 지웁니다).

가져오기(`Store::import`)는 원자적입니다. 새 문서는 `incoming/` 안 임시 폴더에 원본과 manifest를 다 만든 뒤 이름 바꾸기 한 번으로 `library/<id>/`에 넣습니다. 이미 있는 문서는 새 원본을 하나씩 임시 이름에서 이름 바꾸기로 넣고 manifest를 마지막에 바꿉니다. 보관함에 이미 있는 원본은 덮어쓰지 않습니다. 페이지는 id로 합치므로 같은 문서가 Wi-Fi와 iCloud로 모두 와도 장이 늘지 않습니다.

manifest의 `revision`이 보관함 것보다 작으면 아무것도 바꾸지 않고 성공으로 끝냅니다(`Store::is_stale`). Wi-Fi로 새 판을 받은 뒤 iCloud에서 옛 판이 늦게 도착해도, 나중에 찍은 장이 빠지거나 지운 장이 되살아나지 않습니다.

받는 동안에는 문서 단위로 busy 등록부(`src-tauri/src/busy.rs`)에 올라 앱 안 업데이트가 앱을 다시 켜지 않습니다(`transfer/sessions.rs`). 첫 원본 `PUT`에 올리고 manifest 가져오기가 끝나면 내립니다. 원본 사이 몇 초의 틈에 업데이트가 끼어들지 않게 요청 단위가 아니라 문서 단위로 잡았습니다. 요청 없이 2분이 지나면(iPhone이 Wi-Fi를 벗어남) 내립니다. 받다 만 원본은 `incoming/`에 남아 다음에 이어 보냅니다. iCloud inbox 가져오기도 문서 하나를 복사하는 동안 등록합니다.

iPhone은 보내기에 성공한 문서를 "Mac에 있음"으로 표시합니다. 원본 삭제는 설정에서 고릅니다(기본: 7일 뒤 지움).

## iCloud 백업

무료 Apple ID 서명이라 CloudKit과 앱 전용 iCloud 컨테이너를 못 쓰고, 사용자가 고른 iCloud Drive 폴더를 씁니다. iPhone은 처음 한 번 파일 앱 폴더 선택기로 `iCloud Drive/scanshelf`를 고르고 보안 범위 북마크로 기억합니다.

```
iCloud Drive/scanshelf/
  inbox/<문서 id>/manifest.json, raw/<페이지 id>.heic    iPhone이 쓰고, 가져오면 Mac이 지운다
  received/<문서 id>.json                                Mac만 쓴다
```

### 새 폴더를 만들지 않고 inbox를 그대로 쓴 이유

Mac은 이미 inbox를 5초마다 훑고 합칠 줄 압니다. 백업 전용 폴더를 두면 Mac이 받았을 때 지우는 규칙을 또 만들어야 합니다. 찍을 때마다 inbox에 올리면 Mac이 켜져 있는 한 Wi-Fi 없이도 곧 보관함에 들어오고, 꺼져 있으면 iCloud에 남아 있다가 켜질 때 들어옵니다.

### 흐름

1. 찍으면 iPhone은 `Documents/library`에 저장하고 셔터를 끝냅니다. 복사 일꾼은 깨우기만 합니다(`ICloudBackup.kick`)
2. 일꾼은 메인 스레드 밖에서 한 번에 한 문서씩, Mac에 있다고 확인되지 않은 원본을 먼저, manifest를 마지막에 복사합니다. 그다음 지금 판에 없는 파일을 치웁니다(`ICloudBackupEngine.sync`)
3. Mac은 manifest가 있고 원본이 모두 있으면(inbox나 보관함 어디든) 보관함으로 합칩니다
4. Mac은 `received/<문서 id>.json`을 쓰고, 그 뒤에 가져온 파일만 inbox에서 지웁니다
5. iPhone은 20초마다 확인 파일을 봅니다. 적힌 manifest 해시가 지금 판의 지문과 같으면 "Mac에 있음"이 되고 iCloud 사본을 치웁니다

### 확인 파일

```json
{ "documentId": "<id>", "manifestSha256": "<가져온 manifest 바이트의 SHA-256>", "raw": ["raw/<페이지 id>.heic"] }
```

`raw`는 보관함에 있는 원본 전부입니다. iPhone은 이 원본을 다시 올리지 않습니다. `manifestSha256`은 iPhone의 manifest 지문(`Manifest.fingerprint`)과 같은 값입니다.

폴더가 사라진 것을 받은 것으로 보지 않습니다. v0.2.2까지는 그렇게 판단했는데, Mac이 폴더를 지우는 사이 iPhone이 올린 다음 장이 같이 사라지면 iPhone은 그 장까지 받았다고 믿고 나중에 원본을 지웁니다. 같은 이유로 Mac은 폴더를 통째로 지우지 않습니다.

한 달이 지난 확인 파일은 Mac이 앱을 켤 때 지웁니다(`prune_receipts`). 그 뒤에 iPhone이 확인하려 해도 다시 올리면 같은 파일이 다시 생깁니다.

### 함정과 처리

| 상황 | 처리 | 코드 |
|---|---|---|
| Mac이 가져간 뒤 다음 장을 찍었다. 새 manifest가 inbox에 없는 원본을 적는다 | 보관함에 있는 원본은 있는 것으로 친다. `edits.json`, `processing.json`은 건드리지 않는다 | `import_one`, `Store::import` |
| 앞 장 원본이 자리표시로 inbox에 남았다 | 보관함에 있으면 내려받기를 요청하지 않는다 | `import_one` |
| 다시 찍기, 페이지 지우기 | 새 manifest를 쓴 뒤에 옛 원본을 iCloud에서 지운다. Mac이 먼저 보면 manifest에 없는 파일이라 보관함에 넣지 않고 남겨 두며, iPhone이 지운 뒤 빈 폴더를 치운다 | `ICloudBackupEngine.prune`, `clear_imported`, `remove_if_empty` |
| manifest에 없는 원본이 inbox에 있다 | 다음 장일 수 있어 Mac이 지우지 않는다. 지금 판에 없는 것을 지우는 쪽은 iPhone뿐이다 | `clear_imported` |
| Mac이 읽은 뒤 manifest가 바뀌었다 | 읽은 바이트와 같을 때만 지운다 | `clear_imported` |
| 옛 판이 늦게 왔다 | `revision`으로 거르고 inbox에서 치운다. 원본이 빠져 있어도 기다리지 않는다 | `Store::is_stale` |
| 폴더가 사라졌는데 확인 파일이 없다 | 2분을 기다린 뒤 빠진 것을 다시 올린다. 간격은 두 배씩 1시간까지. 보관함에 있으면 Mac은 아무것도 더하지 않고 확인 파일만 다시 쓴다 | `BackupQueue.recopyGrace` |
| 복사 실패(iCloud 꺼짐, 공간 부족, 폴더 접근 끊김) | 촬영에 영향이 없다. 15초에서 두 배씩 1시간까지 간격을 두고 다시 하고, 앱을 다시 켜면 바로 다시 한다 | `BackupQueue.retryDelay` |
| 복사 도중 앱이 죽었다 | 파일은 임시 이름에 쓴 뒤 이름을 바꾼다. 할 일 목록을 저장하지 않고 문서와 기록에서 계산하므로 잃어버릴 대기열이 없다. 끝난 원본은 크기로 건너뛴다 | `BackupQueue.plan` |
| Wi-Fi로 먼저 받았다 | iPhone이 iCloud 사본을 치우고 그 원본은 다시 올리지 않는다. 치우기 전에 Mac이 inbox를 봐도 장이 늘지 않는다 | `ICloudBackup.noteDelivered` |
| iPhone에서 문서를 지웠다 | iCloud 사본도 지운다. 폴더에 닿지 않으면 기억해 두었다가 치운다(`deletedDocuments`). Mac이 이미 가져갔으면 Mac에는 남는다 | `ICloudBackup.documentDeleted` |
| Mac 보관함에서 문서를 지웠는데 iPhone이 이어 찍었다 | 앞 장이 어디에도 없어 기다리게 되므로 Mac이 확인 파일을 비운다. iPhone은 빈 확인 파일을 보면 원본을 처음부터 올린다. 문서는 Mac에 다시 생긴다(Wi-Fi 이어 찍기와 같다) | `ask_for_everything_again` |

iCloud가 충돌을 풀며 만든 사본(`manifest 2.json` 등)은 iPhone이 다음 복사 때 치웁니다.

### 지키는 규칙

- iPhone의 원본은 Mac이 받았다고 확인되기 전에 지우지 않습니다. 자동 삭제는 Mac이 지금 판을 확인한 문서만 대상입니다(`RetentionPolicy`). iCloud에 올라간 것은 확인이 아닙니다
- 사용자가 지운 장과 다시 찍어 버린 장은 예외입니다. 사용자가 고른 일입니다
- 앱을 지우면 iCloud의 inbox는 남고, Mac이 받아 갑니다. 마지막 장은 manifest를 쓰기 전이면 원본만 남아 Mac이 가져가지 못합니다(파일 앱이나 Finder에서 꺼낼 수 있습니다)

### Mac 구현

`src-tauri/src/inbox.rs`:

- 5초마다 inbox를 훑습니다. 파일 이벤트는 iCloud가 자리표시를 실제 파일로 바꾸는 순간을 놓치기 쉬워 훑기를 골랐습니다
- 받지 않은 파일은 두 모양으로 봅니다. 옛 방식의 `.이름.icloud` 자리표시와, macOS 14 이후 FileProvider의 내용 없는(dataless, `SF_DATALESS`) 파일입니다. 둘 다 `brctl download <경로>`로 요청하고 같은 파일은 60초에 한 번만 요청합니다
- 폴더 이름이 문서 id(UUID)가 아니면 건너뛰고, manifest id와 다르면 가져오지 않습니다
- 보관함에는 복사로 넣습니다. 가져오기나 확인 파일 쓰기가 실패하면 inbox는 그대로 남고 다음 훑기에서 다시 합니다
- iCloud Drive가 켜져 있으면 앱을 켤 때 `scanshelf/inbox`를 만들어 둡니다. macOS가 아니면 감시하지 않습니다

### 버전

iPhone과 Mac을 같이 올려야 합니다. 옛 Mac은 확인 파일을 쓰지 않아 iPhone이 "Mac에 있음"이 되지 못하고 간격을 늘려 가며 다시 올립니다(원본은 Mac으로 들어가고, iPhone 원본은 지우지 않으니 잃는 것은 없습니다). 옛 iPhone은 새 Mac에 그대로 보낼 수 있습니다(`revision` 없음은 0).

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
| `pairing-completed` | `{ deviceId: string, deviceName: string }` | 페어링에 성공한 뒤 |

`pairedAt`, `expiresAt`은 RFC 3339 UTC 문자열입니다. 서버 포트를 못 열면 앱은 그대로 뜨고 `listening: false`, `port: 0`으로 보입니다.

## 이름을 바꾸기 전 데이터

v0.2.0에서 앱 이름과 번들 id를 바꿨습니다(`com.nobel6018.leaf` → `com.nobel6018.scanshelf`).

- Mac은 시작할 때 옛 데이터 폴더의 `library/`, `models/`를 옮깁니다(`src-tauri/src/migrate.rs`)
- Mac은 옛 iCloud 폴더 `iCloud Drive/leaf/inbox`가 있으면 같이 훑어 가져옵니다(`src-tauri/src/inbox.rs` `legacy_inbox_dir`)
- 페어링은 다시 합니다. 토큰이 옛 이름의 키체인 항목에 있습니다
- iPhone은 번들 id가 달라 다른 앱으로 설치됩니다. 옛 앱의 문서는 옮겨지지 않으니, 옛 앱을 지우기 전에 보내기를 끝냅니다

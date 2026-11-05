# iPhone 앱

거치대 촬영, 문서 묶기, Mac으로 보내기를 합니다. 보정과 OCR은 Mac이 합니다(docs/architecture.md).

## 빌드

```bash
cd ios
xcodegen generate
export DEVELOPER_DIR=/Applications/Xcode-27.1.0.app/Contents/Developer
xcodebuild test -project Leaf.xcodeproj -scheme Leaf \
  -destination 'platform=iOS Simulator,name=iPhone 18 Pro' \
  -derivedDataPath build CODE_SIGNING_ALLOWED=NO
```

- `Leaf.xcodeproj`는 `project.yml`에서 만들어지므로 커밋하지 않습니다
- `xcode-select -p`가 CommandLineTools이면 `DEVELOPER_DIR`를 줘야 합니다
- 최소 iOS 18, Swift 6(엄격한 동시성 검사)

## 설치

sori처럼 AltStore로 본인 기기에만 설치합니다. 빌드는 서명 없이 하고 AltStore가 무료 Apple ID로 다시 서명합니다. 무료 Apple ID는 7일마다 다시 서명해야 하고 AltStore가 자동으로 갱신합니다.

무료 Apple ID라 entitlements가 비어 있습니다. CloudKit, 앱 전용 iCloud 컨테이너, 백그라운드 모드를 쓰지 않습니다. 토큰은 앱 기본 키체인 그룹에 둡니다.

## 화면

| 화면 | 코드 | 내용 |
|---|---|---|
| 문서 목록 | `Views/DocumentsView.swift` | 세션별 문서, 보낸 상태(안 보냄, 보내는 중, iCloud 대기, Mac에 있음, 바뀜, 실패), 새 문서, 모두 보내기 |
| 새 문서 | `Views/NewDocumentView.swift` | 종류(팜플렛, 책, 문서), 세션(박람회명, 바꾸기 전까지 기억), 이름(기본 `세션_번호`). 책은 "펼친 두 페이지"가 기본으로 켜짐 |
| 촬영 | `Views/CaptureView.swift`, `Capture/CaptureModel.swift` | 4:3 미리보기, 테두리, 자동 촬영 안내, 자동과 수동 전환, 한 장과 두 쪽 전환, 페이지 띠(누르면 다시 찍기, 지우기) |
| 문서 | `Views/DocumentDetailView.swift` | 이름, 세션, 종류 고치기, 찍은 장, 이어서 찍기, Wi-Fi나 iCloud로 보내기, 문서 지우기 |
| 설정 | `Views/SettingsView.swift` | Mac 연결과 해제, iCloud 폴더, 원본 지우기 기간, 자동 촬영 민감도 |

문서 이름 기본값은 같은 세션에서 쓴 가장 큰 번호 + 1입니다(`Library/SendRecords.swift` `DocumentNaming`). 세션이 없으면 `문서_번호`입니다. 팜플렛은 나중에 Mac AI가 표지에서 회사명을 추천합니다.

다시 찍기를 고르면 다음 촬영이 그 장 자리에 들어가고 예전 원본은 지웁니다(`DocumentStore.addPage(replacing:)`).

## 저장

`Documents/library/<문서 id>/manifest.json`과 `raw/<페이지 id>.heic`. 형식은 docs/document-format.md 그대로입니다(`Library/DocumentStore.swift`).

- 원본을 먼저 쓰고 manifest를 나중에 씁니다. manifest는 `.manifest.json.tmp`에 쓴 뒤 `rename(2)`로 바꿉니다
- id는 소문자로 씁니다. Swift `UUID`는 대문자로 인코딩하므로 `Manifest`, `Page`가 직접 인코딩합니다
- 날짜는 밀리초로 맞춘 값을 메모리에도 둡니다. 저장했다 읽은 값과 같아야 보낸 뒤 바뀌었는지(지문) 판단이 맞습니다
- 저장 전에 Rust와 같은 규칙으로 검사합니다(`Manifest.validate`)

보낸 기록은 문서 폴더 밖 `Application Support/send-records.json`에 둡니다. 문서마다 보낸 방법, 시각, 그때의 manifest 지문(SHA-256), Mac이 받은 것이 확인됐는지를 적습니다.

## 보내기

docs/transfer.md를 따릅니다. 코드는 `ios/Leaf/Transfer/`, 흐름은 `App/AppModel.swift`.

| 경로 | 언제 | 확인 |
|---|---|---|
| Wi-Fi | 짝지은 Mac을 Bonjour 이름으로 4초 안에 찾으면. 못 찾으면 마지막 주소로 시도 | manifest `PUT` 200 |
| iCloud 폴더 | Wi-Fi가 실패하고 폴더를 골라 두었으면, 또는 문서 화면에서 직접 | Mac이 `inbox/<문서 id>`를 지운 것이 iPhone에 내려왔을 때 |

- 촬영 화면을 닫을 때, 앱이 앞으로 올 때, "모두 보내기"를 누를 때 안 보냈거나 보낸 뒤 바뀐 문서를 보냅니다
- 이어 보내기: `status`로 Mac에 같은 해시로 있는 원본을 건너뜁니다. manifest가 409 `missing_files`이면 빠진 원본만 다시 보내고 manifest를 다시 보냅니다(2번까지)
- 서명 경로는 보낼 URL에서 꺼낸 퍼센트 인코딩된 경로 그대로입니다. 경로에는 UUID와 `raw/<uuid>.heic`만 들어가 인코딩할 글자가 없습니다
- 짝짓기 오류는 코드 틀림(401 `wrong_code`, 다시 넣으면 됨)과 창 닫힘(403 `pairing_closed`, Mac에서 새 코드)을 다르게 안내합니다
- Bonjour TXT `api`가 1이 아닌 Mac은 목록에 보이지 않습니다. 주소는 IPv4로 풉니다(Mac 서버가 IPv4만 엽니다)
- iCloud 폴더는 고른 폴더에서 `leaf` 루트를 찾습니다. `leaf` 안의 폴더를 골라도 위로 올라가고, iCloud Drive 자체를 고르면 그 안의 `leaf`를 씁니다. iCloud Drive 밖은 거절합니다(`ICloudFolderLayout.leafRoot`)

### 알려진 한계

- 보내기는 앱이 앞에 있을 때만 합니다. 앱을 내리면 `beginBackgroundTask`로 얻는 30초 안에 보내던 파일까지만 보내고, 나머지는 다음에 앱을 열 때 이어 보냅니다. 백그라운드 `URLSession`은 업로드 파일마다 서명 헤더를 미리 정해야 하고 시각 차이 300초 제한에 걸려 쓰지 않았습니다
- iCloud는 iPhone이 파일을 복사한 것까지만 압니다. 실제로 올라갔는지는 Mac이 가져가 inbox를 지운 뒤에야 확인됩니다. 그 전에는 "iCloud 대기"로 보이고 원본 자동 삭제 대상이 아닙니다

## 원본 자동 삭제

설정의 기간(기본 7일, 0이면 지우지 않음)이 지나고, Mac이 받은 것이 확인됐고, 보낸 뒤 바뀌지 않은 문서를 앱이 앞으로 올 때 지웁니다(`RetentionPolicy`, `AppModel.applyRetention`). Mac에는 원본이 남습니다.

## 시뮬레이터에서 화면 보기

카메라 대신 합성 화면이 나옵니다(docs/capture.md "시뮬레이터"). DEBUG 빌드는 실행 인자로 화면을 바로 엽니다.

```bash
xcrun simctl launch <udid> com.nobel6018.leaf -leafScreen capture   # 촬영(세션 "2026 예시박람회" 문서를 만든다)
xcrun simctl launch <udid> com.nobel6018.leaf -leafScreen new       # 새 문서
xcrun simctl launch <udid> com.nobel6018.leaf -leafScreen settings  # 설정
xcrun simctl io <udid> screenshot --display=1 shot.png
```

## 실기기에서만 확인되는 것

- 카메라 세션, 최대 해상도 HEIC, 사진 EXIF 방향과 quad가 실제 사진 위에서 맞는지
- Vision 문서 감지, 손 감지, feature print 거리와 기준값(거치대, 실제 팜플렛과 책으로 맞춘다)
- 볼륨 버튼과 블루투스 리모컨 셔터
- 로컬 네트워크 권한, Bonjour 찾기, Mac 서버와의 실제 전송
- 키체인(무료 Apple ID 서명에서), iCloud 폴더 선택과 보안 범위 북마크

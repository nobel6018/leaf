# 앱 업데이트

sori와 같은 방식입니다(sori `docs/update.md`). Mac 앱은 앱 안에서 새 버전을 받아 설치하고, iPhone 앱은 AltStore 소스로 업데이트합니다. 릴리스 절차는 docs/release.md입니다.

## Mac 동작

1. 앱을 켜면 `releases/latest/download/latest.json`을 읽어 새 버전이 있는지 확인합니다. 손으로 확인하려면 메뉴 leaf > "업데이트 확인하기…"를 누릅니다
2. 새 버전이 있으면 화면 위에 알림과 "업데이트하고 다시 켜기" 버튼이 뜹니다. 저절로 설치하거나 다시 켜지 않습니다
3. 버튼을 누르면 진행 중인 작업을 확인합니다. 하나라도 있으면 설치하지 않고 무엇이 진행 중인지 알립니다
4. 없으면 받고, 설치 직전에 한 번 더 확인한 뒤 설치하고 다시 켭니다
5. latest.json을 못 읽으면 GitHub API로 최신 Release를 확인해 "내려받기" 버튼(Release 페이지)을 보여 줍니다

## 진행 중인 작업 등록부

sori는 모듈마다 흩어진 상태(녹음기, 취소 표, 요약 표)를 `busy_tasks`에서 모읍니다. leaf는 등록부 하나(`src-tauri/src/busy.rs`)를 둡니다.

```rust
let _busy = busy::start(format!("처리: {}", manifest.name));
// 작업이 끝나거나 오류, panic으로 빠져나가면 guard가 사라지며 목록에서 빠진다
```

다시 켜면 끊기는 작업은 모두 등록합니다: 페이지 처리, OCR, iPhone에서 받는 중인 문서, iCloud inbox 가져오기, AI 정리, 모델 받기, 내보내기.

## 코드 위치

| 위치 | 내용 |
|---|---|
| `src/update/useAppUpdate.ts` | 확인, 진행 중인 작업 확인, 받기, 설치, 다시 켜기 |
| `src/update/updateCheck.ts` | latest.json 실패 시 GitHub API 확인 |
| `src/update/UpdateBanner.tsx` | 알림 문구와 버튼 |
| `src-tauri/src/update.rs` | macOS 앱 메뉴 항목 |
| `src-tauri/src/busy.rs` | 진행 중인 작업 등록부, `busy_tasks` 명령 |
| `src-tauri/tauri.conf.json` `plugins.updater` | 공개 키, latest.json 주소 |
| `src-tauri/capabilities/default.json` | `updater:default`, `process:allow-restart` |

## 서명

업데이트 파일은 Tauri updater 서명(minisign)으로 확인합니다. Apple 개발자 계정이 필요 없습니다. 비밀 키는 이 Mac의 `~/.tauri/leaf-updater.key`와 GitHub secret `TAURI_SIGNING_PRIVATE_KEY`에만 있습니다.

로컬에서 설치 파일을 만들 때:

```sh
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/leaf-updater.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" pnpm tauri build
```

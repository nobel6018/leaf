# 배포

코드: `.github/workflows/release.yml`, `.github/scripts/`

## 올리는 법

버전을 세 곳에서 올립니다: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `ios/project.yml`의 `MARKETING_VERSION`. 테스트가 통과하면 커밋과 `v*` 태그를 같이 push합니다.

release.yml이 만드는 것:

| 파일 | 쓰는 곳 |
|---|---|
| `leaf_<버전>_aarch64.dmg` | Mac 첫 설치 |
| `leaf_<버전>_aarch64.app.tar.gz`, `.sig` | Mac 앱 안 업데이트 |
| `latest.json` | Mac 앱이 업데이트 확인할 때 읽음 |
| `leaf-ios.ipa` | iPhone (AltStore) |
| `apps.json` | AltStore 소스 |

파일이 모두 붙기 전까지 Release는 초안입니다. 그래서 `releases/latest`가 반쯤 올라간 버전을 가리키지 않습니다.

## Mac

- 첫 설치만 dmg를 받아 `xattr -cr /Applications/leaf.app`을 실행합니다(공증하지 않음)
- 이후는 앱 안에서 업데이트합니다. 받은 파일은 서명을 확인한 뒤 설치합니다
- 서명 비밀 키: 이 Mac의 `~/.tauri/leaf-updater.key`(비밀번호 없음)와 GitHub secret `TAURI_SIGNING_PRIVATE_KEY`. 잃으면 설치된 앱에 업데이트를 보낼 수 없습니다. 저장소에는 공개 키만 둡니다

## iPhone

- AltStore에 소스 `https://github.com/nobel6018/leaf/releases/latest/download/apps.json`을 추가합니다
- AltStore는 소스의 권한 목록이 ipa와 다르면 설치를 거부합니다. 그래서 `altstore-source.sh`가 빌드된 앱의 Info.plist에서 읽습니다
- 무료 Apple ID는 7일마다 다시 서명해야 하고 AltStore가 갱신합니다

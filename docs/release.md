# 배포

코드: `.github/workflows/release.yml`, `.github/scripts/`

## 올리는 법

1. 버전을 네 곳에서 올립니다: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `ios/project.yml`의 `MARKETING_VERSION`
2. 로컬 테스트(`cargo test`, `pnpm build`, iPhone `xcodebuild test`, `mcp`의 `npm test`)가 통과하면 커밋과 `v*` 태그를 함께 push합니다. build.yml이 끝나길 기다렸다가 태그를 올리면 너무 오래 걸려서, build.yml은 나란히 돌리고 실패하면 고쳐서 다음 패치로 냅니다
3. Release는 정식(prerelease 아님)입니다. 설치 안내는 `.github/release-notes.md`가 들어갑니다

## 빌드 캐시

- GitHub 캐시는 브랜치 단위입니다. 태그에서 만든 캐시는 다음 태그가 못 읽고, main에서 만든 캐시는 모든 태그가 읽습니다
- 그래서 release.yml을 매주 월요일 03:00 KST에 main에서 돌려 릴리스 빌드 캐시를 데웁니다(파일은 올리지 않음). main push로는 돌지 않습니다
- 의존성을 크게 바꾼 직후에는 Actions에서 release를 손으로 한 번 돌리면 다음 태그가 빠릅니다
- 릴리스 캐시 키에는 러너 이미지 버전을 넣습니다. 이미지가 바뀐 뒤 첫 빌드는 느린 게 정상입니다
- build.yml(디버그)과 release.yml(릴리스)은 캐시 키 앞부분(`v1-debug`, `v1-release`)을 나눠 서로의 캐시를 덮지 않습니다

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

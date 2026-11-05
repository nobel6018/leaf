# iPhone 앱 빌드와 설치

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

## 설치

sori처럼 AltStore로 본인 기기에만 설치합니다. 빌드는 서명 없이 하고 AltStore가 무료 Apple ID로 다시 서명합니다. 무료 Apple ID는 7일마다 다시 서명해야 하고 AltStore가 자동으로 갱신합니다.

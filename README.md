# scanshelf

iPhone 거치대로 팜플렛과 책을 빠르게 찍고, Mac에서 보정, OCR, AI 정리, 내보내기를 하는 개인용 스캐너입니다.

- iPhone 앱(`ios/`): SwiftUI 네이티브. 자동 촬영, 테두리 감지, 볼륨 버튼 셔터
- Mac 앱(`src-tauri/`, `src/`): Tauri 2 + React. 원근 보정, 곡면 보정, OCR, PDF 내보내기, 팜플렛 AI 정리
- 문서 형식(`crates/scanshelf-core/`): 두 앱이 주고받는 `manifest.json`

스캔한 자료는 기기 밖으로 나가지 않습니다. OCR은 기기 안(Apple Vision)에서 하고, AI 정리만 구독 로그인된 `claude` CLI를 부릅니다.

## 문서

| 문서 | 내용 |
|---|---|
| [docs/architecture.md](docs/architecture.md) | 전체 구조, 기기별 역할, 기술 선택 이유 |
| [docs/features.md](docs/features.md) | 기능 목록과 단계별 계획 |
| [docs/document-format.md](docs/document-format.md) | 문서 폴더와 manifest 형식 |
| [docs/capture.md](docs/capture.md) | iPhone 자동 촬영 설계 |
| [docs/transfer.md](docs/transfer.md) | iPhone에서 Mac으로 보내기 |
| [docs/ios.md](docs/ios.md) | iPhone 앱 빌드와 설치 |
| [docs/mcp.md](docs/mcp.md) | Claude에서 스캔 문서를 찾고 읽는 MCP 서버(`mcp/`) |

## 개발

```bash
pnpm install
pnpm tauri dev          # Mac 앱
cargo test              # 문서 형식 테스트

cd ios && xcodegen generate   # iPhone 앱 프로젝트 생성 (docs/ios.md)
```

이 저장소는 공개입니다. 키, 인증서, 토큰과 실제 스캔 결과는 커밋하지 않습니다. `.githooks/pre-commit`이 gitleaks로 검사합니다.

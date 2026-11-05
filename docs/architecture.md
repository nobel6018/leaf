# 구조

## 기기별 역할

iPhone은 촬영만 합니다. 원본 사진과 감지한 테두리 꼭짓점을 문서 폴더에 쌓아 두고, Mac을 만나면 보냅니다. 박람회장처럼 Mac이 없는 곳에서도 계속 찍을 수 있어야 하므로 iPhone은 Mac 없이 완결되게 동작합니다.

Mac은 받은 원본으로 모든 처리를 합니다. 원본은 지우지 않습니다. 처리 방식을 고치면 예전 문서도 다시 처리합니다.

| 단계 | iPhone | Mac |
|---|---|---|
| 촬영 | 미리보기, 테두리 표시, 자동 촬영, 볼륨 버튼 셔터 | |
| 문서 묶기 | 새 문서 시작, 종류(팜플렛, 책, 문서), 세션(박람회명) | 이름 추천(표지 OCR에서 회사명) |
| 보정 | | 원근 보정, 두 페이지 분리, 곡면 보정, 손가락 지우기, 조명 보정 |
| 글자 | | OCR, 검색, 텍스트 레이어 PDF |
| 정리 | | 팜플렛 AI 요약, 노션 CSV |
| 내보내기 | | PDF, JPG, 텍스트, Word, 공유 시트 |

## 저장소 구조

| 경로 | 내용 |
|---|---|
| `crates/leaf-core/` | 문서 형식(`Manifest`, `Page`, `Quad`). 읽기, 검증, 원자적 저장 |
| `src-tauri/` | Mac 앱 Rust 쪽. `library.rs`가 보관함 조회 |
| `src/` | Mac 앱 화면(React) |
| `ios/` | iPhone 앱(SwiftUI). `Leaf/Manifest.swift`가 문서 형식의 Swift 정의 |
| `fixtures/` | 합성 예시 데이터. Rust와 Swift 테스트가 같이 읽는다 |

## 기술 선택

### iPhone은 Swift 네이티브

자동 촬영은 카메라 프레임을 초당 30장씩 분석합니다(`AVCaptureVideoDataOutput` + Vision). 웹뷰 기반(Tauri 모바일)으로는 프레임에 접근할 수 없어 네이티브로 갑니다. 볼륨 버튼과 블루투스 리모컨 셔터(`AVCaptureEventInteraction`)도 네이티브 API입니다.

### Mac은 Tauri + Rust

Windows로 넓힐 여지를 남기려고 Tauri를 골랐습니다(2026-11-05 결정). SwiftUI에서 되는 일 중 Tauri에서 안 되는 일은 없고, 방법만 다릅니다.

| 필요한 것 | 방법 |
|---|---|
| Vision OCR(`VNRecognizeTextRequest`), PDFKit, Core Image, 공유 시트 | `objc2-*` 크레이트로 Rust에서 직접 호출 |
| Vision `RecognizeDocumentsRequest`(macOS 26, 표와 단락 구조) | Swift 전용이라 objc2로는 못 부름. 작은 Swift 정적 라이브러리를 `@_cdecl`로 노출하고 JSON으로 주고받는다 |
| 곡면 보정, 인페인팅 모델 | ONNX Runtime(`ort`). Mac은 CoreML 실행기, Windows는 DirectML. Core ML 전용 모델은 쓰지 않는다 |

Windows를 붙일 때 바꿀 곳은 OCR 하나로 제한합니다. OCR은 플랫폼별 모듈(`ocr/macos.rs`, 나중에 `ocr/windows.rs`)로 나눕니다.

### 문서 형식은 Rust가 원본, Swift는 따라 쓴다

iPhone에 Rust 정적 라이브러리를 링크하는 방법(sori 방식)도 있지만, manifest는 단순한 JSON이라 Swift `Codable`로 다시 쓰는 편이 빌드가 가볍습니다. 대신 두 쪽이 같은 `fixtures/manifest-v1.json`을 읽고 쓰는 테스트로 형식 어긋남을 막습니다. 전송 로직이 복잡해지면 그때 Rust 공유를 다시 검토합니다.

### AI 정리는 구독 CLI

sori와 같이 구독 로그인된 `claude -p`를 부릅니다. API 키가 없어 공개 저장소에 비밀 값이 생기지 않습니다.

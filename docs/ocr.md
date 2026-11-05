# OCR (Mac)

코드: `src-tauri/src/ocr/` (`mod.rs` 공통, `macos.rs` Vision, `blocks.rs` 블록 묶기). 결과 형식: `crates/leaf-core/src/ocr.rs`

## 엔진

Apple Vision `VNRecognizeTextRequest`를 objc2로 부릅니다. 기기 안에서 돌고 네트워크를 쓰지 않습니다.

- 정확도 우선(`Accurate`), 언어 보정 켬
- 언어: `ko-KR`, `en-US`, `ja-JP`, `zh-Hans`, `zh-Hant`. 한국어를 맨 앞에 둬 언어 보정이 한국어를 우선합니다
- 입력은 보정을 마친 페이지 이미지. 좌표는 Vision의 왼쪽 아래 원점을 왼쪽 위 원점으로 뒤집어 저장합니다
- `engine` 값: `vision-recognize-text`

Windows는 `ocr/windows.rs`(Windows.Media.Ocr)를 더하고 `ocr::recognize`의 분기만 늘리면 됩니다.

## 블록 묶기 (`blocks.rs`)

`VNRecognizeTextRequest`는 줄만 줍니다. 단락 구조를 주는 `RecognizeDocumentsRequest`(macOS 26)가 붙기 전까지 아래 규칙으로 묶습니다.

- 두 줄이 같은 블록: 높이 비율 1.6배 이하이고, 세로 간격이 작은 줄 높이의 0.9배 미만이면서 가로로 겹침. 또는 같은 줄에서 끊긴 조각(세로로 절반 이상 겹치고 가로 간격이 높이 두 배 미만)
- 종류: 줄 절반 이상이 머리표(가운뎃점, 하이픈, `1.` 같은 번호)로 시작하면 목록, 줄 3개 이하이고 글자 높이가 페이지 중앙값의 1.5배를 넘으면 제목, 나머지는 단락. 표는 아직 판별하지 않습니다
- 읽는 순서: 블록 상자에 재귀 XY 자르기. 가로 빈 띠가 있으면 위아래로, 없으면 세로 빈 띠로 단을 나눕니다(팜플렛 2단, 3단)
- 블록 글자: 단락과 제목은 줄을 공백으로 잇고(영어 줄 끝 하이픈은 붙임), 목록은 줄바꿈으로 잇습니다. 페이지 `text`는 블록 사이를 빈 줄로 잇습니다

## 후속: RecognizeDocumentsRequest 연결

macOS 26의 `RecognizeDocumentsRequest`는 Swift 전용(구조체 API)이라 objc2로 부를 수 없습니다. 연결 방법:

1. `src-tauri/swift/LeafDocumentOCR.swift`에 `@_cdecl("leaf_recognize_document") func(path: UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>?`를 두고, 결과를 `OcrPage` JSON(`engine: vision-recognize-documents`)으로 돌려줍니다. 해제 함수도 같이 노출합니다
2. `build.rs`에서 `swiftc -emit-library -static -target arm64-apple-macos26`으로 만들고 `cargo:rustc-link-lib=static`, Swift 런타임 경로(`/usr/lib/swift`)를 링크합니다
3. `ocr::recognize`에서 macOS 26 이상이면 Swift 쪽을 먼저 부르고, 실패하거나 낮은 버전이면 지금 경로로 돌아갑니다

배포 최소 버전이 14.2라 빌드 대상과 실행 중 버전 확인(`#available`)을 같이 둬야 합니다. 이번 단계에서는 하지 않았습니다.

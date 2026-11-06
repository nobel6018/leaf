# Finder에서 가져오기 (Mac)

코드: `src-tauri/src/import.rs` (`import_files`, `pick_import_files`, `run_import`, `build_document`, `pdf_macos::render`)

iPhone 없이 Mac에 있는 그림과 PDF를 보관함에 넣습니다. 들어온 문서는 iPhone이 찍은 문서와 똑같이 보정, OCR, 검색, 내보내기를 거칩니다.

## 들어오는 길

| 길 | 어디서 받나 |
|---|---|
| Finder 우클릭 "다음으로 열기 > scanshelf", Dock 아이콘에 끌어다 놓기 | `lib.rs` `RunEvent::Opened` → `Action::Import` → 화면이 `import_files` |
| 앱이 이미 떠 있을 때 두 번째 실행 인자 | `lib.rs` single-instance → 같음 |
| 창에 끌어다 놓기 | `App.tsx` `onDragDropEvent` → `import_files` |
| 파일 메뉴 "가져오기…"(⌘O), 사이드바 "가져오기" | `actions::add_import_menu` → `Action::PickImport` → `pick_import_files` → `import_files` |

scanshelf는 Finder에 보조 앱(`LSHandlerRank: Alternate`)으로만 등록해 jpg, png, heic, PDF의 기본 앱을 바꾸지 않습니다(`src-tauri/tauri.macos.conf.json`). Windows 설치 파일은 파일 연결을 넣으면 기본 앱을 덮어써서 macOS 설정에만 둡니다.

## 받는 파일

| 형식 | 확장자 | 원본(`raw/`) |
|---|---|---|
| 그림 | jpg, jpeg, png, heic, heif | 그대로 복사. `raw/<쪽 id>.<확장자>`(jpeg는 jpg로) |
| PDF | pdf | 쪽마다 300dpi로 그린 JPEG(품질 92). `raw/<쪽 id>.jpg` |

- 절대 경로의 실제 파일만 받습니다. 네트워크 경로(`//server/share`)는 여는 순간 로그인 정보가 그 서버로 가므로 거부합니다(`importable_files`)
- 함께 고른 그림은 파일 이름순으로 한 문서가 됩니다(그림 한 장이 한 쪽). 이름은 첫 파일 이름, 둘 이상이면 `<첫 이름> 외 n장`
- PDF는 파일마다 한 문서이고 이름은 파일 이름입니다
- 종류는 `document`, 묶음은 없음, 쪽 배치는 `single`입니다. 책이나 팜플렛이면 문서 화면에서 바꿉니다
- Finder의 원본 파일은 건드리지 않습니다
- 못 가져온 파일은 이유와 함께 `skipped`로 돌려주고 화면 위 오류 줄에 보여 줍니다. 한 문서만 가져왔으면 그 문서를 바로 엽니다

## manifest를 Mac이 쓰는 예외

문서 형식(`docs/document-format.md`)에서 `manifest.json`은 iPhone만 씁니다. 가져오기는 예외입니다. 가져오기가 "찍은 기기" 역할을 해서, 새 문서의 manifest를 처음 한 번 씁니다. 그 뒤로는 iPhone 문서와 같이 Mac은 manifest를 고치지 않고 `edits.json`에 수정을 남깁니다. 가져온 문서는 iPhone에 없으므로 iPhone이 같은 id로 manifest를 다시 보내는 일도 없습니다.

| 필드 | 값 |
|---|---|
| `schemaVersion` | `SCHEMA_VERSION` |
| `id`, `pages[].id` | 새 UUID |
| `createdAt` | 가져온 시각(밀리초) |
| `pages[].capturedAt` | 그림은 파일 수정 시각, PDF는 가져온 시각(밀리초) |
| `pages[].quad` | 비움. 처리 단계가 원본 해상도에서 다시 감지하고, 못 찾으면 사진 전체를 씁니다 |

## 보관함에 넣는 순서

1. 받는 중 폴더 `incoming/<문서 id>/raw/`에 원본을 복사하거나 PDF 쪽을 그립니다(JPEG는 `fsio::write_atomic`)
2. manifest를 만들어 `Store::import(.., ImportMode::Move)`로 보관함에 넣습니다. 새 문서는 임시 폴더에 다 만든 뒤 이름 바꾸기 한 번으로 들어가 반쯤 만든 문서가 목록에 보이지 않습니다
3. 실패하면 받는 중 폴더를 지웁니다
4. 검색 색인에 넣고(`search::refresh_document`) `library-changed`를 보냅니다. lib.rs가 이 이벤트를 받아 처리 줄에 넣습니다

문서 하나마다 busy에 "가져오기: 문서 이름"으로 등록합니다(쪽이 많은 PDF는 수십 초 걸립니다).

## PDF 그리기

macOS CoreGraphics(`CGPDFDocument`, `CGContextDrawPDFPage`)로 그립니다. PDFKit보다 아래 단계라 비트맵 크기와 색 공간을 직접 정합니다.

- 크기: 자르기 상자(CropBox)를 72pt = 1인치로 보고 300dpi. A4는 2479 x 3508픽셀입니다. 포스터처럼 큰 쪽은 긴 변을 7000픽셀로 줄입니다(`PDF_MAX_PIXELS`)
- 회전: 쪽의 `/Rotate`(90, 270)를 따라 가로세로를 바꿉니다. `CGPDFPageGetDrawingTransform`은 키우지 않으므로 쪽 크기 그대로의 변환(회전, 원점)을 구한 뒤 배율을 따로 겁니다
- 바탕: 투명한 PDF는 흰 종이로 봅니다. sRGB RGBA 비트맵을 255로 채우고 그립니다
- 암호가 걸린 PDF는 거부합니다
- Windows는 아직 PDF를 받지 않습니다(`render_pdf_pages`가 오류). 붙일 때 `Windows.Data.Pdf`로 같은 자리를 채웁니다

원본 PDF 파일 자체는 보관함에 두지 않습니다. 그린 JPEG가 그 문서의 원본입니다. 해상도를 바꿔 다시 그리려면 다시 가져옵니다.

## 시험

`cargo test --lib import::`가 합성 데이터로 확인합니다.

| 테스트 | 확인하는 것 |
|---|---|
| `그림은_이름순으로_한_문서_pdf는_파일마다_한_문서` | 묶는 규칙과 이름 |
| `그림을_가져오면_manifest와_원본이_보관함에_생긴다` | manifest 필드, 원본 그대로 복사, 밀리초, 받는 중 폴더 비우기 |
| `pdf는_쪽마다_jpeg_원본이_된다` | 쪽마다 JPEG 원본(가짜 그리기 함수) |
| `쪽이_없는_pdf는_건너뛰고_받는_중_폴더를_남기지_않는다` | 실패 정리 |
| `합성_pdf를_300dpi로_그린다` | CoreGraphics로 만든 200 x 100pt PDF를 833 x 417픽셀로 그리고 왼쪽 검정, 오른쪽 흰색 확인 |

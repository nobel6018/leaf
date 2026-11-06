# 내보내기 (Mac)

코드: `src-tauri/src/export/` (`mod.rs`, `pdf_macos.rs`, `share_macos.rs`). 개수 제한은 없습니다.

결과는 문서 폴더 `exports/`에 쌓이고 같은 형식은 덮어씁니다. 파일 이름은 문서 이름에서 `/`, `:` 같은 글자를 `_`로 바꾼 값입니다.

| 형식 | 파일 | 만드는 법 |
|---|---|---|
| PDF | `<이름>.pdf` | CoreGraphics PDF 문맥. 페이지 그림 + 보이지 않는 글자 층 |
| JPG | `<이름>_jpg.zip` | 처리된 페이지 JPEG를 압축 없이(Stored) 묶음. `<이름>_01.jpg` |
| 텍스트 | `<이름>.txt` | 쪽마다 `[n쪽]` 머리 + OCR 글자 |
| Word, AI 묶음 | ai 팀 `scanshelf-ai` | 화면 단추만 두고 `export_docx`, `export_ai_bundle` 명령을 부름 |

## 텍스트 레이어 PDF

- 페이지 크기: 긴 변을 842pt(A4 긴 변)에 맞춤. 사진만으로는 종이 크기를 알 수 없어서입니다
- 그림: `CGImageCreateWithJPEGDataProvider`로 만든 그림은 PDF에 다시 압축하지 않고 JPEG 그대로(DCTDecode) 들어갑니다
- 글자: OCR 줄마다 CoreText `CTLine`을 `kCGTextInvisible` 모드로 그립니다. 글꼴 크기는 줄 상자 높이의 0.9배, 가로는 글꼴 폭과 상자 폭의 비율로 텍스트 행렬을 늘이거나 줄여 선택 영역이 그림 속 글자와 겹치게 합니다. 글꼴은 `AppleSDGothicNeo-Regular`(한글, 영문 글리프가 다 있음). CoreText가 쓰인 글리프만 넣습니다
- 확인: `e2e_tests.rs`가 PDFKit(`PDFDocument.string`)으로 글자를 다시 꺼내 한국어를 찾습니다. `pdftotext`로도 한국어가 나오는 것을 확인했습니다

## 화면에서 하는 일

| 명령 | 설명 |
|---|---|
| `export_document(documentId, format)` | `pdf`, `jpg`, `txt`. busy 등록 |
| `list_exports(documentId)` | exports/ 파일 목록(최신순) |
| `reveal_export(path)` | Finder에서 보기 |
| `share_export(path, x, y)` | 공유 시트(`NSSharingServicePicker`)를 단추 자리에 띄움. 메인 스레드에서 실행 |
| `save_export_as(path)` | 저장 창으로 고른 곳에 복사 |

화면이 주는 경로는 보관함 안 `exports/` 파일인지 확인한 뒤에만 엽니다.

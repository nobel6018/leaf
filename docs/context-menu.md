# 우클릭 메뉴

보관함의 문서 카드를 우클릭하면 그 문서에 할 수 있는 동작을 OS 메뉴로 띄웁니다. 문서를 열지 않고도 다시 처리, 내보내기, 휴지통으로 옮기기가 됩니다.

## 코드 위치

| 무엇 | 어디 |
|---|---|
| 메뉴 항목과 동작 | `src/documentMenu.ts` `showDocumentMenu` |
| 지우기 확인(⌘⌫와 같이 씀) | `src/documentMenu.ts` `confirmAndDelete` |
| 카드 우클릭 받기 | `src/views/LibraryView.tsx` `onContextMenu` |
| 알림, 지운 뒤 목록 갱신 | `src/App.tsx` `<LibraryView onContextMenu>` |
| 휴지통으로 옮기기 | `src-tauri/src/actions.rs` `delete_document` |
| Finder에서 보기 | `src-tauri/src/actions.rs` `reveal_document` |
| 웹뷰 기본 메뉴 끄기 | `src/App.tsx` (contextmenu 리스너) |

## 항목

| 항목 | 켜지는 상태 | 동작 |
|---|---|---|
| 열기 | 늘 | 문서 화면 |
| 다시 처리 | 처리 대기, 처리 중이 아닐 때 | `reprocess_document(force: true)` |
| 내보내기 > PDF, JPG 묶음, 텍스트, Word, AI 묶음 | 처리된 쪽이 있을 때 | `export_document`, `export_docx`, `export_ai_bundle`. 다 되면 위 알림 줄에 "Finder에서 보기" |
| 글자를 마크다운으로 복사 | 처리된 쪽이 있을 때 | `document_markdown` (빠른 찾기의 ⇧⌘C와 같음) |
| Finder에서 보기 | 늘 | 문서 폴더를 Finder에서 보여 줌 |
| 휴지통으로 옮기기… | 처리 대기, 처리 중이 아닐 때 | 확인 창 뒤 `delete_document` |

Word와 AI 묶음은 종류와 상관없이 늘 보입니다. 둘 다 일반 내보내기입니다(팜플렛 정리와 별개).

## 휴지통으로 옮기기

`delete_document(documentId)`는 문서 폴더를 macOS 휴지통으로 옮깁니다(`NSFileManager trashItemAtURL`). 완전히 지우지 않으므로 Finder 휴지통에서 "되돌려 놓기"로 되살립니다. 되살린 문서는 다음 실행 때 `search::refresh_all`이 다시 색인합니다.

- 처리 대기나 처리 중인 문서는 거부합니다. 작업 줄이 사라진 폴더를 처리하다 실패하지 않게 합니다
- 다시 만들 수 있는 캐시(`thumbs/<id>`, `previews/<id>`)는 바로 지웁니다
- 검색 색인에서 뺍니다(`search::refresh_document`가 폴더가 없으면 지움)
- `document-deleted { documentId }` 이벤트를 보냅니다. `library-changed`는 받은 문서를 처리 줄에 넣으므로(lib.rs) 쓰지 않습니다. 화면은 이 이벤트로 목록을 다시 읽고, 보고 있던 문서면 보관함으로 돌아갑니다
- busy에 "휴지통으로 옮기기: 문서 이름"으로 등록합니다

## 웹뷰 기본 메뉴

배포 빌드에서는 웹뷰 기본 메뉴("다시 불러오기")를 막습니다. 누르면 화면이 새로 불려 열어 둔 상태가 사라지기 때문입니다. 글 입력칸과 글을 선택한 상태에서는 복사, 붙여넣기 메뉴가 필요해 그대로 둡니다. 개발 빌드(`pnpm tauri dev`)는 요소 검사를 쓰려고 막지 않습니다.

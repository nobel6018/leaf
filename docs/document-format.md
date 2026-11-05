# 문서 형식

코드: `crates/leaf-core/src/lib.rs`(원본), `ios/Leaf/Manifest.swift`(Swift 정의). 예시: `fixtures/manifest-v1.json`

## 폴더

문서 하나가 폴더 하나입니다. 폴더 이름은 문서 id(UUID)입니다.

```
<문서 id>/
  manifest.json        페이지 목록과 촬영 정보
  raw/<페이지 id>.heic iPhone 원본. 지우지 않는다
  pages/               Mac 처리 결과 (예정)
  ocr/                 페이지별 OCR 결과 (예정)
```

## manifest.json

| 필드 | 뜻 |
|---|---|
| `schemaVersion` | 형식 버전. 앱이 아는 버전보다 크면 읽지 않는다 |
| `id` | 문서 id |
| `name` | 문서 이름. 예: `2026 데이터센터코리아_한빛로보틱스` |
| `kind` | `pamphlet`, `book`, `document` |
| `session` | 여러 문서를 묶는 촬영 세션(박람회명). 없어도 된다 |
| `createdAt` | RFC 3339, UTC |
| `pages[].id` | 페이지 id |
| `pages[].raw` | 문서 폴더 기준 상대 경로. 폴더 밖을 가리키면 거부 |
| `pages[].capturedAt` | 촬영 시각 |
| `pages[].layout` | `single` 또는 `spread`(펼친 두 페이지) |
| `pages[].quad` | 테두리 네 꼭짓점. 원본 기준 0~1 좌표, 원점 왼쪽 위, EXIF 회전 적용 후. 감지 실패면 없음 |

## 규칙

- 저장은 임시 파일에 쓴 뒤 이름 바꾸기로 합니다. 전송이나 iCloud 동기화 중에 반쯤 쓴 파일을 읽지 않게 합니다
- 날짜는 밀리초 단위로 씁니다. Swift `Date`는 Double이라 그대로 쓰면 저장할 때마다 1ms씩 밀립니다(`Manifest.formatDate`)
- 형식을 바꾸면 `SCHEMA_VERSION`을 올리고 `fixtures/`에 새 예시를 추가합니다. 옛 예시는 지우지 않습니다

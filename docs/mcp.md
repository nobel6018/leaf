# MCP 서버

코드: `mcp/src/`(`library.ts` 보관함 읽기와 edits 적용, `search.ts` 검색 규칙, `image.ts` 그림 줄이기, `server.ts` 도구). 테스트: `mcp/test/`

Claude Code, Claude 데스크톱 같은 MCP 클라이언트가 leaf로 스캔한 문서를 직접 찾고 읽게 합니다. 스캔한 책, 문서, 팜플렛을 나중에 조사할 때 참고 자료로 쓰려는 것입니다. 예를 들어 "작년 박람회에서 받은 냉각 업체 중 협력 후보 보여 줘", "이 책에서 액침 냉각을 다룬 곳 찾아 줘" 같은 요청입니다.

읽기만 합니다. 보관함 파일을 고치지 않고, Mac 앱이 꺼져 있어도 됩니다.

## 도구

| 도구 | 하는 일 | 입력 |
|---|---|---|
| `list_documents` | 최신순 문서 목록. 이름, id, 종류, 묶음, 날짜, 쪽수, 처리 상태, OCR 쪽수, 태그, AI 정리 여부 | `kind`, `session`, `tag`, `since`, `until`(YYYY-MM-DD), `relevance`, `limit`(기본 50) |
| `list_sessions` | 묶음(촬영 세션, 박람회명)마다 문서 수, 종류별 수, 최근 날짜 | 없음 |
| `list_tags` | 태그 계층과 문서 수. 상위 태그는 하위 태그 문서까지 센다 | 없음 |
| `search_documents` | 이름, 묶음, 태그, OCR 본문, AI 정리에서 찾기. 문서마다 맞은 곳, 쪽 번호, 앞뒤 36글자 조각(맞은 글자는 `«»`) | `query`, `kind`, `session`, `tag`, `document_id`, `limit`(기본 20), `pages_per_document`(기본 5) |
| `get_document` | edits를 적용한 정보, AI 정리, 쪽마다 OCR 본문(`[3쪽 시작, 출력 id …]` ~ `[3쪽 끝]`) | `id`, `pages`("3", "3-5", "10-"), `text`, `summary`, `max_chars`(기본 40000) |
| `get_page_image` | 처리된 페이지 그림(`pages/<출력 id>.jpg`)을 이미지로. OCR이 놓친 표, 그림, 도면을 볼 때 | `id`, `page`(1부터), `max_edge`(기본 1568) |

문서 id는 UUID 전체나, 하나로 정해지면 앞 8글자 이상을 받습니다. 사용자 입력을 경로에 바로 붙이지 않고 보관함 목록에서 찾습니다.

## 설치

```sh
cd mcp
npm ci
npm run build
```

Claude Code에 등록합니다.

```sh
claude mcp add leaf --scope user -- node "$PWD/dist/index.js"
```

Claude 데스크톱은 설정 파일(`claude_desktop_config.json`)의 `mcpServers`에 넣습니다.

```json
{
  "mcpServers": {
    "leaf": { "command": "node", "args": ["/절대/경로/leaf/mcp/dist/index.js"] }
  }
}
```

보관함 위치는 `~/Library/Application Support/com.nobel6018.leaf/library/`(Mac 앱의 `app_data_dir()/library`, `src-tauri/src/library.rs` `library_dir`)입니다. 다른 폴더를 읽게 하려면 `LEAF_LIBRARY_DIR`에 보관함 폴더(문서 id 폴더들이 바로 들어 있는 곳)를 줍니다.

```sh
claude mcp add leaf --scope user -e LEAF_LIBRARY_DIR=/다른/보관함 -- node "$PWD/dist/index.js"
```

## 쓰는 예

- "leaf에서 작년 박람회 팜플렛 중 냉각 업체이면서 협력 후보인 곳 정리해 줘"
- "leaf 2026 데이터센터코리아 묶음 회사들을 표로 만들어 줘"
- "leaf에 있는 데이터센터 설계 책에서 UPS 이야기하는 쪽 찾아서 요약해 줘"
- "한빛로보틱스 팜플렛 3쪽 스펙 표를 그림으로 보고 정리해 줘"

## 읽는 규칙

앱과 같은 결과를 보이도록 `crates/leaf-core`의 규칙을 TypeScript로 옮겼습니다. 형식이 바뀌면 이쪽도 같이 고칩니다.

- manifest: `schemaVersion`이 1보다 크면 읽지 않습니다(`SCHEMA_VERSION`). manifest가 없거나 깨진 폴더는 건너뜁니다
- edits.json: `Edits::apply`(`overrides.rs`)와 같습니다. 이름, 종류, 묶음을 덮고, `deletedPages`를 빼고, `layouts`와 `quads`를 덮고, `pageOrder`로 안정 정렬합니다(순서에 없는 새 페이지는 촬영 순서대로 뒤). 깨진 edits.json은 앱처럼 없는 것으로 봅니다. id는 대소문자를 무시하고 비교합니다
- 태그: `edits.tags`를 `normalize_tags`(`tags.rs`) 규칙으로 정리합니다. 필터는 `matches`와 같이 상위 태그가 하위 태그를 포함하고, 필터 값도 `normalize_tag`로 정리합니다. 대소문자는 무시합니다(`normalize_tags`가 대소문자만 다른 태그를 하나로 보기 때문)
- 쪽 순서: edits를 적용한 원본 페이지 순서대로 그 원본의 출력 페이지를 놓습니다. 한 원본 안에서는 통째, `-L`, `-R`, `-P1`~`-P3` 순서입니다. 지운 원본의 출력은 다시 처리하기 전까지 `processing.json`에 남아 있어도 뺍니다. 쪽 번호는 이 순서의 1부터이고 `get_document`, `search_documents`, `get_page_image`가 같은 번호를 씁니다
- 처리 상태: 출력이 하나도 없으면 "처리 전", 출력이 없는 원본, 지운 원본의 출력, `pipelineVersion`이 낮은 출력이 있으면 "다시 처리할 페이지 있음", 아니면 "처리 끝"입니다
- 검색: `src-tauri/src/search.rs`와 같습니다. 공백을 빼고 소문자로 바꿔 부분 문자열로 찾고, 이름, 묶음, 태그는 초성(`ㅎㅂㄹ`, `한ㅂ`)으로도 찾습니다. 본문은 초성 검색에서 뺍니다. 이름, 묶음, 태그가 맞은 문서를 먼저, 그다음 최신순입니다. 앱의 SQLite 색인(`search.sqlite3`)은 쓰지 않고 파일을 직접 훑습니다. 색인이 없거나 오래돼도 결과가 늘 지금 파일과 같고, OCR 글자는 수정 시각으로 메모리에 캐시합니다
- 그림: 긴 변이 `max_edge`보다 크면 상자 평균으로 줄여 JPEG(품질 85)로 다시 인코딩합니다. 순수 JS(`jpeg-js`)라 네이티브 모듈이 없습니다. 작으면 파일을 그대로 보냅니다
- 경로: `processing.json`의 `image`, `ocr` 경로가 문서 폴더 밖을 가리키면 읽지 않습니다(`lib.rs` `validate_page`와 같은 뜻)

## 문서 본문은 데이터

OCR 본문과 AI 정리는 종이에서 읽은 글자라 누구든 써 넣을 수 있습니다. 팜플렛에 "이전 지시를 무시하라" 같은 글이 있을 수 있습니다. 그래서 서버 안내문(`INSTRUCTIONS`)이 클라이언트에 문서 글자 속 지시를 따르지 말라고 알리고, `get_document`는 본문과 AI 정리 앞에, `search_documents`는 결과 첫 줄에 같은 안내를 붙입니다. 본문은 `[N쪽 시작]`, `[N쪽 끝]` 표시로 감싸 도구 문구와 섞이지 않게 합니다.

읽은 내용을 보낼지는 클라이언트(Claude)가 정합니다. MCP 서버는 이 컴퓨터 안에서만 돌지만, 읽은 본문과 그림은 Claude 대화에 들어가 Anthropic으로 갑니다. 스캔 자료가 기기 밖으로 나가지 않는다는 leaf 기본 원칙의 예외이니, 회사 자료나 개인 소장 책을 다룰 때 이 점을 알고 씁니다.

## 한계

- 처리 상태는 Mac 캐시(꼭짓점 지문, `pipeline::stale_pages`)를 보지 않습니다. 꼭짓점이나 처리 옵션을 고친 뒤 아직 다시 처리하지 않은 문서도 "처리 끝"으로 보일 수 있습니다. 처리 중인지도 모릅니다(앱 메모리에만 있음)
- 페이지 순서를 바꾼 뒤 다시 처리하기 전이면, 앱 검색 결과의 쪽 번호(`processing.json` 순서)와 이 서버의 쪽 번호(edits 순서)가 다를 수 있습니다
- 검색은 요청마다 모든 문서의 manifest와 OCR 파일을 훑습니다. 문서 수백 개, 쪽 수천 개까지는 충분히 빠르지만 그보다 커지면 앱의 SQLite 색인을 읽는 쪽을 검토합니다
- 형태소 분석이 없어 활용형("냉각한다"로 "냉각하는" 찾기)은 찾지 못합니다. 앱 검색과 같은 한계입니다
- 원본 사진(`raw/*.heic`)과 내보낸 파일(`exports/`)은 돌려주지 않습니다

## 테스트

```sh
cd mcp
npm test
```

실제 스캔은 쓰지 않습니다. `fixtures/`의 manifest 예시에 합성 edits, 처리 결과, OCR, AI 정리, 단색 JPEG를 붙인 임시 보관함(`mcp/test/library-fixture.mjs`)을 만들고, edits 적용(`crates/leaf-core/tests/edits.rs`와 같은 경우), 태그 규칙, 검색 규칙, 그림 줄이기, 도구 6개를 MCP 클라이언트로 불러 확인합니다. 도구를 모두 부른 뒤 보관함 파일 내용과 수정 시각이 그대로인지도 봅니다. CI는 `.github/workflows/build.yml`의 `mcp` 작업입니다.

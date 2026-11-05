# leaf MCP 서버

Claude Code, Claude 데스크톱 같은 MCP 클라이언트에서 leaf로 스캔한 문서(책, 문서, 팜플렛)를 찾고 읽습니다. 읽기만 하고, Mac 앱이 꺼져 있어도 됩니다. 자세한 규칙과 한계는 [docs/mcp.md](../docs/mcp.md)에 있습니다.

| 도구 | 하는 일 |
|---|---|
| `list_documents` | 최신순 문서 목록. 종류, 묶음, 태그(상위 태그는 하위 포함), 날짜 범위, 팜플렛 관련성으로 거른다 |
| `list_sessions` | 묶음(박람회처럼 함께 찍은 모음)마다 문서 수 |
| `list_tags` | 태그 계층과 문서 수 |
| `search_documents` | 이름, 묶음, 태그, OCR 본문, AI 정리에서 찾기. 띄어쓰기와 대소문자 무시, 이름과 묶음과 태그는 초성(ㅎㅂㄹ)도. 쪽 번호와 조각을 준다 |
| `get_document` | 정보, AI 정리, 쪽마다 OCR 본문. `pages`("12-20")로 일부만, `max_chars`로 길이 제한. 잘리면 이어 읽을 범위를 알려 준다 |
| `get_page_image` | 처리된 페이지 그림. 긴 변을 1568px(기본)로 줄인다 |

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

보관함은 `~/Library/Application Support/com.nobel6018.leaf/library/`를 읽습니다. 다른 곳을 읽게 하려면 `LEAF_LIBRARY_DIR` 환경 변수에 보관함 폴더를 줍니다.

## 개발

```sh
npm test    # 빌드 뒤 합성 보관함으로 테스트
```

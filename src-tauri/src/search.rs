//! 보관함 검색: OCR 글자는 SQLite FTS5 trigram, 문서 이름과 세션은 초성까지.
//!
//! trigram 토크나이저는 형태소 분석 없이 세 글자 조각으로 색인해 한국어 부분 문자열(“센터”가 “데이터센터”에)을 찾는다.
//! OCR은 띄어쓰기를 자주 다르게 읽어 공백을 뺀 글자로 색인하고 찾는다.
//!
//! @see docs/search.md

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, params};
use serde::Serialize;
use tauri::Manager;

use crate::library::{load_applied, load_processing, read_json};

pub struct SearchIndex {
    conn: Mutex<Connection>,
}

impl SearchIndex {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| format!("검색 색인을 열지 못했다: {e}"))?;
        init_schema(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 형식을 바꾸면 올린다. 색인은 언제든 다시 만들 수 있어 옛 표를 지우고 새로 만든다
const INDEX_VERSION: i64 = 1;
/// 문서 하나에서 보여 줄 최대 페이지 수. 나머지는 개수만 알린다
const HITS_PER_DOCUMENT: usize = 20;
const SNIPPET_CONTEXT: usize = 36;

pub fn init_schema(conn: &Connection) -> Result<(), String> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if version != INDEX_VERSION {
        conn.execute_batch("DROP TABLE IF EXISTS docs; DROP TABLE IF EXISTS pages;")
            .map_err(|e| e.to_string())?;
    }
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS docs(
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            session TEXT,
            kind TEXT NOT NULL,
            created_at TEXT NOT NULL,
            fingerprint TEXT NOT NULL
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS pages USING fts5(
            doc_id UNINDEXED, output_id UNINDEXED, page_no UNINDEXED, text UNINDEXED, compact,
            tokenize = 'trigram'
        );
        PRAGMA user_version = {INDEX_VERSION};"
    ))
    .map_err(|e| format!("검색 색인 준비 실패: {e}"))
}

/// 공백을 빼고 소문자로. 색인과 검색어가 같은 규칙을 따른다
pub fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect()
}

/// 한글 초성(ㄱ~ㅎ). 음절의 초성 순서와 같다
const INITIALS: [char; 19] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];

fn initial_of(c: char) -> Option<char> {
    let code = c as u32;
    (0xAC00..=0xD7A3)
        .contains(&code)
        .then(|| INITIALS[((code - 0xAC00) / 588) as usize])
}

/// 검색어 글자가 같거나, 검색어 글자가 초성이고 대상 음절의 초성이 그것이면 맞는다. "한ㅂ"은 "한빛"에 맞는다
fn char_matches(hay: char, needle: char) -> bool {
    hay == needle || (INITIALS.contains(&needle) && initial_of(hay) == Some(needle))
}

/// 초성을 섞은 검색어로 부분 문자열을 찾는다(ㅂㅅㅎ, 한ㅂ). 둘 다 compact를 거친 값이다
pub fn matches_with_initials(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    if hay.contains(needle) {
        return true;
    }
    if !needle.chars().any(|c| INITIALS.contains(&c)) {
        return false;
    }
    let hay: Vec<char> = hay.chars().collect();
    let needle: Vec<char> = needle.chars().collect();
    needle.len() <= hay.len() && hay.windows(needle.len()).any(|w| w.iter().zip(&needle).all(|(h, n)| char_matches(*h, *n)))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub text: String,
    pub hit: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageHit {
    pub output_id: String,
    /// 1부터
    pub page_number: usize,
    pub snippet: Vec<Segment>,
    pub matches: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentHits {
    pub document_id: String,
    pub name: String,
    pub session: Option<String>,
    pub kind: String,
    pub name_match: bool,
    pub hits: Vec<PageHit>,
    pub total_pages: usize,
}

/// 원문에서 공백을 무시하고 검색어가 나오는 자리(원문 글자 위치, 끝은 포함 안 함)
pub fn find_spans(text: &str, needle: &str) -> Vec<(usize, usize)> {
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let compact: Vec<(usize, char)> = text
        .chars()
        .enumerate()
        .filter(|(_, c)| !c.is_whitespace())
        .flat_map(|(i, c)| c.to_lowercase().map(move |l| (i, l)))
        .collect();
    let mut spans = Vec::new();
    let mut i = 0;
    while i + needle.len() <= compact.len() {
        if compact[i..i + needle.len()].iter().zip(&needle).all(|((_, h), n)| h == n) {
            spans.push((compact[i].0, compact[i + needle.len() - 1].0 + 1));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    spans
}

/// 첫 번째 자리 앞뒤를 잘라 강조 조각으로 나눈다. 글자 위치를 화면(UTF-16)으로 넘기지 않으려고 조각으로 보낸다
pub fn snippet(text: &str, spans: &[(usize, usize)]) -> Vec<Segment> {
    let chars: Vec<char> = text.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect();
    let Some(&(first_start, _)) = spans.first() else {
        let end = chars.len().min(SNIPPET_CONTEXT * 2);
        return vec![Segment {
            text: chars[..end].iter().collect(),
            hit: false,
        }];
    };
    let from = first_start.saturating_sub(SNIPPET_CONTEXT);
    let to = (first_start + SNIPPET_CONTEXT * 2).min(chars.len());
    let mut segments = Vec::new();
    let mut cursor = from;
    let push = |segments: &mut Vec<Segment>, a: usize, b: usize, hit: bool| {
        if a < b {
            segments.push(Segment {
                text: chars[a..b].iter().collect(),
                hit,
            });
        }
    };
    if from > 0 {
        segments.push(Segment {
            text: "\u{2026}".into(),
            hit: false,
        });
    }
    for &(start, end) in spans.iter().filter(|(s, _)| *s >= from && *s < to) {
        push(&mut segments, cursor, start, false);
        push(&mut segments, start, end.min(to), true);
        cursor = end.min(to);
    }
    push(&mut segments, cursor, to, false);
    if to < chars.len() {
        segments.push(Segment {
            text: "\u{2026}".into(),
            hit: false,
        });
    }
    segments
}

/// 문서 폴더의 파일 수정 시각을 묶은 값. 같으면 다시 색인하지 않는다
fn document_fingerprint(dir: &Path) -> String {
    let mut parts = Vec::new();
    for name in [
        scanshelf_core::MANIFEST_FILE,
        scanshelf_core::overrides::EDITS_FILE,
        scanshelf_core::processing::PROCESSING_FILE,
    ] {
        let modified = std::fs::metadata(dir.join(name))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        parts.push(modified.to_string());
    }
    parts.join("-")
}

/// 문서 하나를 색인에 다시 넣는다. 지문이 같으면 건너뛴다
pub fn index_document(conn: &mut Connection, dir: &Path, force: bool) -> Result<(), String> {
    let (manifest, _) = load_applied(dir)?;
    let id = manifest.id.to_string();
    let fingerprint = document_fingerprint(dir);
    if !force {
        let stored: Option<String> = conn
            .query_row("SELECT fingerprint FROM docs WHERE id = ?1", [&id], |r| r.get(0))
            .ok();
        if stored.as_deref() == Some(fingerprint.as_str()) {
            return Ok(());
        }
    }
    let processing = load_processing(dir);
    let kind = serde_json::to_value(manifest.kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM pages WHERE doc_id = ?1", [&id]).map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT OR REPLACE INTO docs(id, name, session, kind, created_at, fingerprint) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, manifest.name, manifest.session, kind, manifest.created_at.to_rfc3339(), fingerprint],
    )
    .map_err(|e| e.to_string())?;
    for (index, output) in processing.outputs.iter().enumerate() {
        let Some(ocr_path) = &output.ocr else { continue };
        let Ok(Some(page)) = read_json::<scanshelf_core::ocr::OcrPage>(&dir.join(ocr_path)) else {
            continue;
        };
        tx.execute(
            "INSERT INTO pages(doc_id, output_id, page_no, text, compact) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, output.id, (index + 1) as i64, page.text, compact(&page.text)],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

pub fn remove_document(conn: &Connection, document_id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM pages WHERE doc_id = ?1", [document_id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM docs WHERE id = ?1", [document_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// 이름, 세션이 맞는 문서를 먼저, 그다음 본문이 맞는 문서를 최신순으로
pub fn search(conn: &Connection, query: &str) -> Result<Vec<DocumentHits>, String> {
    let needle = compact(query);
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let mut docs: Vec<DocumentHits> = Vec::new();
    {
        let mut stmt = conn
            .prepare("SELECT id, name, session, kind FROM docs ORDER BY created_at DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?))
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            let (id, name, session, kind) = row;
            let name_match = matches_with_initials(&compact(&name), &needle)
                || session.as_deref().is_some_and(|s| matches_with_initials(&compact(s), &needle));
            docs.push(DocumentHits {
                document_id: id,
                name,
                session,
                kind,
                name_match,
                hits: Vec::new(),
                total_pages: 0,
            });
        }
    }

    // trigram은 세 글자 이상이어야 색인을 탄다. 그보다 짧으면 LIKE로 훑는다(페이지 수가 수만 장이 아니면 충분히 빠르다)
    let (sql, arg) = if needle.chars().count() >= 3 {
        (
            "SELECT doc_id, output_id, page_no, text FROM pages WHERE compact MATCH ?1 ORDER BY doc_id, page_no",
            format!("\"{}\"", needle.replace('"', "\"\"")),
        )
    } else {
        (
            "SELECT doc_id, output_id, page_no, text FROM pages WHERE compact LIKE ?1 ESCAPE '\\' ORDER BY doc_id, page_no",
            format!("%{}%", escape_like(&needle)),
        )
    };
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([arg], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?))
        })
        .map_err(|e| e.to_string())?;
    for row in rows.flatten() {
        let (doc_id, output_id, page_no, text) = row;
        let Some(doc) = docs.iter_mut().find(|d| d.document_id == doc_id) else {
            continue;
        };
        doc.total_pages += 1;
        if doc.hits.len() < HITS_PER_DOCUMENT {
            let spans = find_spans(&text, &needle);
            doc.hits.push(PageHit {
                output_id,
                page_number: page_no as usize,
                snippet: snippet(&text, &spans),
                matches: spans.len(),
            });
        }
    }
    docs.retain(|d| d.name_match || !d.hits.is_empty());
    // 안정 정렬이라 같은 무리 안에서는 최신순이 유지된다
    docs.sort_by_key(|d| !d.name_match);
    Ok(docs)
}

#[tauri::command]
pub fn search_library(app: tauri::AppHandle, query: String) -> Result<Vec<DocumentHits>, String> {
    let index = app.state::<SearchIndex>();
    let conn = index.lock();
    search(&conn, &query)
}

/// 처리가 끝났거나 이름을 바꾼 문서를 다시 색인한다. 실패해도 화면 동작은 막지 않는다
pub fn refresh_document(app: &tauri::AppHandle, document_id: &str) {
    let Some(index) = app.try_state::<SearchIndex>() else {
        return;
    };
    let Ok(dir) = crate::library::document_dir(app, document_id) else {
        let _ = remove_document(&index.lock(), document_id);
        return;
    };
    if let Err(error) = index_document(&mut index.lock(), &dir, false) {
        eprintln!("검색 색인 실패 ({document_id}): {error}");
    }
}

/// 앱을 켤 때 보관함 전체를 맞춘다. 사라진 문서는 색인에서 뺀다
pub fn refresh_all(app: &tauri::AppHandle) {
    let (Some(index), Ok(library)) = (app.try_state::<SearchIndex>(), crate::library::library_dir(app)) else {
        return;
    };
    let mut present = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&library) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.join(scanshelf_core::MANIFEST_FILE).exists() {
                continue;
            }
            if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
                present.push(name.to_string());
            }
            if let Err(error) = index_document(&mut index.lock(), &dir, false) {
                eprintln!("검색 색인 실패 ({}): {error}", dir.display());
            }
        }
    }
    let conn = index.lock();
    let known: Vec<String> = conn
        .prepare("SELECT id FROM docs")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .unwrap_or_default();
    for id in known.iter().filter(|id| !present.contains(id)) {
        let _ = remove_document(&conn, id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scanshelf_core::ocr::{OcrPage, Rect};
    use scanshelf_core::processing::{OutputPage, PIPELINE_VERSION, PagePart, ProcessOptions, Processing};

    fn write_document(root: &Path, name: &str, session: &str, pages: &[&str]) -> std::path::PathBuf {
        let id = uuid::Uuid::new_v4();
        let dir = root.join(id.to_string());
        std::fs::create_dir_all(dir.join("ocr")).unwrap();
        let mut manifest_pages = Vec::new();
        let mut outputs = Vec::new();
        for text in pages {
            let page_id = uuid::Uuid::new_v4();
            manifest_pages.push(serde_json::json!({
                "id": page_id, "raw": format!("raw/{page_id}.heic"),
                "capturedAt": "2026-11-05T01:20:03.412Z", "layout": "single"
            }));
            let ocr = OcrPage {
                engine: "test".into(),
                image_width: 100,
                image_height: 100,
                text: text.to_string(),
                blocks: vec![],
            };
            let _ = Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 };
            crate::library::write_json(&dir.join(format!("ocr/{page_id}.json")), &ocr).unwrap();
            outputs.push(OutputPage {
                id: page_id.to_string(),
                source_page: page_id,
                part: PagePart::Whole,
                image: format!("pages/{page_id}.jpg"),
                ocr: Some(format!("ocr/{page_id}.json")),
                options: ProcessOptions::default(),
                pipeline_version: PIPELINE_VERSION,
                processed_at: chrono::Utc::now(),
            });
        }
        let manifest = serde_json::json!({
            "schemaVersion": 1, "id": id, "name": name, "kind": "pamphlet", "session": session,
            "createdAt": "2026-11-05T01:20:00Z", "pages": manifest_pages
        });
        std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
        crate::library::write_json(
            &dir.join("processing.json"),
            &Processing {
                defaults: ProcessOptions::default(),
                outputs,
            },
        )
        .unwrap();
        dir
    }

    fn index_with(docs: &[(&str, &str, &[&str])]) -> (tempfile::TempDir, Connection) {
        let root = tempfile::tempdir().unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        for (name, session, pages) in docs {
            let dir = write_document(root.path(), name, session, pages);
            index_document(&mut conn, &dir, false).unwrap();
        }
        (root, conn)
    }

    #[test]
    fn 한국어_부분_문자열을_찾는다() {
        let (_root, conn) = index_with(&[(
            "예시로보틱스",
            "2026 예시박람회",
            &["데이터센터 구축용 자율 주행 로봇", "두 번째 페이지 내용"],
        )]);
        let results = search(&conn, "센터 구축").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].hits.len(), 1);
        assert_eq!(results[0].hits[0].page_number, 1);
        let hit: Vec<_> = results[0].hits[0].snippet.iter().filter(|s| s.hit).map(|s| s.text.as_str()).collect();
        assert_eq!(hit, vec!["센터 구축"]);
    }

    #[test]
    fn 두_글자_검색어도_찾는다() {
        let (_root, conn) = index_with(&[("문서", "", &["로봇 팔 사양"])]);
        let results = search(&conn, "로봇").unwrap();
        assert_eq!(results[0].hits[0].matches, 1);
    }

    #[test]
    fn 영어는_대소문자를_가리지_않는다() {
        let (_root, conn) = index_with(&[("문서", "", &["Autonomous Mobile Robot"])]);
        assert_eq!(search(&conn, "mobile robot").unwrap().len(), 1);
    }

    #[test]
    fn 문서_이름은_초성으로_찾는다() {
        let (_root, conn) = index_with(&[("한빛로보틱스", "2026 데이터센터코리아", &["본문"]), ("다른회사", "", &["본문"])]);
        let results = search(&conn, "ㅎㅂㄹ").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].name_match);
        assert_eq!(results[0].name, "한빛로보틱스");
        assert_eq!(search(&conn, "ㄷㅇㅌㅅㅌ").unwrap().len(), 1, "세션 이름도 초성으로");
    }

    #[test]
    fn 다시_색인하면_옛_페이지가_남지_않는다() {
        let root = tempfile::tempdir().unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let dir = write_document(root.path(), "문서", "", &["사과 바나나"]);
        index_document(&mut conn, &dir, false).unwrap();
        index_document(&mut conn, &dir, true).unwrap();
        let count: i64 = conn.query_row("SELECT count(*) FROM pages", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn 공백을_무시하고_원문_위치를_찾는다() {
        assert_eq!(find_spans("데이터 센터와 데이터센터", "데이터센터"), vec![(0, 6), (8, 13)]);
    }

    #[test]
    fn 초성과_글자를_섞어_찾는다() {
        assert!(matches_with_initials("한빛로보틱스", "한ㅂ"));
        assert!(!matches_with_initials("한빛로보틱스", "ㅂㅎ"));
    }
}

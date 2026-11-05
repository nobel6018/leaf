//! 문서 태그. `/`로 계층을 만들어 폴더처럼 모아 본다(분야/냉각, 박람회/2026 DCK, 책/경제). sori tags.rs를 그대로 가져왔다.
//! Mac 화면, AI 정리, MCP가 같은 규칙으로 정리하도록 여기에 둔다.
//! @see docs/organize.md

/// 문서 하나에 붙일 수 있는 태그 수
pub const MAX_TAGS: usize = 10;
/// 태그 하나의 최대 글자 수
const MAX_LEN: usize = 60;

/// 태그 하나를 정리한다. 앞뒤 공백, `/` 앞뒤 공백, 빈 단계(업무//영업), `#` 머리를 없앤다. 남는 게 없으면 None
pub fn normalize_tag(raw: &str) -> Option<String> {
    let parts: Vec<String> = raw
        .trim()
        .trim_start_matches('#')
        .split('/')
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }
    let tag: String = parts.join("/").chars().take(MAX_LEN).collect();
    Some(tag.trim_end_matches('/').to_string())
}

/// 태그 목록을 정리한다. 순서는 지키고 같은 태그(대소문자 무시)는 처음 것만 남긴다
pub fn normalize_tags<I, S>(raw: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: Vec<String> = Vec::new();
    for tag in raw.into_iter().filter_map(|t| normalize_tag(t.as_ref())) {
        if !out.iter().any(|t| t.to_lowercase() == tag.to_lowercase()) {
            out.push(tag);
        }
        if out.len() == MAX_TAGS {
            break;
        }
    }
    out
}

/// 태그 필터에 드는지. 하위 태그(업무/영업/한빛)가 붙은 문서도 상위(업무/영업)에 든다. 화면(src/tags.ts)도 같은 규칙을 쓴다
pub fn matches(tags: &[String], filter: &str) -> bool {
    tags.iter().any(|t| t == filter || t.starts_with(&format!("{filter}/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 하위_태그도_상위_필터에_든다() {
        let tags = vec!["업무/영업/한빛".to_string()];
        assert!(matches(&tags, "업무"));
        assert!(matches(&tags, "업무/영업"));
        assert!(!matches(&tags, "업무/영"));
    }

    #[test]
    fn 공백과_빈_단계와_샵을_정리한다() {
        assert_eq!(normalize_tag("  #업무 / 영업//한빛  로보틱스/ ").as_deref(), Some("업무/영업/한빛 로보틱스"));
        assert_eq!(normalize_tag(" / "), None);
    }

    #[test]
    fn 같은_태그는_처음_것만_남기고_순서를_지킨다() {
        let tags = normalize_tags(["회의", "Work/A", "work/a", "", "개인"]);
        assert_eq!(tags, vec!["회의", "Work/A", "개인"]);
    }

    #[test]
    fn 태그는_10개까지만_남긴다() {
        let raw: Vec<String> = (0..15).map(|i| format!("t{i}")).collect();
        assert_eq!(normalize_tags(&raw).len(), MAX_TAGS);
    }
}

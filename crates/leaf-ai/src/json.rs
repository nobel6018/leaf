//! 모델 응답에서 JSON 객체 꺼내기. 지시해도 모델이 코드 블록이나 앞뒤 설명을 붙이는 일이 있다.
//!
//! @see docs/ai.md

/// 응답 글자에서 처음으로 완결된 `{...}` 객체를 찾는다. 문자열 안의 중괄호와 이스케이프는 건너뛴다
pub fn extract_json_object(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut search_from = 0;
    while let Some(offset) = text[search_from..].find('{') {
        let start = search_from + offset;
        if let Some(end) = matching_brace(bytes, start) {
            let candidate = &text[start..=end];
            if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                return Some(candidate);
            }
        }
        search_from = start + 1;
    }
    None
}

fn matching_brace(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 코드_블록과_설명을_걷어낸다() {
        let text = "정리했습니다.\n```json\n{\"a\": \"}{\", \"b\": {\"c\": 1}}\n```\n끝";
        assert_eq!(
            extract_json_object(text),
            Some("{\"a\": \"}{\", \"b\": {\"c\": 1}}")
        );
    }

    #[test]
    fn 깨진_객체는_건너뛰고_다음_객체를_찾는다() {
        let text = "{설명} {\"a\": \"\\\"x\"}";
        assert_eq!(extract_json_object(text), Some("{\"a\": \"\\\"x\"}"));
    }

    #[test]
    fn 객체가_없으면_none() {
        assert_eq!(extract_json_object("죄송합니다"), None);
        assert_eq!(extract_json_object("{\"a\": 1"), None);
    }
}

//! 책 쪽 번호 확인. 삼각대로 300쪽 책을 찍다 보면 한 장을 건너뛰거나(두 장을 한 번에 넘김) 같은 쪽을 두 번 찍는다.
//! OCR 결과(`ocr/<출력 id>.json`)에서 위아래 가장자리의 짧은 숫자를 쪽 번호로 읽고,
//! 빠진 번호, 겹친 번호(두 번 찍음), 순서가 어긋난 페이지를 알려 준다.
//!
//! 순서 바꾸기는 사용자가 "쪽 번호 순서로 정렬"을 누를 때만 `edits.json`의 `pageOrder`에 쓴다.
//! `pageOrder`는 원본 페이지 id 단위라, 펼친 책(왼쪽, 오른쪽 출력)이나 3단 접지(세 면)는 원본 한 장이 한 덩어리로 움직인다.
//!
//! 순수 함수(`analyze`, `plan_order`)와 Tauri 명령(`book_page_report`, `sort_by_page_numbers`)으로 나눴다.
//!
//! @see docs/book.md

use std::collections::{BTreeMap, HashMap};

use scanshelf_core::ocr::OcrPage;
use scanshelf_core::overrides::Edits;
use scanshelf_core::processing::PagePart;
use serde::Serialize;
use uuid::Uuid;

use crate::library::{document_dir, load_applied, load_processing, read_json};
use crate::pipeline::Pipeline;

/// 줄 가운데가 이 안쪽이면 위 가장자리, `1 - EDGE_ZONE` 밖이면 아래 가장자리
const EDGE_ZONE: f64 = 0.12;
/// 쪽 번호만 있는 줄의 최대 길이("- 157 -", "p. 57" 정도)
const MAX_PLAIN_LEN: usize = 14;
/// 머리글 줄("57 | 3장 제목")의 최대 길이
const MAX_HEADER_LEN: usize = 60;
/// 이웃과 맞는지 볼 때 몇 장 앞뒤까지 보나
const NEIGHBOR_WINDOW: usize = 3;
/// 화면에 늘어놓을 빠진 번호 개수 상한(구간 하나당)
const MAX_LISTED_MISSING: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NumberKind {
    /// 앞쪽(머리말, 차례)의 i, ii, iii. 본문 아라비아 숫자보다 앞에 온다
    Roman,
    Arabic,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedNumber {
    pub value: u32,
    pub kind: NumberKind,
    /// 화면 표시: `57`, `xii`. 두 쪽이 한 장에 찍혔으면 `56-57`
    pub label: String,
    /// 이 출력 페이지가 덮는 쪽 수. 펼친 책을 나누지 못해 한 장에 두 번호가 있으면 2
    pub span: u32,
    /// 0~1. OCR 신뢰도, 줄 모양, 앞뒤 번호와 맞는 정도를 곱한 값
    pub confidence: f32,
    /// OCR이 잘못 읽은 글자를 앞뒤 순서로 고쳤으면 true(예: `5B`를 58로)
    pub corrected: bool,
    /// OCR이 읽은 원래 글자
    pub raw: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputNumber {
    pub output_id: String,
    pub source_page: Uuid,
    pub part: PagePart,
    pub has_ocr: bool,
    pub number: Option<DetectedNumber>,
    /// 같은 번호가 앞에 이미 있다(두 번 찍음)
    pub duplicate: bool,
    /// 쪽 번호 순서에서 벗어난 자리에 있다
    pub out_of_order: bool,
}

/// 쪽 번호 사이가 비었다. `unnumbered_between`은 그 사이에 번호를 못 읽은 페이지 수(장 첫 페이지, 그림 페이지는 번호가 없곤 하다)
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gap {
    pub kind: NumberKind,
    pub after: u32,
    pub before: u32,
    pub missing_count: u32,
    /// 앞쪽부터 최대 `MAX_LISTED_MISSING`개
    pub missing: Vec<u32>,
    pub unnumbered_between: usize,
    /// 번호 없는 페이지 수가 빈 번호 수 이상이면, 빠진 게 아니라 번호를 못 읽은 것일 수 있다
    pub likely_unnumbered: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Duplicate {
    pub kind: NumberKind,
    pub value: u32,
    pub output_ids: Vec<String>,
    /// 지울 원본 페이지를 고르기 쉽게 원본 id도 준다(겹치지 않게, 문서 순서)
    pub source_pages: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookReport {
    /// 문서 순서대로
    pub outputs: Vec<OutputNumber>,
    pub detected: usize,
    pub total: usize,
    pub first: Option<u32>,
    pub last: Option<u32>,
    /// 번호 없는 페이지로 설명되지 않는 빈 번호(구간마다 최대 50개)
    pub missing: Vec<u32>,
    pub gaps: Vec<Gap>,
    pub duplicates: Vec<Duplicate>,
    /// 순서가 어긋난 출력 페이지 id
    pub out_of_order: Vec<String>,
    /// 쪽 번호 순서로 정렬하면 자리가 바뀌는 원본 페이지 수. 0이면 이미 정렬돼 있다
    pub sort_moves: usize,
}

/// 분석 입력. 문서 순서대로 준다
#[derive(Debug, Clone)]
pub struct PageInput {
    pub output_id: String,
    pub source_page: Uuid,
    pub part: PagePart,
    pub ocr: Option<OcrPage>,
}

/// 한 줄에서 읽은 쪽 번호 후보
#[derive(Debug, Clone, PartialEq)]
struct Candidate {
    value: u32,
    kind: NumberKind,
    span: u32,
    confidence: f32,
    /// 글자 보정을 거쳤다(O → 0 같은)
    fixed: bool,
    raw: String,
    /// 줄 가운데 x(0~1)
    x: f64,
}

// --- 줄 하나 읽기 ---

/// 쪽 번호 앞뒤에 붙는 꾸밈. em dash와 가운뎃점은 코드 규칙상 이스케이프로 쓴다
const DECORATIONS: &[char] = &[
    '-', '\u{2013}', '\u{2014}', '\u{2212}', '|', '\u{00B7}', '\u{2022}', '.', ',', '[', ']', '(', ')', '<', '>', '\u{00AB}', '\u{00BB}', '~', '_',
    '=', '*', '/', '\\', ':', '\'', '"', '`',
];

/// 글자 하나의 OCR 혼동. 숫자 사이에서만 쓴다
fn confusable_digit(c: char) -> Option<char> {
    match c {
        'O' | 'o' | 'D' | 'Q' => Some('0'),
        'l' | 'I' | 'i' | '!' | '|' => Some('1'),
        'Z' | 'z' => Some('2'),
        'S' | 's' => Some('5'),
        'G' | 'b' => Some('6'),
        'T' => Some('7'),
        'B' => Some('8'),
        'g' | 'q' => Some('9'),
        _ => None,
    }
}

fn to_ascii_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            // 전각 숫자
            '\u{FF10}'..='\u{FF19}' => char::from_u32(c as u32 - 0xFF10 + '0' as u32).unwrap_or(c),
            _ => c,
        })
        .collect()
}

fn strip_affixes(s: &str) -> &str {
    let mut t = s.trim();
    loop {
        let before = t;
        t = t.trim_matches(|c: char| c.is_whitespace() || DECORATIONS.contains(&c));
        for prefix in ["pp", "p", "P", "page", "Page", "PAGE"] {
            if let Some(rest) = t.strip_prefix(prefix)
                && rest.starts_with(|c: char| c == '.' || c.is_whitespace() || c.is_ascii_digit())
            {
                t = rest;
            }
        }
        for suffix in ["쪽", "페이지", "면", "p", "P"] {
            if let Some(rest) = t.strip_suffix(suffix)
                && rest.ends_with(|c: char| c.is_ascii_digit() || c.is_whitespace())
            {
                t = rest;
            }
        }
        if t == before {
            return t;
        }
    }
}

fn parse_digits(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 4 || !s.chars().all(|c| c.is_ascii_digit()) || s.starts_with('0') {
        return None;
    }
    s.parse().ok().filter(|v| *v > 0)
}

/// i, ii, ..., xl(40)까지. 대소문자가 섞이면 거부. 낱자 l(50)은 숫자 1을 잘못 읽은 경우가 많아 받지 않는다
fn parse_roman(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 8 {
        return None;
    }
    let lower = s.chars().all(|c| "ivxl".contains(c));
    let upper = s.chars().all(|c| "IVXL".contains(c));
    if !lower && !upper {
        return None;
    }
    let value_of = |c: char| match c.to_ascii_lowercase() {
        'i' => 1,
        'v' => 5,
        'x' => 10,
        'l' => 50,
        _ => 0,
    };
    let chars: Vec<u32> = s.chars().map(value_of).collect();
    let mut total = 0i64;
    for (i, &v) in chars.iter().enumerate() {
        if chars.get(i + 1).is_some_and(|&next| next > v) {
            total -= v as i64;
        } else {
            total += v as i64;
        }
    }
    let total = u32::try_from(total).ok().filter(|v| (1..=40).contains(v))?;
    // "iiii", "vx" 같은 엉터리는 다시 써 보면 다르다
    (to_roman(total) == s.to_ascii_lowercase()).then_some(total)
}

fn to_roman(mut n: u32) -> String {
    const TABLE: [(u32, &str); 7] = [(50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
    let mut out = String::new();
    for (v, s) in TABLE {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

/// 줄 글자에서 쪽 번호를 읽는다. (값, 종류, 모양 점수, 글자 보정 여부)
fn parse_line(text: &str) -> Option<(u32, NumberKind, f32, bool)> {
    let text = to_ascii_digits(text.trim());
    if text.is_empty() || text.chars().count() > MAX_HEADER_LEN {
        return None;
    }
    let core = strip_affixes(&text);
    if text.chars().count() <= MAX_PLAIN_LEN {
        if let Some(v) = parse_digits(core) {
            return Some((v, NumberKind::Arabic, 1.0, false));
        }
        // 숫자 사이에 혼동 글자가 낀 경우(5B, l2, 1O3). 진짜 숫자가 하나 이상 있어야 한다
        let has_digit = core.chars().any(|c| c.is_ascii_digit());
        if has_digit && core.chars().count() <= 4 {
            let mapped: Option<String> = core
                .chars()
                .map(|c| if c.is_ascii_digit() { Some(c) } else { confusable_digit(c) })
                .collect();
            if let Some(v) = mapped.as_deref().and_then(parse_digits) {
                return Some((v, NumberKind::Arabic, 0.7, true));
            }
        }
        if let Some(v) = parse_roman(core) {
            // 한 글자(I, V, X)는 머리글 낱자일 수 있어 낮게 본다. 대문자도 조금 낮게
            let len_factor = if core.len() == 1 { 0.55 } else { 0.8 };
            let case_factor = if core.chars().all(|c| c.is_ascii_lowercase()) { 1.0 } else { 0.85 };
            return Some((v, NumberKind::Roman, len_factor * case_factor, false));
        }
    }
    // 머리글: "57 | 3장 제목", "제목 | 57", "57  제목". 맨 앞이나 맨 뒤 낱말이 숫자
    let tokens: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == '|' || c == '\u{00B7}' || c == '\u{2022}')
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.len() >= 2 {
        let has_words = |rest: &[&str]| rest.iter().any(|t| t.chars().any(char::is_alphabetic));
        let first = parse_digits(strip_affixes(tokens[0]));
        let last = parse_digits(strip_affixes(tokens[tokens.len() - 1]));
        match (first, last) {
            (Some(v), None) if has_words(&tokens[1..]) => return Some((v, NumberKind::Arabic, 0.6, false)),
            (None, Some(v)) if has_words(&tokens[..tokens.len() - 1]) => return Some((v, NumberKind::Arabic, 0.6, false)),
            _ => {}
        }
    }
    None
}

/// OCR 한 페이지에서 쪽 번호 후보. 위아래 가장자리 줄만 본다
fn candidates(ocr: &OcrPage) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = ocr
        .blocks
        .iter()
        .flat_map(|b| b.lines.iter())
        .filter_map(|line| {
            let cy = line.bbox.y + line.bbox.height / 2.0;
            if cy > EDGE_ZONE && cy < 1.0 - EDGE_ZONE {
                return None;
            }
            let (value, kind, shape, fixed) = parse_line(&line.text)?;
            // 가장자리에 가까울수록 쪽 번호답다
            let edge = cy.min(1.0 - cy);
            let edge_factor = (1.0 - edge / EDGE_ZONE * 0.3) as f32;
            Some(Candidate {
                value,
                kind,
                span: 1,
                confidence: (line.confidence.clamp(0.0, 1.0) * shape * edge_factor).clamp(0.0, 1.0),
                fixed,
                raw: line.text.trim().to_string(),
                x: line.bbox.x + line.bbox.width / 2.0,
            })
        })
        .collect();
    // 펼친 책을 나누지 못한 한 장: 왼쪽 반에 n, 오른쪽 반에 n+1이 있으면 두 쪽을 덮는 한 후보로 합친다
    let mut merged = Vec::new();
    for a in out.iter().filter(|c| c.kind == NumberKind::Arabic && c.x < 0.5) {
        if let Some(b) = out.iter().find(|b| b.kind == NumberKind::Arabic && b.x >= 0.5 && b.value == a.value + 1) {
            merged.push(Candidate {
                span: 2,
                confidence: a.confidence.min(b.confidence),
                raw: format!("{} / {}", a.raw, b.raw),
                fixed: a.fixed || b.fixed,
                ..a.clone()
            });
        }
    }
    out.extend(merged);
    out
}

// --- 문서 전체 ---

/// 출력 페이지가 원본 안에서 몇 번째 쪽인지. 펼친 책 오른쪽은 왼쪽보다 1, 3단 접지 k면은 k만큼 뒤
fn part_offset(part: PagePart) -> u32 {
    match part {
        PagePart::Whole | PagePart::Left => 0,
        PagePart::Right => 1,
        PagePart::Panel(k) => k as u32,
    }
}

fn label(kind: NumberKind, value: u32, span: u32) -> String {
    let one = |v: u32| match kind {
        NumberKind::Arabic => v.to_string(),
        NumberKind::Roman => to_roman(v),
    };
    if span > 1 { format!("{}-{}", one(value), one(value + span - 1)) } else { one(value) }
}

/// 숫자 문자열 두 개가 글자 하나 차이(바뀜, 빠짐, 더함) 이내인가
fn similar_digits(a: u32, b: u32) -> bool {
    let (a, b) = (a.to_string(), b.to_string());
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len() == b.len() {
        return a.iter().zip(&b).filter(|(x, y)| x != y).count() <= 1;
    }
    let (short, long) = if a.len() < b.len() { (&a, &b) } else { (&b, &a) };
    if long.len() - short.len() != 1 {
        return false;
    }
    (0..long.len()).any(|skip| {
        let rest: Vec<char> = long.iter().enumerate().filter(|(i, _)| *i != skip).map(|(_, c)| *c).collect();
        &rest == short
    })
}

/// j번째 출력의 번호 o와 i번째 출력의 번호 c가 이어지는가. 출력 거리만큼(번호 없는 페이지를 건너뛰면 그보다 적게) 번호가 는다
fn links(o: &Candidate, j: usize, c: &Candidate, i: usize) -> bool {
    if o.kind != c.kind || i == j {
        return false;
    }
    let (early, late, distance) = if j < i { (o, c, i - j) } else { (c, o, j - i) };
    let advance = late.value as i64 - (early.value + early.span - 1) as i64;
    advance >= 1 && advance <= distance as i64 + 1
}

fn window(i: usize, len: usize) -> std::ops::Range<usize> {
    i.saturating_sub(NEIGHBOR_WINDOW)..(i + NEIGHBOR_WINDOW + 1).min(len)
}

/// (앞뒤 `NEIGHBOR_WINDOW`장 안에 같은 종류 번호가 있나, 그중 하나와 이어지나)
fn consistent_with(chosen: &[Option<Candidate>], i: usize, c: &Candidate) -> (bool, bool) {
    let neighbors: Vec<(usize, &Candidate)> = window(i, chosen.len())
        .filter(|&j| j != i)
        .filter_map(|j| chosen[j].as_ref().filter(|o| o.kind == c.kind).map(|o| (j, o)))
        .collect();
    (!neighbors.is_empty(), neighbors.iter().any(|(j, o)| links(o, *j, c, i)))
}

/// 앞뒤 `NEIGHBOR_WINDOW`장의 후보 중 하나라도 c와 이어지는가
fn links_any(all: &[Vec<Candidate>], i: usize, c: &Candidate) -> bool {
    window(i, all.len()).any(|j| all[j].iter().any(|o| links(o, j, c, i)))
}

/// 앞뒤 가장 가까운 번호(각각 `NEIGHBOR_WINDOW`장 안)가 사이 출력 수와 꼭 맞게 이어지면 i 자리의 값.
/// 예: 56, ?, 58 → 57. 56, (번호 없음), ?, 59 → 58. 사이 출력은 한 쪽씩 덮는다고 본다
fn expected_between(chosen: &[Option<Candidate>], i: usize) -> Option<(NumberKind, u32)> {
    let pj = (i.saturating_sub(NEIGHBOR_WINDOW)..i).rev().find(|&j| chosen[j].is_some())?;
    let nj = (i + 1..(i + NEIGHBOR_WINDOW + 1).min(chosen.len())).find(|&j| chosen[j].is_some())?;
    let (prev, next) = (chosen[pj].as_ref()?, chosen[nj].as_ref()?);
    let e = prev.value + prev.span + (i - pj - 1) as u32;
    let exact = next.value == prev.value + prev.span + (nj - pj - 1) as u32;
    (prev.kind == next.kind && exact).then_some((prev.kind, e))
}

/// 문서 순서대로 받은 출력 페이지의 쪽 번호를 읽고 검사한다
pub fn analyze(pages: &[PageInput]) -> BookReport {
    let all: Vec<Vec<Candidate>> = pages.iter().map(|p| p.ocr.as_ref().map(candidates).unwrap_or_default()).collect();

    // 페이지마다 가장 믿을 만한 후보. 앞뒤 페이지 후보 중 하나와 이어지면 더 믿는다(머리글의 장 번호 대신 쪽 번호).
    // 두 쪽을 덮는 후보는 같은 줄에서 나온 낱개 후보보다 조금 앞세운다
    let mut chosen: Vec<Option<Candidate>> = all
        .iter()
        .enumerate()
        .map(|(i, cands)| {
            let score = |c: &Candidate| {
                let linked = if cands.len() > 1 && links_any(&all, i, c) { 0.5 } else { 0.0 };
                c.confidence + linked + if c.span > 1 { 0.02 } else { 0.0 }
            };
            cands.iter().max_by(|a, b| score(a).total_cmp(&score(b))).cloned()
        })
        .collect();

    // 3차: OCR이 잘못 읽은 번호를 앞뒤 순서로 고치고, 엉뚱한 숫자는 버린다
    let mut corrected = vec![false; pages.len()];
    let mut factor = vec![1.0f32; pages.len()];
    for i in 0..pages.len() {
        if let Some((kind, e)) = expected_between(&chosen, i) {
            let current = chosen[i].clone();
            match current {
                Some(c) if c.kind == kind && c.value == e => {}
                Some(c) if c.kind == kind && (c.fixed || similar_digits(c.value, e)) => {
                    chosen[i] = Some(Candidate { value: e, span: 1, ..c });
                    corrected[i] = true;
                }
                // 앞뒤가 딱 한 자리를 남겼는데 전혀 다른 숫자: 쪽 번호가 아닌 숫자(표 번호, 연도).
                // 같은 페이지의 다른 후보가 맞으면 그것을 쓰고, 없으면 못 읽은 것으로 둔다(번호를 지어내지 않는다)
                Some(c) if c.kind == kind => {
                    chosen[i] = all[i].iter().find(|o| o.kind == kind && o.value == e).cloned();
                }
                _ => {}
            }
        }
    }
    // 원본 한 장 안: 펼친 책 왼쪽 n이면 오른쪽은 n+1, 3단 접지 k면은 n+k다. 같은 장 출력끼리 정렬 값(번호 - 자리)이
    // 다르면, 장 밖 이웃과 이어지는 쪽을 믿고 나머지를 고친다(글자 하나 차이일 때만)
    let mut by_raw: Vec<(Uuid, Vec<usize>)> = Vec::new();
    for (i, p) in pages.iter().enumerate() {
        match by_raw.last_mut() {
            Some((id, idx)) if *id == p.source_page => idx.push(i),
            _ => by_raw.push((p.source_page, vec![i])),
        }
    }
    for (_, idx) in &by_raw {
        let numbered: Vec<usize> = idx.iter().copied().filter(|&i| chosen[i].as_ref().is_some_and(|c| c.span == 1)).collect();
        if numbered.len() < 2 {
            continue;
        }
        let key = |i: usize| chosen[i].as_ref().map(|c| (c.kind, c.value as i64 - part_offset(pages[i].part) as i64));
        // 장 밖 이웃과 이어지는지는 같은 장 출력을 빼고 본다
        let outside: Vec<Option<Candidate>> =
            chosen.iter().enumerate().map(|(j, c)| if idx.contains(&j) { None } else { c.clone() }).collect();
        let mut support: HashMap<SortKey, f32> = HashMap::new();
        for &i in &numbered {
            let c = chosen[i].as_ref().expect("번호가 있는 출력");
            // 장 밖 이웃 중 자리와 번호가 꼭 맞게 이어지는 수
            let exact = window(i, outside.len())
                .filter(|&j| {
                    outside[j].as_ref().is_some_and(|o| {
                        let (early, late, d) = if j < i { (o, c, i - j) } else { (c, o, j - i) };
                        o.kind == c.kind && late.value as i64 - (early.value + early.span - 1) as i64 == d as i64
                    })
                })
                .count();
            *support.entry(key(i).expect("번호가 있다")).or_default() += 1.0 + exact as f32;
        }
        let mut ranked: Vec<(SortKey, f32)> = support.into_iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        if ranked.len() < 2 || ranked[0].1 <= ranked[1].1 {
            continue;
        }
        let (kind, best) = ranked[0].0;
        for &i in &numbered {
            let c = chosen[i].clone().expect("번호가 있다");
            let want = best + part_offset(pages[i].part) as i64;
            if c.kind == kind && c.value as i64 != want && want > 0 && (c.fixed || similar_digits(c.value, want as u32)) {
                chosen[i] = Some(Candidate { value: want as u32, ..c });
                corrected[i] = true;
            }
        }
    }

    // 바로 옆이 빠진 쪽이나 겹친 장이라 자리가 꼭 맞지 않을 때: 앞뒤 어느 쪽과도 안 이어지는 번호가
    // 한쪽에서 기대한 값과 글자 하나 차이이고 그 값이 다른 쪽과도 어긋나지 않으면 고친다. 양쪽 이웃이 다 있어야 한다
    for i in 0..pages.len() {
        let Some(c) = chosen[i].clone() else { continue };
        if corrected[i] || consistent_with(&chosen, i, &c).1 {
            continue;
        }
        let prev = (i.saturating_sub(NEIGHBOR_WINDOW)..i).rev().find_map(|j| chosen[j].clone().filter(|p| p.kind == c.kind).map(|p| (j, p)));
        let next = (i + 1..(i + NEIGHBOR_WINDOW + 1).min(pages.len()))
            .find_map(|j| chosen[j].clone().filter(|n| n.kind == c.kind).map(|n| (j, n)));
        let (Some((pj, p)), Some((nj, n))) = (prev, next) else { continue };
        let (lower, upper) = (p.value + p.span - 1, n.value);
        let mut options: Vec<u32> = vec![p.value + p.span + (i - pj - 1) as u32];
        options.extend(n.value.checked_sub((nj - i) as u32));
        options.retain(|&e| e > lower && e < upper && (c.fixed || similar_digits(c.value, e)));
        options.dedup();
        if let [e] = options[..] {
            chosen[i] = Some(Candidate { value: e, span: 1, ..c });
            corrected[i] = true;
        }
    }
    for i in 0..pages.len() {
        let Some(c) = chosen[i].clone() else { continue };
        let (has_neighbor, agrees) = consistent_with(&chosen, i, &c);
        factor[i] = match (has_neighbor, agrees) {
            (_, true) => 1.0,
            (false, _) => 0.8,
            (true, false) => 0.6,
        };
        // 앞뒤가 서로 맞는데 혼자 동떨어진 숫자는 쪽 번호가 아니라고 본다(나중에 따로 찍어 끝에 붙인 페이지는 앞뒤가 서로 안 맞아 남는다)
        let prev = (0..i).rev().find_map(|j| chosen[j].clone().map(|p| (j, p)));
        let next = (i + 1..pages.len()).find_map(|j| chosen[j].clone().map(|n| (j, n)));
        if let (Some((pj, p)), Some((nj, n))) = (prev, next)
            && !agrees
            && p.kind == c.kind
            && n.kind == c.kind
            && i - pj <= NEIGHBOR_WINDOW
            && nj - i <= NEIGHBOR_WINDOW
        {
            let room = n.value as i64 - (p.value + p.span) as i64;
            let neighbors_agree = room >= 0 && room <= (nj - pj) as i64;
            let inside = c.value > p.value && c.value < n.value;
            let far = (c.value as i64 - p.value as i64).abs() > 20 && (c.value as i64 - n.value as i64).abs() > 20;
            if neighbors_agree && !inside && far {
                chosen[i] = None;
            }
        }
    }

    let numbers: Vec<Option<DetectedNumber>> = chosen
        .iter()
        .enumerate()
        .map(|(i, c)| {
            c.as_ref().map(|c| DetectedNumber {
                value: c.value,
                kind: c.kind,
                label: label(c.kind, c.value, c.span),
                span: c.span,
                confidence: if corrected[i] { 0.6 * c.confidence.max(0.5) } else { c.confidence * factor[i] },
                corrected: corrected[i] || c.fixed,
                raw: c.raw.clone(),
            })
        })
        .collect();

    // 겹침: 같은 번호(덮는 쪽 기준)가 앞에 이미 있다
    let mut seen: HashMap<(NumberKind, u32), usize> = HashMap::new();
    let mut duplicate = vec![false; pages.len()];
    let mut groups: BTreeMap<(NumberKind, u32), Vec<usize>> = BTreeMap::new();
    for (i, n) in numbers.iter().enumerate() {
        let Some(n) = n else { continue };
        for v in n.value..n.value + n.span {
            groups.entry((n.kind, v)).or_default().push(i);
            if seen.insert((n.kind, v), i).is_some() {
                duplicate[i] = true;
            }
        }
    }
    let duplicates: Vec<Duplicate> = groups
        .iter()
        .filter(|(_, idx)| idx.len() > 1)
        .map(|(&(kind, value), idx)| {
            let mut source_pages: Vec<Uuid> = Vec::new();
            for &i in idx {
                if !source_pages.contains(&pages[i].source_page) {
                    source_pages.push(pages[i].source_page);
                }
            }
            Duplicate { kind, value, output_ids: idx.iter().map(|&i| pages[i].output_id.clone()).collect(), source_pages }
        })
        .collect();

    // 순서 어긋남: 처음 나온 번호들로 가장 긴 증가 부분열을 잡고, 거기 못 든 페이지. 겹친 뒤쪽 사본은 겹침으로만 알린다
    let firsts: Vec<(usize, (NumberKind, u32))> = numbers
        .iter()
        .enumerate()
        .filter(|(i, _)| !duplicate[*i])
        .filter_map(|(i, n)| n.as_ref().map(|n| (i, (n.kind, n.value))))
        .collect();
    let keep = longest_increasing(&firsts.iter().map(|(_, k)| *k).collect::<Vec<_>>());
    let mut out_of_order = vec![false; pages.len()];
    for (pos, (i, _)) in firsts.iter().enumerate() {
        if !keep.contains(&pos) {
            out_of_order[*i] = true;
        }
    }

    // 빈 번호: 종류마다 덮인 번호를 모아 사이가 빈 곳
    let mut gaps = Vec::new();
    for kind in [NumberKind::Roman, NumberKind::Arabic] {
        let mut covered: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
        for (i, n) in numbers.iter().enumerate() {
            let Some(n) = n.as_ref().filter(|n| n.kind == kind) else { continue };
            for v in n.value..n.value + n.span {
                let e = covered.entry(v).or_insert((i, i));
                e.0 = e.0.min(i);
                e.1 = e.1.max(i);
            }
        }
        let values: Vec<(u32, (usize, usize))> = covered.into_iter().collect();
        for pair in values.windows(2) {
            let ((a, (_, a_last)), (b, (b_first, _))) = (pair[0], pair[1]);
            if b <= a + 1 {
                continue;
            }
            let missing_count = b - a - 1;
            let unnumbered_between = if a_last < b_first {
                (a_last + 1..b_first).filter(|&j| numbers[j].is_none()).count()
            } else {
                0
            };
            gaps.push(Gap {
                kind,
                after: a,
                before: b,
                missing_count,
                missing: (a + 1..b).take(MAX_LISTED_MISSING as usize).collect(),
                unnumbered_between,
                likely_unnumbered: unnumbered_between as u32 >= missing_count,
            });
        }
    }
    let missing: Vec<u32> = gaps
        .iter()
        .filter(|g| g.kind == NumberKind::Arabic && g.unnumbered_between == 0)
        .flat_map(|g| g.missing.iter().copied())
        .collect();

    let arabic: Vec<(u32, u32)> =
        numbers.iter().flatten().filter(|n| n.kind == NumberKind::Arabic).map(|n| (n.value, n.value + n.span - 1)).collect();
    let outputs: Vec<OutputNumber> = pages
        .iter()
        .zip(numbers)
        .enumerate()
        .map(|(i, (p, number))| OutputNumber {
            output_id: p.output_id.clone(),
            source_page: p.source_page,
            part: p.part,
            has_ocr: p.ocr.is_some(),
            number,
            duplicate: duplicate[i],
            out_of_order: out_of_order[i],
        })
        .collect();
    let mut report = BookReport {
        detected: outputs.iter().filter(|o| o.number.is_some()).count(),
        total: outputs.len(),
        first: arabic.iter().map(|n| n.0).min(),
        last: arabic.iter().map(|n| n.1).max(),
        out_of_order: outputs.iter().filter(|o| o.out_of_order).map(|o| o.output_id.clone()).collect(),
        outputs,
        missing,
        gaps,
        duplicates,
        sort_moves: 0,
    };
    let current = raw_order(&report.outputs);
    let planned = plan_order(&current, &report);
    report.sort_moves = current.iter().zip(&planned).filter(|(a, b)| a != b).count();
    report
}

/// 가장 긴 순증가 부분열의 위치들(O(n log n))
fn longest_increasing<K: Ord + Copy>(keys: &[K]) -> Vec<usize> {
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; keys.len()];
    for i in 0..keys.len() {
        let pos = tails.partition_point(|&t| keys[t] < keys[i]);
        prev[i] = pos.checked_sub(1).map(|p| tails[p]);
        if pos == tails.len() {
            tails.push(i);
        } else {
            tails[pos] = i;
        }
    }
    let mut out = Vec::new();
    let mut cur = tails.last().copied();
    while let Some(i) = cur {
        out.push(i);
        cur = prev[i];
    }
    out.reverse();
    out
}

/// 출력 순서에서 원본 페이지 순서(처음 나온 순서)
fn raw_order(outputs: &[OutputNumber]) -> Vec<Uuid> {
    let mut order = Vec::new();
    for o in outputs {
        if !order.contains(&o.source_page) {
            order.push(o.source_page);
        }
    }
    order
}

/// 원본 한 장의 정렬 값: (종류, 번호 - 원본 안 자리). 로마 숫자가 아라비아 숫자보다 앞
type SortKey = (NumberKind, i64);

/// 쪽 번호 순서의 원본 페이지 순서.
///
/// 원본 한 장의 정렬 값은 그 장 출력들의 (종류, 번호 - 원본 안 자리) 중 가장 작은 값이다.
/// 펼친 책 왼쪽 56, 오른쪽 57이면 둘 다 56이 되고, 왼쪽을 못 읽고 오른쪽만 57이어도 56이 된다.
/// 번호를 하나도 못 읽은 원본은 지금 바로 앞 원본에 붙어 같이 움직인다(장 첫 페이지, 그림 페이지).
/// 맨 앞의 번호 없는 원본(표지)은 맨 앞에 남는다. 같은 값이면 지금 순서를 지킨다(안정 정렬).
pub fn plan_order(current: &[Uuid], report: &BookReport) -> Vec<Uuid> {
    let mut key_of: HashMap<Uuid, SortKey> = HashMap::new();
    for o in &report.outputs {
        let Some(n) = &o.number else { continue };
        let key = (n.kind, n.value as i64 - part_offset(o.part) as i64);
        key_of.entry(o.source_page).and_modify(|k| *k = (*k).min(key)).or_insert(key);
    }
    // (정렬 값, 원본들). 맨 앞 묶음은 값이 없다
    let mut groups: Vec<(Option<SortKey>, Vec<Uuid>)> = vec![(None, Vec::new())];
    for id in current {
        match key_of.get(id) {
            Some(k) => groups.push((Some(*k), vec![*id])),
            None => groups.last_mut().expect("맨 앞 묶음이 있다").1.push(*id),
        }
    }
    let (head, mut rest) = (groups.remove(0), groups);
    rest.sort_by_key(|(k, _)| *k);
    head.1.into_iter().chain(rest.into_iter().flat_map(|(_, ids)| ids)).collect()
}

// --- Tauri 명령 ---

/// 문서의 출력 페이지를 문서(edits 적용) 순서대로 읽는다. 한 원본 안에서는 왼쪽부터
fn load_pages(dir: &std::path::Path) -> Result<(Vec<Uuid>, Vec<PageInput>), String> {
    let (manifest, _) = load_applied(dir)?;
    let processing = load_processing(dir);
    let order: Vec<Uuid> = manifest.pages.iter().map(|p| p.id).collect();
    let mut outputs: Vec<_> = processing
        .outputs
        .into_iter()
        .filter_map(|o| order.iter().position(|id| *id == o.source_page).map(|pos| (pos, part_offset(o.part), o)))
        .collect();
    outputs.sort_by_key(|(pos, offset, _)| (*pos, *offset));
    let pages = outputs
        .into_iter()
        .map(|(_, _, o)| {
            let ocr = o.ocr.as_ref().and_then(|rel| {
                read_json::<OcrPage>(&dir.join(rel)).unwrap_or_else(|e| {
                    eprintln!("OCR을 읽지 못했다 ({}): {e}", o.id);
                    None
                })
            });
            PageInput { output_id: o.id, source_page: o.source_page, part: o.part, ocr }
        })
        .collect();
    Ok((order, pages))
}

#[tauri::command]
pub async fn book_page_report(app: tauri::AppHandle, document_id: String) -> Result<BookReport, String> {
    let dir = document_dir(&app, &document_id)?;
    // 300쪽이면 OCR 파일 300개를 읽는다. 화면 스레드를 막지 않게 따로 돈다
    tauri::async_runtime::spawn_blocking(move || load_pages(&dir).map(|(_, pages)| analyze(&pages)))
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SortResult {
    pub changed: bool,
    /// 자리가 바뀐 원본 페이지 수
    pub moved: usize,
}

/// 사용자가 누를 때만 부른다. 쪽 번호 순서로 `pageOrder`를 쓰고, processing.json 순서를 맞추게 처리 줄에 세운다
#[tauri::command]
pub async fn sort_by_page_numbers(
    app: tauri::AppHandle,
    pipeline: tauri::State<'_, Pipeline>,
    document_id: String,
) -> Result<SortResult, String> {
    let dir = document_dir(&app, &document_id)?;
    let pipeline = pipeline.inner().clone();
    let id = document_id.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<SortResult, String> {
        let _busy = crate::busy::start(format!("쪽 번호 정렬: {id}"));
        let (current, pages) = load_pages(&dir)?;
        let report = analyze(&pages);
        // 출력이 없는(처리 전) 원본도 순서에 남긴다. 지금 자리 바로 앞 원본에 붙여 둔다
        let with_outputs = raw_order(&report.outputs);
        let planned = plan_order(&with_outputs, &report);
        let order = reinsert_unprocessed(&current, &planned);
        if order == current {
            return Ok(SortResult { changed: false, moved: 0 });
        }
        let moved = current.iter().zip(&order).filter(|(a, b)| a != b).count();
        let mut edits = Edits::load(&dir).map_err(|e| e.to_string())?;
        edits.page_order = Some(order);
        edits.save(&dir).map_err(|e| e.to_string())?;
        Ok(SortResult { changed: true, moved })
    })
    .await
    .map_err(|e| e.to_string())??;
    if result.changed {
        // 처리 줄은 바뀐 페이지가 없어도 마지막에 processing.json을 새 순서로 다시 쓴다(PDF, 검색, AI 묶음이 그 순서를 따른다)
        pipeline.enqueue(&document_id, false);
        crate::search::refresh_document(&app, &document_id);
    }
    Ok(result)
}

/// planned에 없는 원본(아직 출력이 없음)을 current에서 바로 앞에 있던 원본 뒤에 다시 넣는다
fn reinsert_unprocessed(current: &[Uuid], planned: &[Uuid]) -> Vec<Uuid> {
    let mut order: Vec<Uuid> = planned.to_vec();
    for (i, id) in current.iter().enumerate() {
        if order.contains(id) {
            continue;
        }
        let anchor = current[..i].iter().rev().find(|p| order.contains(p));
        let at = anchor.and_then(|a| order.iter().position(|o| o == a)).map_or(0, |p| p + 1);
        order.insert(at, *id);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use scanshelf_core::ocr::{BlockKind, OcrBlock, OcrLine, Rect};

    /// 본문 한 줄과 가장자리 줄(y 위치)을 가진 합성 OCR 페이지
    fn ocr(edge: &[(&str, f64, f64)]) -> OcrPage {
        let mut blocks = vec![block("합성 본문 글줄입니다 2026년 3장", 0.5, 0.45)];
        for (text, x, y) in edge {
            blocks.push(block(text, *x, *y));
        }
        OcrPage { engine: "test".into(), image_width: 1000, image_height: 1400, text: String::new(), blocks }
    }

    fn block(text: &str, x: f64, y: f64) -> OcrBlock {
        let bbox = Rect { x: x - 0.03, y: y - 0.01, width: 0.06, height: 0.02 };
        OcrBlock {
            kind: BlockKind::Paragraph,
            text: text.into(),
            bbox,
            lines: vec![OcrLine { text: text.into(), bbox, confidence: 0.95 }],
        }
    }

    fn page(source: Uuid, part: PagePart, edge: &[(&str, f64, f64)]) -> PageInput {
        PageInput { output_id: part.output_id(source), source_page: source, part, ocr: Some(ocr(edge)) }
    }

    /// 낱장 책: 번호 목록으로 페이지들을 만든다. None은 번호 없는 페이지
    fn singles(numbers: &[Option<&str>]) -> (Vec<Uuid>, Vec<PageInput>) {
        let ids: Vec<Uuid> = numbers.iter().map(|_| Uuid::new_v4()).collect();
        let pages = ids
            .iter()
            .zip(numbers)
            .map(|(id, n)| match n {
                Some(text) => page(*id, PagePart::Whole, &[(text, 0.5, 0.95)]),
                None => page(*id, PagePart::Whole, &[]),
            })
            .collect();
        (ids, pages)
    }

    fn values(report: &BookReport) -> Vec<Option<u32>> {
        report.outputs.iter().map(|o| o.number.as_ref().map(|n| n.value)).collect()
    }

    #[test]
    fn 꾸밈이_붙은_번호를_읽는다() {
        for (text, want) in [("57", 57), ("- 57 -", 57), ("57 |", 57), ("| 57", 57), ("p. 57", 57), ("57쪽", 57), ("[ 203 ]", 203), ("\u{2014} 12 \u{2014}", 12)] {
            assert_eq!(parse_line(text).map(|p| p.0), Some(want), "{text}");
        }
        assert_eq!(parse_line("xiv").map(|p| (p.0, p.1)), Some((14, NumberKind::Roman)));
        assert_eq!(parse_line("57 | 3장 경제의 흐름").map(|p| p.0), Some(57));
        assert_eq!(parse_line("경제의 흐름 | 58").map(|p| p.0), Some(58));
        assert_eq!(parse_line("5B").map(|p| (p.0, p.3)), Some((58, true)));
        assert!(parse_line("2026년 3월 보고서 요약 본문 글줄").is_none());
        assert!(parse_line("iiii").is_none());
        assert!(parse_line("0").is_none());
    }

    #[test]
    fn 가운데_본문의_숫자는_쪽_번호로_보지_않는다() {
        let p = ocr(&[("57", 0.5, 0.5)]);
        assert!(candidates(&p).is_empty());
    }

    #[test]
    fn 빠진_번호와_겹친_번호를_찾는다() {
        // 59를 건너뛰고(두 장 넘김), 61을 두 번 찍었다
        let (_, pages) = singles(&[Some("56"), Some("57"), Some("58"), Some("60"), Some("61"), Some("61"), Some("62")]);
        let r = analyze(&pages);
        assert_eq!(r.missing, vec![59]);
        assert_eq!(r.duplicates.len(), 1);
        assert_eq!(r.duplicates[0].value, 61);
        assert_eq!(r.duplicates[0].output_ids, vec![pages[4].output_id.clone(), pages[5].output_id.clone()]);
        assert!(r.out_of_order.is_empty(), "{:?}", r.out_of_order);
        assert_eq!((r.first, r.last), (Some(56), Some(62)));
    }

    #[test]
    fn 번호_없는_페이지가_있으면_빠진_것으로_단정하지_않는다() {
        // 장 첫 페이지(번호 없음)가 59 자리에 있다
        let (_, pages) = singles(&[Some("57"), Some("58"), None, Some("60")]);
        let r = analyze(&pages);
        assert!(r.missing.is_empty());
        assert_eq!(r.gaps.len(), 1);
        assert!(r.gaps[0].likely_unnumbered);
    }

    #[test]
    fn 잘못_읽은_번호를_앞뒤_순서로_고친다() {
        // 58을 "5B"로, 61을 "67"로 읽었다
        let (_, pages) = singles(&[Some("57"), Some("5B"), Some("59"), Some("60"), Some("67"), Some("62")]);
        let r = analyze(&pages);
        assert_eq!(values(&r), (57..=62).map(Some).collect::<Vec<_>>());
        assert!(r.outputs[1].number.as_ref().unwrap().corrected);
        assert!(r.outputs[4].number.as_ref().unwrap().corrected);
        assert!(r.missing.is_empty() && r.duplicates.is_empty() && r.out_of_order.is_empty());
    }

    #[test]
    fn 동떨어진_숫자는_버린다() {
        // 61 자리 아래쪽에 표 번호 "350"이 걸렸다
        let (_, pages) = singles(&[Some("59"), Some("60"), Some("350"), Some("62"), Some("63")]);
        let r = analyze(&pages);
        assert_eq!(r.outputs[2].number, None, "번호를 지어내지 않는다");
        assert!(r.out_of_order.is_empty() && r.missing.is_empty(), "{r:?}");
    }

    #[test]
    fn 머리글에서_장_번호보다_쪽_번호를_고른다() {
        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let pages: Vec<PageInput> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let n = (100 + i).to_string();
                // 위에는 장 번호 "3", 아래에는 쪽 번호
                page(*id, PagePart::Whole, &[("3", 0.9, 0.04), (n.as_str(), 0.5, 0.96)])
            })
            .collect();
        assert_eq!(values(&analyze(&pages)), vec![Some(100), Some(101), Some(102)]);
    }

    #[test]
    fn 나중에_찍어_끝에_붙인_페이지를_제자리로_정렬한다() {
        // 59, 60을 건너뛰고 끝까지 찍은 뒤 iPhone으로 다시 찍어 맨 뒤에 붙었다
        let (ids, pages) = singles(&[None, Some("57"), Some("58"), Some("61"), Some("62"), Some("59"), Some("60")]);
        let r = analyze(&pages);
        assert_eq!(r.out_of_order.len(), 2, "{:?}", r.out_of_order);
        let order = plan_order(&ids, &r);
        assert_eq!(order, vec![ids[0], ids[1], ids[2], ids[5], ids[6], ids[3], ids[4]], "표지는 맨 앞에 남는다");
        assert_eq!(r.sort_moves, 4);
    }

    #[test]
    fn 펼친_책은_원본_한_장_단위로_정렬한다() {
        // 원본 셋: [56|57], [60|61], [58|59]. 셋째 원본의 왼쪽 번호는 못 읽었다
        let raw: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let pages = vec![
            page(raw[0], PagePart::Left, &[("56", 0.1, 0.95)]),
            page(raw[0], PagePart::Right, &[("57", 0.9, 0.95)]),
            page(raw[1], PagePart::Left, &[("60", 0.1, 0.95)]),
            page(raw[1], PagePart::Right, &[("61", 0.9, 0.95)]),
            page(raw[2], PagePart::Left, &[]),
            page(raw[2], PagePart::Right, &[("59", 0.9, 0.95)]),
        ];
        let r = analyze(&pages);
        assert!(r.missing.is_empty(), "58은 번호 없는 왼쪽 면일 수 있다: {:?}", r.gaps);
        assert_eq!(plan_order(&raw, &r), vec![raw[0], raw[2], raw[1]]);
    }

    #[test]
    fn 나누지_못한_펼친_면은_두_쪽을_덮는다() {
        let raw: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let pages = vec![
            page(raw[0], PagePart::Left, &[("10", 0.1, 0.95)]),
            page(raw[0], PagePart::Right, &[("11", 0.9, 0.95)]),
            // 접힌 선을 못 찾아 통째로 남은 펼친 면
            page(raw[1], PagePart::Whole, &[("12", 0.1, 0.95), ("13", 0.9, 0.95)]),
            page(raw[2], PagePart::Left, &[("14", 0.1, 0.95)]),
        ];
        let r = analyze(&pages);
        assert_eq!(r.outputs[2].number.as_ref().map(|n| (n.value, n.span)), Some((12, 2)));
        assert!(r.missing.is_empty() && r.duplicates.is_empty() && r.out_of_order.is_empty(), "{r:?}");
    }

    #[test]
    fn 앞쪽_로마_숫자는_본문보다_앞에_온다() {
        let (ids, pages) = singles(&[Some("1"), Some("2"), Some("iii"), Some("iv"), Some("3")]);
        let r = analyze(&pages);
        let order = plan_order(&ids, &r);
        assert_eq!(order, vec![ids[2], ids[3], ids[0], ids[1], ids[4]]);
    }

    #[test]
    fn 이미_정렬된_문서는_그대로다() {
        let (ids, pages) = singles(&[None, Some("1"), Some("2"), None, Some("4"), Some("5")]);
        let r = analyze(&pages);
        assert_eq!(plan_order(&ids, &r), ids);
        assert_eq!(r.sort_moves, 0);
    }

    #[test]
    fn 처리_전_원본은_앞_원본_뒤에_남는다() {
        let ids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
        let planned = vec![ids[2], ids[0]];
        assert_eq!(reinsert_unprocessed(&ids, &planned), vec![ids[2], ids[3], ids[0], ids[1]]);
    }

    #[test]
    fn 합성_300쪽_책_정확도() {
        // 1~300쪽 낱장, 일부 번호는 OCR 오독, 일부 페이지는 번호 없음, 두 장 건너뜀, 한 장 겹침
        let mut texts: Vec<Option<String>> = Vec::new();
        for n in 1..=300u32 {
            if n == 120 || n == 121 {
                continue; // 두 장을 한 번에 넘겼다
            }
            let t = match n {
                _ if n % 50 == 1 => None, // 장 첫 페이지
                _ if n % 37 == 0 => Some(n.to_string().replace('8', "B").replace('0', "O").replace('5', "S")),
                _ if n % 23 == 0 => Some(format!("- {n} -")),
                _ => Some(n.to_string()),
            };
            texts.push(t.clone());
            if n == 200 {
                texts.push(t); // 두 번 찍었다
            }
        }
        let refs: Vec<Option<&str>> = texts.iter().map(|t| t.as_deref()).collect();
        let (_, pages) = singles(&refs);
        let r = analyze(&pages);
        assert_eq!(r.missing, vec![120, 121], "{:?}", r.gaps.iter().filter(|g| !g.likely_unnumbered).collect::<Vec<_>>());
        assert_eq!(r.duplicates.iter().map(|d| d.value).collect::<Vec<_>>(), vec![200]);
        assert!(r.out_of_order.is_empty(), "{:?}", r.out_of_order);
        assert_eq!(r.sort_moves, 0);
    }

    /// 재현 가능한 의사 난수(테스트 전용)
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) as u32
        }
        fn chance(&mut self, percent: u32) -> bool {
            self.next() % 100 < percent
        }
    }

    /// 펼친 책 300쪽(원본 150장)을 무작위 사고와 OCR 오류로 찍는다. 원본 하나에 왼쪽, 오른쪽 출력.
    /// 장마다 4% 건너뜀, 3% 두 번 찍음. 쪽마다 6% 번호 없음, 5% 혼동 글자(8→B, 0→O, 1→l), 3% 끝자리 오독, 10% 머리글 장 번호
    /// 돌려주는 값: (출력 입력, 출력별 실제 인쇄 번호, 실제로 빠진 번호, 두 번 찍은 장의 왼쪽 번호)
    fn noisy_spread_book(seed: u64) -> (Vec<PageInput>, Vec<Option<u32>>, Vec<u32>, Vec<u32>) {
        let mut rng = Lcg(seed);
        let mut pages = Vec::new();
        let mut truth = Vec::new();
        let (mut missing, mut dups) = (Vec::new(), Vec::new());
        let mut n = 1u32;
        while n < 300 {
            // 4%: 한 장(두 쪽) 건너뜀, 3%: 같은 장을 두 번 찍음
            if rng.chance(4) {
                missing.extend([n, n + 1]);
                n += 2;
                continue;
            }
            let repeats = if rng.chance(3) { 2 } else { 1 };
            if repeats == 2 {
                dups.push(n);
            }
            for _ in 0..repeats {
                let raw = Uuid::new_v4();
                for (part, value, x) in [(PagePart::Left, n, 0.1), (PagePart::Right, n + 1, 0.9)] {
                    let printed = !(rng.chance(6)); // 장 첫 페이지, 그림 페이지
                    let text = if !printed {
                        None
                    } else if rng.chance(5) {
                        Some(value.to_string().replace('8', "B").replace('0', "O").replace('1', "l"))
                    } else if rng.chance(3) {
                        // 숫자 하나를 잘못 읽음
                        let s = value.to_string();
                        let last = (s.as_bytes()[s.len() - 1] - b'0' + 3) % 10;
                        Some(format!("{}{}", &s[..s.len() - 1], last))
                    } else {
                        Some(value.to_string())
                    };
                    let mut edge: Vec<(String, f64, f64)> = text.iter().map(|t| (t.clone(), x, 0.95)).collect();
                    if rng.chance(10) {
                        edge.push(("7".into(), 0.5, 0.04)); // 머리글의 장 번호
                    }
                    let edge_refs: Vec<(&str, f64, f64)> = edge.iter().map(|(t, x, y)| (t.as_str(), *x, *y)).collect();
                    pages.push(page(raw, part, &edge_refs));
                    truth.push(printed.then_some(value));
                }
            }
            n += 2;
        }
        (pages, truth, missing, dups)
    }

    #[test]
    fn 합성_펼친_책_무작위_정확도() {
        let (mut right, mut wrong, mut undetected, mut printed) = (0, 0, 0, 0);
        let (mut miss_found, mut miss_total, mut miss_false) = (0, 0, 0);
        let (mut dup_found, mut dup_total, mut dup_false) = (0, 0, 0);
        let (mut order_false, mut outputs_total) = (0, 0);
        for seed in 1..=20u64 {
            let (pages, truth, missing, dups) = noisy_spread_book(seed);
            let r = analyze(&pages);
            for (o, t) in r.outputs.iter().zip(&truth) {
                let Some(t) = t else { continue };
                printed += 1;
                match &o.number {
                    Some(n) if n.value == *t => right += 1,
                    Some(_) => wrong += 1,
                    None => undetected += 1,
                }
            }
            // 빈 번호: 번호 없는 페이지로 설명되지 않는 구간 + 설명될 수도 있는 구간 모두 화면에 나온다
            let flagged: Vec<u32> = r.gaps.iter().flat_map(|g| g.missing.iter().copied()).collect();
            miss_total += missing.len();
            miss_found += missing.iter().filter(|m| flagged.contains(m)).count();
            miss_false += r.missing.iter().filter(|m| !missing.contains(m)).count();
            let dup_values: Vec<u32> = r.duplicates.iter().map(|d| d.value).collect();
            dup_total += dups.len();
            // 두 번 찍은 장 하나에서 두 쪽 중 하나라도 겹침으로 나오면 찾은 것
            dup_found += dups.iter().filter(|d| dup_values.contains(d) || dup_values.contains(&(**d + 1))).count();
            dup_false += dup_values.iter().filter(|v| !dups.contains(v) && !dups.contains(&(**v - 1))).count();
            order_false += r.out_of_order.len();
            outputs_total += r.total;
        }
        let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
        eprintln!(
            "쪽 번호: 맞음 {:.1}%, 틀림 {:.1}%, 못 읽음 {:.1}% (인쇄 {printed}쪽) | 빠진 번호 재현율 {:.1}%, 오탐 {miss_false} | 겹침 재현율 {:.1}%, 오탐 {dup_false} | 순서 어긋남 오탐 {order_false}/{outputs_total}",
            pct(right, printed),
            pct(wrong, printed),
            pct(undetected, printed),
            pct(miss_found, miss_total),
            pct(dup_found, dup_total),
        );
        assert!(pct(right, printed) > 97.0);
        assert!(pct(wrong, printed) < 1.0);
        assert!(pct(miss_found, miss_total) > 95.0);
        assert!(pct(dup_found, dup_total) > 95.0);
        // 오탐은 한 문서(300쪽)에 평균 한 건 미만. 같은 장 두 쪽을 함께 잘못 읽으면 앞뒤로도 못 고친다
        assert!(dup_false < 20 && miss_false < 20);
        assert!(pct(order_false, outputs_total) < 0.5);
    }
}

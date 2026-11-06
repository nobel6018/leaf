//! OCR 줄을 블록(제목, 단락, 목록)으로 묶고 읽는 순서를 정한다.
//!
//! `VNRecognizeTextRequest`는 줄 단위 결과만 준다. 단락 구조를 주는 `RecognizeDocumentsRequest`(macOS 26)는
//! Swift 전용이라 그 연결이 붙기 전까지 위치와 글자 크기로 묶는다.
//!
//! @see docs/ocr.md

use scanshelf_core::ocr::{BlockKind, OcrBlock, OcrLine, OcrPage, Rect};

/// 목록 줄 머리표. 가운뎃점은 U+00B7, U+2022 둘 다 쓰인다
const BULLETS: [char; 8] = ['\u{2022}', '\u{00B7}', '-', '*', '\u{25AA}', '\u{25A0}', '\u{25CF}', '\u{2013}'];

fn right(r: &Rect) -> f64 {
    r.x + r.width
}

fn bottom(r: &Rect) -> f64 {
    r.y + r.height
}

fn union(a: &Rect, b: &Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect {
        x,
        y,
        width: right(a).max(right(b)) - x,
        height: bottom(a).max(bottom(b)) - y,
    }
}

fn overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// 두 줄이 같은 블록에 들어갈 만큼 가까운가. 좌표는 페이지 정규화 좌표지만 가로세로 비율이 달라 픽셀로 바꿔 비교한다
fn adjacent(a: &Rect, b: &Rect, aspect: f64) -> bool {
    let (ah, bh) = (a.height, b.height);
    let min_h = ah.min(bh);
    if min_h <= 0.0 || ah.max(bh) / min_h > 1.6 {
        return false;
    }
    let vertical_overlap = overlap(a.y, bottom(a), b.y, bottom(b));
    if vertical_overlap > 0.5 * min_h {
        // 같은 줄에서 끊긴 조각: 가로 간격이 글자 높이 두 배 안쪽이면 잇는다
        let gap = (b.x - right(a)).max(a.x - right(b)) * aspect;
        return gap < 2.0 * min_h;
    }
    let gap = (b.y - bottom(a)).max(a.y - bottom(b));
    let horizontal = overlap(a.x, right(a), b.x, right(b));
    gap < 0.9 * min_h && horizontal > 0.0
}

fn find(parent: &mut [usize], i: usize) -> usize {
    let mut root = i;
    while parent[root] != root {
        root = parent[root];
    }
    let mut node = i;
    while parent[node] != root {
        let next = parent[node];
        parent[node] = root;
        node = next;
    }
    root
}

/// 재귀 XY 자르기. 가로로 빈 띠가 있으면 위아래로, 없으면 세로 빈 띠로 단을 나눈다. 팜플렛의 2단, 3단 배치를 읽는 순서대로 편다
fn reading_order(boxes: &[Rect], indices: Vec<usize>, out: &mut Vec<usize>) {
    if indices.len() <= 1 {
        out.extend(indices);
        return;
    }
    for axis in [Axis::Y, Axis::X] {
        let groups = split_by_gaps(boxes, &indices, axis);
        if groups.len() > 1 {
            for group in groups {
                reading_order(boxes, group, out);
            }
            return;
        }
    }
    let mut rest = indices;
    rest.sort_by(|&a, &b| boxes[a].y.total_cmp(&boxes[b].y).then(boxes[a].x.total_cmp(&boxes[b].x)));
    out.extend(rest);
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

fn split_by_gaps(boxes: &[Rect], indices: &[usize], axis: Axis) -> Vec<Vec<usize>> {
    let span = |i: usize| match axis {
        Axis::X => (boxes[i].x, right(&boxes[i])),
        Axis::Y => (boxes[i].y, bottom(&boxes[i])),
    };
    let mut sorted = indices.to_vec();
    sorted.sort_by(|&a, &b| span(a).0.total_cmp(&span(b).0));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut end = f64::NEG_INFINITY;
    for i in sorted {
        let (start, stop) = span(i);
        if groups.is_empty() || start > end {
            groups.push(vec![i]);
        } else {
            groups.last_mut().expect("방금 넣었다").push(i);
        }
        end = end.max(stop);
    }
    groups
}

fn is_bullet_line(text: &str) -> bool {
    let trimmed = text.trim_start();
    let mut chars = trimmed.chars();
    match chars.next() {
        Some(c) if BULLETS.contains(&c) => true,
        // "1." "2)" 같은 번호 목록
        Some(c) if c.is_ascii_digit() => {
            let rest: String = chars.take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ')').collect();
            rest.ends_with('.') || rest.ends_with(')')
        }
        _ => false,
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// 위에서 아래로, 같은 줄이면 왼쪽부터.
/// "겹치면 x, 아니면 y"를 비교 함수 하나로 쓰면 안 된다. 기울어진 글줄에서 A와 B, B와 C는 겹치고 A와 C는 안 겹칠 수 있어
/// 순서가 돌고 돌아(A<B<C<A) 정렬이 panic한다. 그래서 y로 정렬한 뒤 줄로 묶고, 줄 안에서만 x로 정렬한다
fn sort_lines_for_reading(mut lines: Vec<OcrLine>) -> Vec<OcrLine> {
    lines.sort_by(|a, b| a.bbox.y.total_cmp(&b.bbox.y));
    let mut rows: Vec<Vec<OcrLine>> = Vec::new();
    for line in lines {
        let same_row = rows.last().and_then(|row| row.last()).is_some_and(|last| {
            overlap(last.bbox.y, bottom(&last.bbox), line.bbox.y, bottom(&line.bbox))
                > 0.5 * last.bbox.height.min(line.bbox.height)
        });
        if same_row {
            rows.last_mut().expect("같은 줄이면 줄이 있다").push(line);
        } else {
            rows.push(vec![line]);
        }
    }
    for row in &mut rows {
        row.sort_by(|a, b| a.bbox.x.total_cmp(&b.bbox.x));
    }
    rows.into_iter().flatten().collect()
}

/// 영어 줄 끝 하이픈은 떼고 잇는다. 한글과 그 밖의 줄은 공백으로 잇는다
fn join_lines(lines: &[OcrLine], kind: BlockKind) -> String {
    if kind == BlockKind::List {
        return lines.iter().map(|l| l.text.trim()).collect::<Vec<_>>().join("\n");
    }
    let mut text = String::new();
    for line in lines {
        let piece = line.text.trim();
        if text.is_empty() {
            text.push_str(piece);
        } else if text.ends_with('-') && piece.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
            text.pop();
            text.push_str(piece);
        } else {
            text.push(' ');
            text.push_str(piece);
        }
    }
    text
}

/// 줄 목록으로 OcrPage를 만든다. `lines`의 bbox는 왼쪽 위 원점 정규화 좌표
pub fn build_page(engine: &str, image_width: u32, image_height: u32, lines: Vec<OcrLine>) -> OcrPage {
    let lines: Vec<OcrLine> = lines.into_iter().filter(|l| !l.text.trim().is_empty()).collect();
    let aspect = image_width as f64 / image_height.max(1) as f64;
    let n = lines.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in i + 1..n {
            if adjacent(&lines[i].bbox, &lines[j].bbox, aspect) {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[rj] = ri;
                }
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut root_to_group = std::collections::HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        let group = *root_to_group.entry(root).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group].push(i);
    }

    let page_line_height = median(lines.iter().map(|l| l.bbox.height).collect());
    let mut blocks: Vec<OcrBlock> = groups
        .into_iter()
        .map(|members| {
            let block_lines: Vec<OcrLine> = members.iter().map(|&i| lines[i].clone()).collect();
            let block_lines = sort_lines_for_reading(block_lines);
            let bbox = block_lines
                .iter()
                .skip(1)
                .fold(block_lines[0].bbox, |acc, l| union(&acc, &l.bbox));
            let line_height = median(block_lines.iter().map(|l| l.bbox.height).collect());
            let bullets = block_lines.iter().filter(|l| is_bullet_line(&l.text)).count();
            let kind = if block_lines.len() >= 2 && bullets * 2 >= block_lines.len() {
                BlockKind::List
            } else if block_lines.len() <= 3 && line_height > 1.5 * page_line_height {
                BlockKind::Title
            } else {
                BlockKind::Paragraph
            };
            OcrBlock {
                kind,
                text: join_lines(&block_lines, kind),
                bbox,
                lines: block_lines,
            }
        })
        .collect();

    let boxes: Vec<Rect> = blocks.iter().map(|b| b.bbox).collect();
    let mut order = Vec::with_capacity(blocks.len());
    reading_order(&boxes, (0..blocks.len()).collect(), &mut order);
    let mut slots: Vec<Option<OcrBlock>> = blocks.drain(..).map(Some).collect();
    let blocks: Vec<OcrBlock> = order.into_iter().filter_map(|i| slots[i].take()).collect();
    let text = blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n\n");
    OcrPage {
        engine: engine.to_string(),
        image_width,
        image_height,
        text,
        blocks,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn 기울어진_글줄이_많아도_정렬이_panic하지_않고_위에서_아래로_읽는다() {
        // 비교가 돌고 도는 배치: 이웃끼리만 겹치고 x는 아래로 갈수록 왼쪽으로 간다
        let lines: Vec<OcrLine> = (0..200)
            .map(|i| OcrLine {
                text: format!("줄{i}"),
                bbox: Rect { x: 0.9 - i as f64 * 0.004, y: i as f64 * 0.004, width: 0.05, height: 0.007 },
                confidence: 1.0,
            })
            .collect();

        let ordered = super::sort_lines_for_reading(lines.iter().rev().cloned().collect());

        assert_eq!(ordered.len(), 200);
        assert_eq!(ordered[0].text, "줄0");
    }

    #[test]
    fn 같은_줄은_왼쪽부터_다음_줄은_그_아래() {
        let line = |text: &str, x: f64, y: f64| OcrLine {
            text: text.into(),
            bbox: Rect { x, y, width: 0.1, height: 0.02 },
            confidence: 1.0,
        };

        let ordered = super::sort_lines_for_reading(vec![line("다", 0.1, 0.1), line("나", 0.5, 0.05), line("가", 0.1, 0.051)]);

        let texts: Vec<&str> = ordered.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["가", "나", "다"]);
    }

    use super::*;

    fn line(text: &str, x: f64, y: f64, w: f64, h: f64) -> OcrLine {
        OcrLine {
            text: text.to_string(),
            bbox: Rect { x, y, width: w, height: h },
            confidence: 1.0,
        }
    }

    #[test]
    fn 가까운_줄은_한_단락으로_묶는다() {
        let page = build_page(
            "test",
            1000,
            1000,
            vec![
                line("첫째 줄입니다", 0.1, 0.30, 0.6, 0.02),
                line("둘째 줄입니다", 0.1, 0.33, 0.6, 0.02),
                line("떨어진 단락", 0.1, 0.60, 0.6, 0.02),
            ],
        );
        assert_eq!(page.blocks.len(), 2);
        assert_eq!(page.blocks[0].text, "첫째 줄입니다 둘째 줄입니다");
        assert_eq!(page.text, "첫째 줄입니다 둘째 줄입니다\n\n떨어진 단락");
    }

    #[test]
    fn 큰_글자는_제목이다() {
        let page = build_page(
            "test",
            1000,
            1000,
            vec![
                line("한빛로보틱스", 0.1, 0.05, 0.5, 0.06),
                line("본문 하나", 0.1, 0.30, 0.6, 0.02),
                line("본문 둘", 0.1, 0.33, 0.6, 0.02),
                line("본문 셋", 0.1, 0.50, 0.6, 0.02),
            ],
        );
        assert_eq!(page.blocks[0].kind, BlockKind::Title);
        assert_eq!(page.blocks[1].kind, BlockKind::Paragraph);
    }

    #[test]
    fn 머리표_줄은_목록이다() {
        let page = build_page(
            "test",
            1000,
            1000,
            vec![
                line("\u{2022} 적재 하중 20kg", 0.1, 0.30, 0.6, 0.02),
                line("\u{2022} 반복 정밀도 0.02mm", 0.1, 0.33, 0.6, 0.02),
            ],
        );
        assert_eq!(page.blocks[0].kind, BlockKind::List);
        assert_eq!(page.blocks[0].text, "\u{2022} 적재 하중 20kg\n\u{2022} 반복 정밀도 0.02mm");
    }

    #[test]
    fn 두_단은_왼쪽_단을_먼저_읽는다() {
        let page = build_page(
            "test",
            1000,
            1000,
            vec![
                line("오른쪽 1", 0.55, 0.30, 0.35, 0.02),
                line("왼쪽 1", 0.05, 0.30, 0.35, 0.02),
                line("오른쪽 2", 0.55, 0.33, 0.35, 0.02),
                line("왼쪽 2", 0.05, 0.33, 0.35, 0.02),
            ],
        );
        assert_eq!(page.text, "왼쪽 1 왼쪽 2\n\n오른쪽 1 오른쪽 2");
    }

    #[test]
    fn 영어_줄끝_하이픈은_붙인다() {
        let page = build_page(
            "test",
            1000,
            1000,
            vec![line("auto-", 0.1, 0.30, 0.3, 0.02), line("mation", 0.1, 0.33, 0.3, 0.02)],
        );
        assert_eq!(page.text, "automation");
    }
}

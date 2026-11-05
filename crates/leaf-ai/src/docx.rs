//! Word(.docx) 내보내기. OCR 블록(제목, 단락, 목록, 표)을 출력 페이지 순서대로 옮긴다.
//!
//! OCR 결과에는 표 칸 구조가 없어, 표 블록은 줄 위치(bbox)로 행과 열을 다시 짠다.
//!
//! @see docs/ai.md

use std::path::{Path, PathBuf};

use docx_rs::{
    BreakType, Docx, PageMargin, Paragraph, Pic, Run, RunFonts, Table, TableCell, TableRow,
};
use leaf_core::ocr::{BlockKind, OcrBlock, OcrLine};

use crate::document::LoadedDocument;
use crate::error::AiError;
use crate::fsutil::{sanitize_file_name, write_atomic};
use crate::images::{encode_png, load_downscaled};

/// A4, 여백 1인치(단위 twip = 1/1440인치)
const PAGE_WIDTH: u32 = 11_906;
const PAGE_HEIGHT: u32 = 16_838;
const MARGIN: i32 = 1_440;
const EMU_PER_TWIP: u32 = 635;

#[derive(Debug, Clone)]
pub struct DocxOptions {
    /// 페이지마다 글자 앞에 처리된 페이지 이미지를 넣는다
    pub include_images: bool,
    /// docx-rs는 그림을 PNG로만 넣어 파일이 커진다. 긴 변을 이만큼 줄인다
    pub image_max_edge: u32,
    /// Word는 글꼴 대체 목록을 받지 않아 하나만 정한다. 없는 글꼴이면 Word가 비슷한 한글 글꼴로 바꿔 보여 준다
    pub font: String,
}

impl Default for DocxOptions {
    fn default() -> Self {
        Self {
            include_images: false,
            image_max_edge: 1_600,
            font: default_korean_font().to_string(),
        }
    }
}

pub fn default_korean_font() -> &'static str {
    if cfg!(windows) {
        "Malgun Gothic"
    } else {
        "Apple SD Gothic Neo"
    }
}

/// `exports/<문서 이름>.docx`에 쓰고 경로를 돌려준다
pub fn export_docx(document_dir: &Path, options: &DocxOptions) -> Result<PathBuf, AiError> {
    let document = LoadedDocument::load(document_dir)?;
    let bytes = build_docx(&document, options)?;
    let file_name = sanitize_file_name(&document.manifest.name, &document.manifest.id.to_string());
    let path = document_dir
        .join("exports")
        .join(format!("{file_name}.docx"));
    write_atomic(&path, &bytes)?;
    Ok(path)
}

pub fn build_docx(document: &LoadedDocument, options: &DocxOptions) -> Result<Vec<u8>, AiError> {
    let font = RunFonts::new()
        .ascii(&options.font)
        .hi_ansi(&options.font)
        .east_asia(&options.font)
        .cs(&options.font);
    let mut docx = Docx::new()
        .page_size(PAGE_WIDTH, PAGE_HEIGHT)
        .page_margin(
            PageMargin::new()
                .top(MARGIN)
                .bottom(MARGIN)
                .left(MARGIN)
                .right(MARGIN),
        )
        .default_fonts(font)
        .add_paragraph(
            Paragraph::new().add_run(Run::new().add_text(&document.manifest.name).size(36).bold()),
        );

    for (index, page) in document.pages.iter().enumerate() {
        if index > 0 {
            docx =
                docx.add_paragraph(Paragraph::new().add_run(Run::new().add_break(BreakType::Page)));
        }
        if options.include_images && page.image_path.is_file() {
            docx =
                docx.add_paragraph(Paragraph::new().add_run(
                    Run::new().add_image(picture(&page.image_path, options.image_max_edge)?),
                ));
        }
        let Some(ocr) = &page.ocr else { continue };
        for block in &ocr.blocks {
            docx = add_block(docx, block);
        }
    }

    let mut cursor = std::io::Cursor::new(Vec::new());
    docx.build()
        .pack(&mut cursor)
        .map_err(|e| AiError::Docx(e.to_string()))?;
    Ok(cursor.into_inner())
}

fn picture(path: &Path, max_edge: u32) -> Result<Pic, AiError> {
    let image = load_downscaled(path, max_edge)?;
    let (width, height) = (image.width().max(1), image.height().max(1));
    let png = encode_png(&image, path)?;
    let max_w = (PAGE_WIDTH - 2 * MARGIN as u32) * EMU_PER_TWIP;
    // 그림 아래 글자가 한 줄은 같은 쪽에 오게 높이를 조금 남긴다
    let max_h = (PAGE_HEIGHT - 2 * MARGIN as u32) * EMU_PER_TWIP / 10 * 9;
    let scale = (max_w as f64 / width as f64).min(max_h as f64 / height as f64);
    let (w_emu, h_emu) = (
        (width as f64 * scale) as u32,
        (height as f64 * scale) as u32,
    );
    Ok(Pic::new_with_dimensions(png, width, height).size(w_emu, h_emu))
}

fn text_paragraph(text: &str) -> Paragraph {
    Paragraph::new().add_run(Run::new().add_text(text))
}

fn add_block(docx: Docx, block: &OcrBlock) -> Docx {
    match block.kind {
        BlockKind::Title => docx.add_paragraph(
            Paragraph::new().add_run(
                Run::new()
                    .add_text(reflow(&block_lines(block)))
                    .size(30)
                    .bold(),
            ),
        ),
        BlockKind::Paragraph => docx.add_paragraph(text_paragraph(&reflow(&block_lines(block)))),
        BlockKind::List => list_items(&block_lines(block))
            .into_iter()
            .fold(docx, |docx, item| {
                docx.add_paragraph(text_paragraph(&format!("\u{2022} {item}")).indent(
                    Some(360),
                    None,
                    None,
                    None,
                ))
            }),
        BlockKind::Table => {
            let rows = table_rows(block);
            if rows.is_empty() {
                return docx;
            }
            let columns = rows.iter().map(Vec::len).max().unwrap_or(1);
            let table_rows = rows
                .into_iter()
                .map(|mut cells| {
                    cells.resize(columns, String::new());
                    TableRow::new(
                        cells
                            .into_iter()
                            .map(|cell| TableCell::new().add_paragraph(text_paragraph(&cell)))
                            .collect(),
                    )
                })
                .collect();
            docx.add_table(Table::new(table_rows))
                .add_paragraph(Paragraph::new())
        }
    }
}

fn block_lines(block: &OcrBlock) -> Vec<String> {
    if block.lines.is_empty() {
        block.text.lines().map(str::to_string).collect()
    } else {
        block.lines.iter().map(|l| l.text.clone()).collect()
    }
}

/// OCR 줄은 인쇄물의 줄바꿈 위치라 Word에서는 한 단락으로 다시 잇는다. 영어 하이픈 줄바꿈은 붙인다
fn reflow(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        if out.ends_with('-') && line.starts_with(|c: char| c.is_ascii_lowercase()) {
            out.pop();
        } else if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line);
    }
    out
}

const BULLETS: [char; 8] = [
    '\u{2022}', '-', '*', '\u{25AA}', '\u{25A0}', '\u{25CF}', '\u{25E6}', '\u{2023}',
];

fn strip_marker(line: &str) -> Option<&str> {
    let line = line.trim_start();
    if let Some(rest) = line.strip_prefix(BULLETS) {
        return Some(rest.trim_start());
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 3 {
        let rest = &line[digits..];
        if let Some(rest) = rest.strip_prefix(['.', ')']) {
            return Some(rest.trim_start());
        }
    }
    None
}

/// 표시가 붙은 줄에서 새 항목을 시작하고, 표시 없는 줄은 앞 항목에 잇는다. 표시가 하나도 없으면 줄마다 항목이다
fn list_items(lines: &[String]) -> Vec<String> {
    let lines: Vec<&str> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if !lines.iter().any(|l| strip_marker(l).is_some()) {
        return lines.into_iter().map(str::to_string).collect();
    }
    let mut items: Vec<Vec<String>> = Vec::new();
    for line in lines {
        match strip_marker(line) {
            Some(rest) => items.push(vec![rest.to_string()]),
            None => match items.last_mut() {
                Some(item) => item.push(line.to_string()),
                None => items.push(vec![line.to_string()]),
            },
        }
    }
    items.iter().map(|item| reflow(item)).collect()
}

/// 탭으로 나뉜 글자면 그대로 쓰고, 아니면 줄 위치로 행(세로 중심이 가까운 줄)과 열(왼쪽부터)을 짠다
fn table_rows(block: &OcrBlock) -> Vec<Vec<String>> {
    if block.text.contains('\t') || block.lines.is_empty() {
        return block
            .text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split('\t').map(|c| c.trim().to_string()).collect())
            .collect();
    }
    let mut lines: Vec<&OcrLine> = block.lines.iter().collect();
    lines.sort_by(|a, b| center_y(a).total_cmp(&center_y(b)));
    let mut heights: Vec<f64> = lines.iter().map(|l| l.bbox.height).collect();
    heights.sort_by(f64::total_cmp);
    let tolerance = heights[heights.len() / 2] * 0.5;

    let mut rows: Vec<Vec<&OcrLine>> = Vec::new();
    for line in lines {
        match rows.last_mut() {
            Some(row) if (center_y(line) - row_center(row)).abs() <= tolerance => row.push(line),
            _ => rows.push(vec![line]),
        }
    }
    rows.into_iter()
        .map(|mut row| {
            row.sort_by(|a, b| a.bbox.x.total_cmp(&b.bbox.x));
            row.into_iter().map(|l| l.text.trim().to_string()).collect()
        })
        .collect()
}

fn center_y(line: &OcrLine) -> f64 {
    line.bbox.y + line.bbox.height / 2.0
}

fn row_center(row: &[&OcrLine]) -> f64 {
    row.iter().map(|l| center_y(l)).sum::<f64>() / row.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use leaf_core::ocr::Rect;

    fn line(text: &str, x: f64, y: f64) -> OcrLine {
        OcrLine {
            text: text.into(),
            bbox: Rect {
                x,
                y,
                width: 0.1,
                height: 0.02,
            },
            confidence: 1.0,
        }
    }

    #[test]
    fn 표_줄을_위치로_행과_열로_짠다() {
        let block = OcrBlock {
            kind: BlockKind::Table,
            text: String::new(),
            bbox: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            lines: vec![
                line("하중", 0.5, 0.101),
                line("모델", 0.1, 0.1),
                line("X1", 0.1, 0.2),
                line("500 kg", 0.5, 0.203),
            ],
        };
        assert_eq!(
            table_rows(&block),
            vec![
                vec!["모델".to_string(), "하중".into()],
                vec!["X1".into(), "500 kg".into()]
            ]
        );
    }

    #[test]
    fn 목록은_표시가_붙은_줄에서_새_항목을_시작한다() {
        let lines = [
            "\u{2022} 자율 주행".to_string(),
            "랙 운반".into(),
            "2. 원격 관제".into(),
        ];
        assert_eq!(
            list_items(&lines),
            vec!["자율 주행 랙 운반".to_string(), "원격 관제".into()]
        );
    }

    #[test]
    fn 영어_하이픈_줄바꿈은_붙인다() {
        assert_eq!(
            reflow(&["auto-".into(), "mation".into(), "로봇".into()]),
            "automation 로봇"
        );
    }
}

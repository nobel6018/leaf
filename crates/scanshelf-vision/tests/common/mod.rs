//! 합성 평가 데이터. 실제 책이나 스캔은 쓰지 않는다(저작권, 개인 자료).
//! 글자는 시스템 글꼴이 있으면 뜻 없는 낱말로 그리고, 없으면 낱말 크기의 막대로 대신한다.
//!
//! @see docs/vision.md

#![allow(dead_code)]

use ab_glyph::{FontVec, PxScale};
use image::{GrayImage, Luma, Rgb, RgbImage};
use scanshelf_vision::util::{downscale_area, luma};

pub const PAPER: [u8; 3] = [246, 244, 238];
pub const INK: [u8; 3] = [28, 28, 30];
pub const SKIN: [u8; 3] = [212, 158, 130];
pub const BLUE_PEN: [u8; 3] = [38, 62, 168];
pub const RED_PEN: [u8; 3] = [188, 36, 42];

/// 재현 가능한 의사 난수(테스트 전용)
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    pub fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + self.next() % (hi - lo)
    }
}

const FONT_PATHS: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Times New Roman.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "C:\\Windows\\Fonts\\times.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
];

pub fn system_font() -> Option<FontVec> {
    FONT_PATHS
        .iter()
        .find_map(|p| std::fs::read(p).ok().and_then(|b| FontVec::try_from_vec(b).ok()))
}

fn word(rng: &mut Lcg) -> String {
    const SYL: &[&str] = &["ka", "lo", "mi", "ren", "tu", "sa", "vel", "nor", "pi", "qua", "de", "zon", "bri", "ex", "mol"];
    let n = rng.range(1, 4);
    (0..n).map(|_| SYL[rng.range(0, SYL.len() as u32) as usize]).collect()
}

/// 줄글 페이지. 제목 한 줄과 문단 몇 개
pub fn render_page(w: u32, h: u32, seed: u64) -> RgbImage {
    let mut img = RgbImage::from_pixel(w, h, Rgb(PAPER));
    let font = system_font();
    let mut rng = Lcg::new(seed);
    let size = h as f32 / 52.0;
    let margin_x = (w as f32 * 0.1) as i32;
    let right = w as i32 - margin_x;
    let mut y = (h as f32 * 0.08) as i32;
    let bottom = (h as f32 * 0.9) as i32;
    let mut first = true;
    while y < bottom {
        let line_size = if first { size * 1.6 } else { size };
        let mut x = margin_x;
        // 문단 끝 줄은 짧다
        let end = if rng.range(0, 7) == 0 { margin_x + (right - margin_x) * rng.range(30, 80) as i32 / 100 } else { right };
        loop {
            let text = word(&mut rng);
            let advance = draw_word(&mut img, font.as_ref(), x, y, line_size, &text);
            if x + advance > end {
                break;
            }
            x += advance + (line_size * 0.35) as i32;
            if first && x > margin_x + (right - margin_x) / 2 {
                break;
            }
        }
        y += (line_size * 1.65) as i32;
        if first {
            y += size as i32;
            first = false;
        }
    }
    img
}

/// 낱말 하나를 그리고 폭을 돌려준다. 오른쪽 끝을 넘으면 그리지 않는다
fn draw_word(img: &mut RgbImage, font: Option<&FontVec>, x: i32, y: i32, size: f32, text: &str) -> i32 {
    match font {
        Some(f) => {
            let scale = PxScale::from(size);
            let (tw, _) = imageproc::drawing::text_size(scale, f, text);
            if x + tw as i32 <= img.width() as i32 * 9 / 10 {
                imageproc::drawing::draw_text_mut(img, Rgb(INK), x, y, scale, f, text);
            }
            tw as i32
        }
        None => {
            let tw = (text.len() as f32 * size * 0.5) as i32;
            if x + tw <= img.width() as i32 * 9 / 10 {
                let top = y + (size * 0.25) as i32;
                let rect = imageproc::rect::Rect::at(x, top).of_size(tw.max(1) as u32, (size * 0.6) as u32);
                imageproc::drawing::draw_filled_rect_mut(img, rect, Rgb(INK));
            }
            tw
        }
    }
}

/// 책 페이지처럼 접힌 쪽(왼쪽)으로 갈수록 굽게 휜다. 페이지는 어두운 바탕 위에 놓인다.
/// 역방향 사상: 출력 (x, y)가 평평한 원본 어디인지 계산한다
pub fn curl(page: &RgbImage, strength: f32, shadow: f32) -> RgbImage {
    let (w, h) = (page.width() as f32, page.height() as f32);
    let margin = 0.05;
    let (ow, oh) = ((w * (1.0 + 2.0 * margin)) as u32, (h * (1.0 + 2.0 * margin)) as u32);
    let raw = page.as_raw();
    RgbImage::from_fn(ow, oh, |xo, yo| {
        let u = xo as f32 - w * margin;
        let v = yo as f32 - h * margin;
        // 접힌 쪽 휘어짐 정도(0~1)
        let c = (-(u.max(0.0)) / (w * 0.3)).exp();
        let cy = h / 2.0;
        // 위아래 줄이 바깥으로 휘고(원통 원근), 전체가 아래로 처진다
        let ys = cy + (v - cy - strength * h * 0.04 * c) / (1.0 + strength * 0.12 * c);
        // 접힌 쪽은 가로로 눌려 보인다
        let xs = u + strength * w * 0.05 * (1.0 - c);
        if xs < 0.0 || ys < 0.0 || xs >= w - 1.0 || ys >= h - 1.0 || u < 0.0 {
            return Rgb([70, 64, 58]);
        }
        let p = scanshelf_vision::util::sample_rgb(raw, page.width() as usize, page.height() as usize, xs, ys);
        let s = 1.0 - shadow * c;
        Rgb(p.map(|v| (v * s).clamp(0.0, 255.0) as u8))
    })
}

/// 테두리에서 들어온 손가락 하나(타원). 정답 마스크도 돌려준다
pub fn add_finger(img: &mut RgbImage, from_left: bool, cy: f32, len: f32, thick: f32) -> GrayImage {
    let (w, h) = img.dimensions();
    let mut truth = GrayImage::new(w, h);
    let x_edge = if from_left { 0.0 } else { w as f32 };
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 - x_edge) / len;
            let dy = (y as f32 - cy) / thick;
            let d = dx * dx + dy * dy;
            if d <= 1.0 {
                // 가운데가 밝고 가장자리가 어두운 입체감
                let s = 0.82 + 0.18 * (1.0 - d);
                img.put_pixel(x, y, Rgb(SKIN.map(|v| (v as f32 * s) as u8)));
                truth.put_pixel(x, y, Luma([255]));
            }
        }
    }
    truth
}

/// 물결 모양 펜 자국
pub fn add_scribble(img: &mut RgbImage, color: [u8; 3], y0: f32, amp: f32, period: f32, thick: f32) {
    let (w, h) = img.dimensions();
    for x in (w / 8)..(w * 7 / 8) {
        let yc = y0 + amp * (x as f32 / period).sin();
        let t = thick.ceil() as i32;
        for dy in -t..=t {
            let y = yc as i32 + dy;
            if y >= 0 && (y as u32) < h && (dy as f32).abs() <= thick {
                img.put_pixel(x, y as u32, Rgb(color));
            }
        }
    }
}

/// 가로 글줄 단면의 뾰족함(표준편차/평균). 글줄이 곧을수록 크다
pub fn line_sharpness(img: &RgbImage) -> f32 {
    let bw = scanshelf_vision::apply_filter(img, scanshelf_core::processing::Filter::BlackWhite);
    let (w, h) = bw.dimensions();
    let x0 = w / 10;
    let x1 = w * 9 / 10;
    let rows: Vec<f32> = (h / 20..h * 19 / 20)
        .map(|y| (x0..x1).filter(|&x| bw.get_pixel(x, y).0[0] == 0).count() as f32)
        .collect();
    let m = rows.iter().sum::<f32>() / rows.len() as f32;
    let s = (rows.iter().map(|v| (v - m).powi(2)).sum::<f32>() / rows.len() as f32).sqrt();
    s / m.max(1e-3)
}

/// 8x8 창 평균 SSIM. 두 이미지를 같은 크기(가로 400)로 맞춰 휘도로 비교한다
pub fn ssim(a: &RgbImage, b: &RgbImage) -> f32 {
    let tw = 400;
    let th = (tw as f32 * b.height() as f32 / b.width() as f32) as u32;
    let la = luma(&fit(a, tw, th));
    let lb = luma(&fit(b, tw, th));
    let (c1, c2) = ((0.01f32 * 255.0).powi(2), (0.03f32 * 255.0).powi(2));
    let mut total = 0.0;
    let mut n = 0;
    for by in (0..th as usize - 8).step_by(4) {
        for bx in (0..tw as usize - 8).step_by(4) {
            let (mut ma, mut mb) = (0.0, 0.0);
            for y in by..by + 8 {
                for x in bx..bx + 8 {
                    ma += la.get(x, y);
                    mb += lb.get(x, y);
                }
            }
            ma /= 64.0;
            mb /= 64.0;
            let (mut va, mut vb, mut cov) = (0.0, 0.0, 0.0);
            for y in by..by + 8 {
                for x in bx..bx + 8 {
                    let (da, db) = (la.get(x, y) - ma, lb.get(x, y) - mb);
                    va += da * da;
                    vb += db * db;
                    cov += da * db;
                }
            }
            va /= 63.0;
            vb /= 63.0;
            cov /= 63.0;
            total += ((2.0 * ma * mb + c1) * (2.0 * cov + c2)) / ((ma * ma + mb * mb + c1) * (va + vb + c2));
            n += 1;
        }
    }
    total / n as f32
}

fn fit(img: &RgbImage, w: u32, h: u32) -> RgbImage {
    if img.width() >= w && img.height() >= h {
        downscale_area(img, w, h)
    } else {
        image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle)
    }
}

/// 마스크 a가 b를 덮는 비율과 b 밖으로 넘친 비율
pub fn coverage(pred: &GrayImage, truth: &GrayImage) -> (f32, f32) {
    let mut hit = 0;
    let mut t = 0;
    let mut extra = 0;
    for (p, q) in pred.iter().zip(truth.iter()) {
        let (p, q) = (*p > 127, *q > 127);
        if q {
            t += 1;
            if p {
                hit += 1;
            }
        } else if p {
            extra += 1;
        }
    }
    (hit as f32 / t.max(1) as f32, extra as f32 / pred.len() as f32)
}

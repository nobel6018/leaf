//! 필기 지우기(색 펜). 검은 인쇄 글자와 색이 다른 파랑, 빨강, 초록 펜 자국만 지운다.
//!
//! 1. 종이 배경으로 나눠 조명과 종이색을 걷어낸다(filter::Background)
//! 2. 채도가 높고 종이보다 어두운 화소를 색상각으로 파랑, 빨강, 초록 묶음에 나눈다
//! 3. 넓고 꽉 찬 덩어리(인쇄된 색 상자, 로고)는 펜 획이 아니라서 뺀다
//! 4. 경계까지 조금 넓혀 메운다(inpaint)
//!
//! 한계: 연필과 검은 펜은 인쇄 글자와 색으로 구별할 수 없어 지우지 못한다. 색 인쇄 글자(파란 제목 등)는 펜과 같이
//! 지워진다. 그래서 기본값은 꺼짐이고, 팜플렛보다 책에 맞다. 형광펜은 글자 위에 덮여 있어 메우면 글자가 사라지므로
//! 지우지 않는다. 손글씨 분할 모델을 붙이면 이 한계를 넘을 수 있다(docs/vision.md).
//!
//! @see docs/vision.md

use image::{GrayImage, Luma, RgbImage};
use rayon::prelude::*;

use crate::VisionError;
use crate::filter::Background;
use crate::inpaint::{InpaintEngine, inpaint};
use crate::models::Models;
use crate::util::{components, dilate};

/// 채도(최대-최소 채널, 0~255) 하한
const MIN_CHROMA: f32 = 50.0;
/// 종이(255)보다 이만큼 어두워야 잉크
const MAX_LIGHTNESS: f32 = 225.0;
/// 덩어리가 이미지의 이 비율보다 크고 꽉 차 있으면 인쇄물로 본다
const SOLID_AREA: f32 = 0.002;
const SOLID_FILL: f32 = 0.55;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InkColor {
    Blue,
    Red,
    Green,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HandwritingReport {
    /// 색별 화소 수(파랑, 빨강, 초록)
    pub pixels: [usize; 3],
    /// 인쇄물로 보고 남긴 덩어리 수
    pub kept_solid: usize,
    pub area: f32,
    pub engine: InpaintEngine,
}

/// 배경으로 나눈 색이 펜 잉크면 그 색
#[inline]
pub fn classify(n: [f32; 3]) -> Option<InkColor> {
    let max = n[0].max(n[1]).max(n[2]);
    let min = n[0].min(n[1]).min(n[2]);
    let chroma = max - min;
    let lightness = 0.299 * n[0] + 0.587 * n[1] + 0.114 * n[2];
    if chroma < MIN_CHROMA || lightness > MAX_LIGHTNESS {
        return None;
    }
    let hue = hue_deg(n, max, chroma);
    match hue {
        h if !(20.0..330.0).contains(&h) => Some(InkColor::Red),
        h if (80.0..170.0).contains(&h) => Some(InkColor::Green),
        h if (190.0..270.0).contains(&h) => Some(InkColor::Blue),
        // 노랑, 주황(형광펜, 살구색 인쇄), 보라 근처는 애매해서 둔다
        _ => None,
    }
}

fn hue_deg(n: [f32; 3], max: f32, chroma: f32) -> f32 {
    let [r, g, b] = n;
    let h = if max == r {
        ((g - b) / chroma).rem_euclid(6.0)
    } else if max == g {
        (b - r) / chroma + 2.0
    } else {
        (r - g) / chroma + 4.0
    };
    h * 60.0
}

/// 색 펜 자국 마스크(원본 크기, 255 = 지울 곳)
pub fn detect_colored_ink(img: &RgbImage) -> (GrayImage, HandwritingReport) {
    let bg = Background::estimate(img);
    let (w, h) = img.dimensions();
    let wu = w as usize;
    let raw = img.as_raw();
    let mut labels = vec![0u8; wu * h as usize];
    labels.par_chunks_mut(wu).enumerate().for_each(|(y, row)| {
        for (x, l) in row.iter_mut().enumerate() {
            let i = (y * wu + x) * 3;
            let b = bg.at(x as f32, y as f32);
            let n = [0, 1, 2].map(|c| (raw[i + c] as f32 / b[c].max(8.0) * 255.0).min(255.0));
            *l = match classify(n) {
                Some(InkColor::Blue) => 1,
                Some(InkColor::Red) => 2,
                Some(InkColor::Green) => 3,
                None => 0,
            };
        }
    });
    let candidate = GrayImage::from_raw(w, h, labels.iter().map(|&l| if l > 0 { 255 } else { 0 }).collect())
        .expect("크기 계산이 맞다");
    let total = (w * h) as f32;
    let mut mask = GrayImage::new(w, h);
    let mut pixels = [0usize; 3];
    let mut kept_solid = 0;
    for comp in components(&candidate) {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for &(x, y) in &comp {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let bbox = ((x1 - x0 + 1) * (y1 - y0 + 1)) as f32;
        let fill = comp.len() as f32 / bbox;
        if comp.len() as f32 / total >= SOLID_AREA && fill >= SOLID_FILL {
            kept_solid += 1;
            continue;
        }
        // 잡티 한두 화소는 JPEG 색 번짐일 수 있어 뺀다
        if comp.len() < 4 {
            continue;
        }
        for (x, y) in comp {
            let l = labels[y as usize * wu + x as usize];
            pixels[(l - 1) as usize] += 1;
            mask.put_pixel(x, y, Luma([255]));
        }
    }
    // 획 가장자리의 옅은 색 번짐까지 덮는다
    let r = (w.max(h) / 1500).max(1);
    let mask = dilate(&mask, r);
    let area = mask.iter().filter(|v| **v > 127).count() as f32 / total;
    (mask, HandwritingReport { pixels, kept_solid, area, engine: InpaintEngine::None })
}

pub fn remove_handwriting(img: &RgbImage, models: &Models) -> Result<(RgbImage, HandwritingReport), VisionError> {
    let (mask, mut report) = detect_colored_ink(img);
    let (out, engine) = inpaint(img, &mask, models)?;
    report.engine = engine;
    Ok((out, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn 펜_색을_나눈다() {
        assert_eq!(classify([40.0, 70.0, 170.0]), Some(InkColor::Blue));
        assert_eq!(classify([190.0, 40.0, 45.0]), Some(InkColor::Red));
        assert_eq!(classify([40.0, 130.0, 60.0]), Some(InkColor::Green));
        assert_eq!(classify([30.0, 30.0, 32.0]), None, "검은 인쇄");
        assert_eq!(classify([250.0, 250.0, 248.0]), None, "종이");
        assert_eq!(classify([250.0, 240.0, 90.0]), None, "형광펜");
    }

    fn page_with_pen() -> RgbImage {
        RgbImage::from_fn(500, 500, |x, y| {
            let fx = x as f32;
            let fy = y as f32;
            // 파란 물결선 필기
            let wave = 250.0 + 40.0 * (fx / 30.0).sin();
            if (fy - wave).abs() < 2.0 && (50..450).contains(&x) {
                return Rgb([35, 60, 165]);
            }
            // 빨간 동그라미
            let d = ((fx - 120.0).powi(2) + (fy - 120.0).powi(2)).sqrt();
            if (d - 40.0).abs() < 1.5 {
                return Rgb([185, 35, 40]);
            }
            // 인쇄된 파란 상자(지우면 안 된다)
            if (330..450).contains(&x) && (60..140).contains(&y) {
                return Rgb([30, 80, 170]);
            }
            let line = (y / 10) % 3 == 0 && (x / 7) % 5 != 0 && x > 30 && x < 470;
            if line { Rgb([25, 25, 25]) } else { Rgb([238, 234, 224]) }
        })
    }

    #[test]
    fn 색_펜만_지우고_인쇄는_남긴다() {
        let img = page_with_pen();
        let (out, report) = remove_handwriting(&img, &Models::none()).unwrap();
        assert!(report.pixels[0] > 500 && report.pixels[1] > 300, "{report:?}");
        assert_eq!(report.kept_solid, 1);
        let mut blue_left = 0;
        let mut black_kept = 0;
        let mut black_total = 0;
        for (x, y, p) in out.enumerate_pixels() {
            let src = img.get_pixel(x, y).0;
            if src == [35, 60, 165] && classify(p.0.map(|v| v as f32)).is_some() {
                blue_left += 1;
            }
            if src == [25, 25, 25] {
                black_total += 1;
                if p.0[0] < 80 {
                    black_kept += 1;
                }
            }
        }
        assert!(blue_left < 20, "남은 파란 화소 {blue_left}");
        assert!(black_kept as f32 > black_total as f32 * 0.95, "검은 글자 {black_kept}/{black_total}");
        assert_eq!(out.get_pixel(390, 100).0, [30, 80, 170], "인쇄 상자는 그대로");
    }
}

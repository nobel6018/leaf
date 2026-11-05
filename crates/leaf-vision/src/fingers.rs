//! 손가락 지우기. 책을 누르고 찍으면 페이지 가장자리로 손가락이 들어온다.
//!
//! 1. 종이 색으로 흰색을 맞춘 뒤 YCrCb 피부색 범위로 화소를 고른다
//! 2. 열림, 닫힘으로 잡티를 지우고, 이미지 테두리에 닿은 덩어리만 남긴다. 손은 페이지 밖에서 들어오기 때문이다
//! 3. 손가락 그림자와 경계를 덮게 조금 넓혀 메운다(inpaint)
//!
//! 플랫폼 쪽(iPhone Vision 손 인식 등)이 마스크를 주면 합쳐서 쓴다.
//! 피부색과 비슷한 살구색, 주황색 인쇄물이 테두리에 닿아 있으면 손으로 잘못 볼 수 있다(docs/vision.md 한계).
//!
//! @see docs/vision.md

use image::{GrayImage, Luma, RgbImage};

use crate::VisionError;
use crate::inpaint::{InpaintEngine, inpaint};
use crate::models::Models;
use crate::util::{components, dilate, downscale_area, erode, fit_long_side, resize_mask};

const ANALYSIS_LONG_SIDE: u32 = 512;
/// 손가락 하나는 페이지의 0.15% 이상, 손 전체라도 25% 이하
const MIN_AREA: f32 = 0.0015;
const MAX_AREA: f32 = 0.25;

#[derive(Debug, Clone, PartialEq)]
pub struct FingerReport {
    /// 찾은 손 덩어리 수(외부 마스크 제외)
    pub detected: usize,
    /// 지운 면적 비율(0~1)
    pub area: f32,
    pub engine: InpaintEngine,
}

/// 테두리에서 들어온 피부색 영역 마스크(원본 크기, 255 = 손)
pub fn detect_fingers(img: &RgbImage) -> (GrayImage, usize) {
    let (sw, sh) = fit_long_side(img.width(), img.height(), ANALYSIS_LONG_SIDE);
    let small = downscale_area(img, sw, sh);
    let gains = paper_gains(&small);
    let mut skin = GrayImage::new(sw, sh);
    for (x, y, p) in small.enumerate_pixels() {
        let [r, g, b] = [0, 1, 2].map(|c| (p.0[c] as f32 * gains[c]).min(255.0));
        if is_skin(r, g, b) {
            skin.put_pixel(x, y, Luma([255]));
        }
    }
    let cleaned = erode(&dilate(&dilate(&erode(&skin, 1), 1), 2), 2);
    let total = (sw * sh) as f32;
    let mut mask = GrayImage::new(sw, sh);
    let mut detected = 0;
    for comp in components(&cleaned) {
        let area = comp.len() as f32 / total;
        let touches = comp.iter().any(|&(x, y)| x <= 1 || y <= 1 || x + 2 >= sw || y + 2 >= sh);
        if touches && (MIN_AREA..=MAX_AREA).contains(&area) {
            detected += 1;
            for (x, y) in comp {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
    }
    // 손가락 가장자리의 반그림자와 흐린 경계까지 덮는다
    let mask = dilate(&mask, 3);
    (resize_mask(&mask, img.width(), img.height()), detected)
}

/// 손가락을 찾아 지운다. external은 플랫폼 쪽 손 마스크(크기가 달라도 맞춰 쓴다)
pub fn remove_fingers(
    img: &RgbImage,
    external: Option<&GrayImage>,
    models: &Models,
) -> Result<(RgbImage, FingerReport), VisionError> {
    let (mut mask, detected) = detect_fingers(img);
    if let Some(ext) = external {
        let ext = resize_mask(ext, img.width(), img.height());
        for (m, e) in mask.iter_mut().zip(ext.iter()) {
            *m = (*m).max(*e);
        }
    }
    let area = mask.iter().filter(|v| **v > 127).count() as f32 / (img.width() * img.height()) as f32;
    let (out, engine) = inpaint(img, &mask, models)?;
    Ok((out, FingerReport { detected, area, engine }))
}

/// 가장 밝은 10% 화소(종이)의 색이 회색이 되게 하는 채널 배율
fn paper_gains(img: &RgbImage) -> [f32; 3] {
    let mut lum: Vec<(f32, [u8; 3])> =
        img.pixels().map(|p| (0.299 * p.0[0] as f32 + 0.587 * p.0[1] as f32 + 0.114 * p.0[2] as f32, p.0)).collect();
    if lum.is_empty() {
        return [1.0; 3];
    }
    let k = lum.len() * 9 / 10;
    lum.select_nth_unstable_by(k, |a, b| a.0.total_cmp(&b.0));
    let mut acc = [0f32; 3];
    for (_, p) in &lum[k..] {
        for c in 0..3 {
            acc[c] += p[c] as f32;
        }
    }
    let mean = (acc[0] + acc[1] + acc[2]) / 3.0;
    acc.map(|a| if a > 0.0 { mean / a } else { 1.0 })
}

/// YCrCb 피부색 범위. 종이(Cr, Cb가 128 근처)와 겹치지 않게 Cr 하한을 높게 잡았다
#[inline]
pub fn is_skin(r: f32, g: f32, b: f32) -> bool {
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let cr = 128.0 + 0.5 * r - 0.4187 * g - 0.0813 * b;
    let cb = 128.0 - 0.1687 * r - 0.3313 * g + 0.5 * b;
    y > 50.0 && (138.0..=178.0).contains(&cr) && (77.0..=130.0).contains(&cb) && cr - cb >= 15.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn page_with_thumb() -> (RgbImage, GrayImage) {
        let (w, h) = (600, 800);
        let mut truth = GrayImage::new(w, h);
        let img = RgbImage::from_fn(w, h, |x, y| {
            // 왼쪽 테두리에서 들어온 엄지(타원)
            let dx = x as f32 / 90.0;
            let dy = (y as f32 - 500.0) / 45.0;
            if dx * dx + dy * dy <= 1.0 {
                truth.put_pixel(x, y, Luma([255]));
                return Rgb([214, 160, 132]);
            }
            let line = (y / 10) % 3 == 0 && x > 120 && x < 560 && (x / 8) % 5 != 0;
            if line { Rgb([30, 30, 30]) } else { Rgb([236, 232, 222]) }
        });
        (img, truth)
    }

    #[test]
    fn 테두리에서_들어온_손가락을_찾는다() {
        let (img, truth) = page_with_thumb();
        let (mask, n) = detect_fingers(&img);
        assert_eq!(n, 1);
        let inter = mask.iter().zip(truth.iter()).filter(|(a, b)| **a > 127 && **b > 127).count();
        let truth_n = truth.iter().filter(|v| **v > 127).count();
        assert!(inter as f32 >= truth_n as f32 * 0.97, "덮은 비율 {inter}/{truth_n}");
        // 글자 쪽은 건드리지 않는다
        assert_eq!(mask.get_pixel(300, 300).0[0], 0);
    }

    #[test]
    fn 가운데_떠_있는_살구색은_손이_아니다() {
        let img = RgbImage::from_fn(400, 400, |x, y| {
            if (150..250).contains(&x) && (150..250).contains(&y) { Rgb([214, 160, 132]) } else { Rgb([236, 232, 222]) }
        });
        let (_, n) = detect_fingers(&img);
        assert_eq!(n, 0);
    }

    #[test]
    fn 지우면_종이색이_된다() {
        let (img, _) = page_with_thumb();
        let (out, report) = remove_fingers(&img, None, &Models::none()).unwrap();
        assert_eq!(report.detected, 1);
        let p = out.get_pixel(20, 500).0;
        assert!(!is_skin(p[0] as f32, p[1] as f32, p[2] as f32), "{p:?}");
        assert!(p[0] > 200 && p[2] > 190, "{p:?}");
    }

    #[test]
    fn 외부_마스크를_합친다() {
        let img = RgbImage::from_pixel(300, 300, Rgb([236, 232, 222]));
        let mut ext = GrayImage::new(150, 150);
        for y in 0..20 {
            for x in 0..20 {
                ext.put_pixel(x, y, Luma([255]));
            }
        }
        let (_, report) = remove_fingers(&img, Some(&ext), &Models::none()).unwrap();
        assert!(report.area > 0.015, "{report:?}");
    }
}

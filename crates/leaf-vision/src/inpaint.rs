//! 마스크 영역 메우기. 손가락과 필기 지우기가 같이 쓴다.
//!
//! LaMa는 512x512 고정 입력이라 사진 전체가 아니라 마스크 영역마다 둘레 여백을 붙인 정사각형 조각을 잘라
//! 512로 맞춰 넣고, 결과를 원래 크기로 되돌려 마스크 안쪽만 붙인다. 모델이 없으면 pull-push 보간으로 메운다.
//! 종이 여백 위 손가락은 둘 다 자연스럽지만, 글자 위를 지나는 필기는 LaMa가 글자 획을 더 잘 잇는다.
//!
//! @see docs/vision.md

use image::{GrayImage, RgbImage, imageops};
use ort::value::TensorRef;

use crate::VisionError;
use crate::filter::pull_push;
use crate::models::{ModelId, Models};
use crate::util::{Plane, components, dilate, downscale_area, fit_long_side, resize_mask};

const LAMA_SIZE: u32 = 512;
/// 이보다 큰 영역은 타일로 나눈다(512로 줄이는 비율이 2배를 넘지 않게)
const TILE_MAX: u32 = 1024;
const TILE: u32 = 768;
const TILE_OVERLAP: u32 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InpaintEngine {
    Lama,
    /// 모델 없이 둘레 값 보간
    Classical,
    /// 마스크가 비어 할 일이 없었다
    None,
}

/// 사각형 [x0, x1) x [y0, y1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl Rect {
    fn width(&self) -> u32 {
        self.x1 - self.x0
    }
    fn height(&self) -> u32 {
        self.y1 - self.y0
    }
    fn overlaps(&self, o: &Rect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
    fn union(&self, o: &Rect) -> Rect {
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
}

/// mask에서 255인 곳을 메운다. 모델이 있으면 LaMa, 없으면 고전 보간
pub fn inpaint(img: &RgbImage, mask: &GrayImage, models: &Models) -> Result<(RgbImage, InpaintEngine), VisionError> {
    if mask.dimensions() != img.dimensions() {
        return Err(VisionError::MaskSize);
    }
    let regions = regions(mask);
    if regions.is_empty() {
        return Ok((img.clone(), InpaintEngine::None));
    }
    let use_lama = models.available(ModelId::Lama) && models.get(ModelId::Lama).is_some();
    let mut out = img.clone();
    for region in &regions {
        // LaMa는 512 입력이라 큰 영역을 통째로 넣으면 크게 줄였다 늘려 흐려진다. 타일로 나눠 원래 해상도에 가깝게 메운다.
        // 앞 타일이 메운 결과가 다음 타일의 문맥이 되도록 차례로 돈다
        let tiles = if use_lama { tiles(region) } else { vec![*region] };
        for r in &tiles {
            let crop_mask = imageops::crop_imm(mask, r.x0, r.y0, r.width(), r.height()).to_image();
            if !crop_mask.iter().any(|v| *v > 127) {
                continue;
            }
            let crop = imageops::crop_imm(&out, r.x0, r.y0, r.width(), r.height()).to_image();
            let filled = if use_lama { lama(&crop, &crop_mask, models)? } else { classical(&crop, &crop_mask) };
            blend_into(&mut out, &filled, &crop_mask, r);
        }
    }
    Ok((out, if use_lama { InpaintEngine::Lama } else { InpaintEngine::Classical }))
}

/// 마스크 영역마다 둘레 여백을 붙인 조각. 겹치는 조각은 합친다
pub fn regions(mask: &GrayImage) -> Vec<Rect> {
    let (w, h) = mask.dimensions();
    // 성분 찾기는 줄인 마스크에서 한다. 가까운 획은 팽창으로 한 덩어리가 된다
    let (sw, sh) = fit_long_side(w, h, 768);
    let small = dilate(&shrink_mask_max(mask, sw, sh), 3);
    let (fx, fy) = (w as f32 / sw as f32, h as f32 / sh as f32);
    let mut rects: Vec<Rect> = components(&small)
        .into_iter()
        .map(|px| {
            let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
            for (x, y) in px {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
            let r = Rect {
                x0: (x0 as f32 * fx) as u32,
                y0: (y0 as f32 * fy) as u32,
                x1: ((x1 as f32 * fx).ceil() as u32).min(w),
                y1: ((y1 as f32 * fy).ceil() as u32).min(h),
            };
            pad_square(r, w, h)
        })
        .collect();
    // 합친 뒤 다시 겹칠 수 있어 더 바뀌지 않을 때까지 합친다
    loop {
        let mut merged = false;
        let mut i = 0;
        while i < rects.len() {
            let mut j = i + 1;
            while j < rects.len() {
                if rects[i].overlaps(&rects[j]) {
                    rects[i] = rects[i].union(&rects[j]);
                    rects.swap_remove(j);
                    merged = true;
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
        if !merged {
            break;
        }
    }
    rects
}

/// 한 변이 TILE_MAX 이하면 그대로, 크면 TILE 크기 타일로 겹쳐 나눈다
fn tiles(r: &Rect) -> Vec<Rect> {
    if r.width().max(r.height()) <= TILE_MAX {
        return vec![*r];
    }
    let starts = |a: u32, b: u32| -> Vec<u32> {
        if b - a <= TILE {
            return vec![a];
        }
        let mut v: Vec<u32> = (a..b - TILE).step_by((TILE - TILE_OVERLAP) as usize).collect();
        v.push(b - TILE);
        v
    };
    let mut out = Vec::new();
    for y in starts(r.y0, r.y1) {
        for x in starts(r.x0, r.x1) {
            out.push(Rect { x0: x, y0: y, x1: (x + TILE).min(r.x1), y1: (y + TILE).min(r.y1) });
        }
    }
    out
}

/// 영역 둘레에 긴 변 절반(최소 32화소) 여백을 붙이고 정사각형에 가깝게 늘린다. 모델에 문맥을 주기 위해서다
fn pad_square(r: Rect, w: u32, h: u32) -> Rect {
    let long = r.width().max(r.height());
    let pad = (long / 2).max(32);
    let side = (long + 2 * pad).min(w.max(h));
    let cx = (r.x0 + r.x1) / 2;
    let cy = (r.y0 + r.y1) / 2;
    let half = side / 2;
    let clamp_span = |c: u32, limit: u32| {
        let span = side.min(limit);
        let start = c.saturating_sub(half).min(limit - span);
        (start, start + span)
    };
    let (x0, x1) = clamp_span(cx, w);
    let (y0, y1) = clamp_span(cy, h);
    Rect { x0, y0, x1, y1 }
}

/// 축소하면서 하나라도 켜진 칸은 켠다. 가는 필기 획이 줄이다가 사라지지 않게 한다
fn shrink_mask_max(mask: &GrayImage, sw: u32, sh: u32) -> GrayImage {
    let (w, h) = mask.dimensions();
    if (w, h) == (sw, sh) {
        return mask.clone();
    }
    let mut out = GrayImage::new(sw, sh);
    for (x, y, p) in mask.enumerate_pixels() {
        if p.0[0] > 127 {
            let sx = (x as u64 * sw as u64 / w as u64) as u32;
            let sy = (y as u64 * sh as u64 / h as u64) as u32;
            out.put_pixel(sx, sy, image::Luma([255]));
        }
    }
    out
}

fn lama(crop: &RgbImage, mask: &GrayImage, models: &Models) -> Result<RgbImage, VisionError> {
    let loaded = models.get(ModelId::Lama).ok_or_else(|| VisionError::Model("LaMa를 불러오지 못했습니다".into()))?;
    let s = LAMA_SIZE;
    let small = if crop.width() >= s && crop.height() >= s {
        downscale_area(crop, s, s)
    } else {
        imageops::resize(crop, s, s, imageops::FilterType::Triangle)
    };
    // 줄이면서 가장자리 화소가 마스크 밖으로 새지 않게 한 칸 넓힌다
    let small_mask = dilate(&resize_mask(&dilate(mask, 1), s, s), 1);
    let n = (s * s) as usize;
    let mut image = vec![0f32; 3 * n];
    let mut m = vec![0f32; n];
    for (i, (p, mv)) in small.as_raw().as_chunks::<3>().0.iter().zip(small_mask.as_raw()).enumerate() {
        let hole = *mv > 127;
        m[i] = if hole { 1.0 } else { 0.0 };
        for c in 0..3 {
            image[c * n + i] = if hole { 0.0 } else { p[c] as f32 / 255.0 };
        }
    }
    let err = |e: ort::Error| VisionError::Model(format!("LaMa 실행 실패: {e}"));
    let side = s as usize;
    let image_t = TensorRef::from_array_view(([1usize, 3, side, side], &image[..])).map_err(err)?;
    let mask_t = TensorRef::from_array_view(([1usize, 1, side, side], &m[..])).map_err(err)?;
    let mut session = loaded.session.lock().map_err(|_| VisionError::Model("세션 잠금 실패".into()))?;
    let outputs = session.run(ort::inputs!["image" => image_t, "mask" => mask_t]).map_err(err)?;
    let (_, data) = outputs["output"].try_extract_tensor::<f32>().map_err(err)?;
    let mut result = RgbImage::new(s, s);
    for (i, p) in result.as_mut().as_chunks_mut::<3>().0.iter_mut().enumerate() {
        for c in 0..3 {
            p[c] = data[c * n + i].clamp(0.0, 255.0) as u8;
        }
    }
    let mut back = imageops::resize(&result, crop.width(), crop.height(), imageops::FilterType::Triangle);
    match_tone(&mut back, crop, mask);
    Ok(back)
}

/// 줄였다 늘린 모델 출력은 종이 색이 조금 어긋나 메운 자리가 얼룩처럼 보인다.
/// 마스크 밖(원본이 살아 있는 곳)에서 채널별 평균 차이를 구해 메운 결과 전체를 그만큼 옮긴다
fn match_tone(filled: &mut RgbImage, original: &RgbImage, mask: &GrayImage) {
    let keep = dilate(mask, 3);
    let mut diff = [0f64; 3];
    let mut n = 0u64;
    for ((f, o), m) in filled.pixels().zip(original.pixels()).zip(keep.iter()) {
        if *m > 127 {
            continue;
        }
        for ((d, ov), fv) in diff.iter_mut().zip(o.0).zip(f.0) {
            *d += ov as f64 - fv as f64;
        }
        n += 1;
    }
    if n == 0 {
        return;
    }
    let shift = diff.map(|d| (d / n as f64) as f32);
    for p in filled.pixels_mut() {
        for (v, s) in p.0.iter_mut().zip(shift) {
            *v = (*v as f32 + s).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// 마스크 밖의 종이 화소만 남기고 pull-push로 메운다. 글자 화소까지 섞으면 메운 자리가 회색 얼룩이 되므로
/// 둘레 종이 밝기(상위 10%)보다 많이 어두운 화소는 뺀다. 가려진 글자는 되살리지 못하고 종이색이 된다
pub fn classical(crop: &RgbImage, mask: &GrayImage) -> RgbImage {
    let (w, h) = (crop.width() as usize, crop.height() as usize);
    let lum: Vec<f32> = crop.pixels().map(|p| crate::util::luma_of(p.0)).collect();
    let mut outside: Vec<f32> = lum.iter().zip(mask.as_raw()).filter(|(_, m)| **m <= 127).map(|(l, _)| *l).collect();
    let paper = if outside.is_empty() {
        0.0
    } else {
        let k = outside.len() * 9 / 10;
        *outside.select_nth_unstable_by(k, f32::total_cmp).1
    };
    let mut valid: Vec<bool> = mask.as_raw().iter().zip(&lum).map(|(&m, &l)| m <= 127 && l >= paper * 0.75).collect();
    if !valid.iter().any(|v| *v) {
        valid = mask.as_raw().iter().map(|&m| m <= 127).collect();
    }
    let mut out = crop.clone();
    for c in 0..3 {
        let p = Plane { width: w, height: h, data: crop.as_raw().iter().skip(c).step_by(3).map(|&v| v as f32).collect() };
        let filled = pull_push(&p, &valid);
        for (o, v) in out.as_mut().iter_mut().skip(c).step_by(3).zip(&filled.data) {
            *o = v.clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// 마스크 경계를 2화소 부드럽게 해 붙인다
fn blend_into(dst: &mut RgbImage, filled: &RgbImage, mask: &GrayImage, r: &Rect) {
    let soft = {
        let grown = dilate(mask, 2);
        let p = Plane {
            width: grown.width() as usize,
            height: grown.height() as usize,
            data: grown.as_raw().iter().map(|&v| v as f32 / 255.0).collect(),
        };
        crate::util::box_blur(&p, 2)
    };
    for y in 0..r.height() {
        for x in 0..r.width() {
            let a = soft.get(x as usize, y as usize);
            if a <= 0.0 {
                continue;
            }
            let src = filled.get_pixel(x, y).0;
            let d = dst.get_pixel_mut(r.x0 + x, r.y0 + y);
            for (dc, sc) in d.0.iter_mut().zip(src) {
                *dc = (sc as f32 * a + *dc as f32 * (1.0 - a)).round() as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Luma, Rgb};

    #[test]
    fn 고전_보간은_종이색으로_메운다() {
        let mut img = RgbImage::from_pixel(200, 200, Rgb([230, 228, 220]));
        let mut mask = GrayImage::new(200, 200);
        for y in 60..120 {
            for x in 0..50 {
                img.put_pixel(x, y, Rgb([200, 140, 120]));
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        let (out, engine) = inpaint(&img, &mask, &Models::none()).unwrap();
        assert_eq!(engine, InpaintEngine::Classical);
        let p = out.get_pixel(20, 90).0;
        assert!((p[0] as i32 - 230).abs() < 6 && (p[2] as i32 - 220).abs() < 6, "{p:?}");
        assert_eq!(out.get_pixel(150, 150).0, [230, 228, 220]);
    }

    #[test]
    fn 겹치는_영역은_합친다() {
        let mut mask = GrayImage::new(400, 400);
        for x in 100..110 {
            mask.put_pixel(x, 100, Luma([255]));
            mask.put_pixel(x, 130, Luma([255]));
        }
        mask.put_pixel(390, 390, Luma([255]));
        let r = regions(&mask);
        assert_eq!(r.len(), 2, "{r:?}");
    }

    #[test]
    fn 빈_마스크는_아무것도_안_한다() {
        let img = RgbImage::new(50, 50);
        let (_, e) = inpaint(&img, &GrayImage::new(50, 50), &Models::none()).unwrap();
        assert_eq!(e, InpaintEngine::None);
    }
}

//! 조명 보정과 필터(원본, 보정, 흑백 회색조, 흑백 이진).
//!
//! 보정(Enhanced)은 종이 배경을 추정해 나눈다. 배경은 블록마다 밝은 화소의 색이고, 블록이 사진이나
//! 굵은 제목처럼 종이가 아닌 곳이면 주변 종이 블록에서 메워 넣는다. 채널마다 나누므로 그림자 제거와
//! 흰색 맞춤이 한 번에 된다. 사진 영역도 주변 종이와 같은 비율로 밝아져 색이 날아가지 않는다.
//!
//! 이진(BlackWhite)은 보정한 휘도에 Sauvola 적응 문턱을 쓴다. 지역 평균과 표준편차는 축소 영상에서
//! 구해 늘린다(문턱은 천천히 변하는 값이라 차이가 거의 없고 4800만 화소에서도 빠르다).
//!
//! @see docs/vision.md

use image::RgbImage;
use leaf_core::processing::Filter;
use rayon::prelude::*;

use crate::util::{Plane, box_blur, downscale_area, fit_long_side, luma, luma_of};

/// 긴 변 기준 배경 블록 수
const BACKGROUND_BLOCKS: u32 = 64;
/// 보정 뒤 수준 맞춤. 종이 잡티는 흰색으로, 잉크는 더 진하게
const BLACK_POINT: f32 = 22.0;
const WHITE_POINT: f32 = 246.0;
/// Sauvola 계수. 보정 뒤라 배경이 평평해 일반 문서보다 조금 낮게 둔다
const SAUVOLA_K: f32 = 0.18;
const SAUVOLA_R: f32 = 128.0;

pub fn apply_filter(img: &RgbImage, filter: Filter) -> RgbImage {
    match filter {
        Filter::Original => img.clone(),
        Filter::Enhanced => enhance(img),
        Filter::Grayscale => to_gray_rgb(&enhance(img)),
        Filter::BlackWhite => binarize(&enhance(img)),
    }
}

/// 종이 배경 추정. 블록 중심 격자에 RGB를 담고 양선형으로 읽는다
pub struct Background {
    cols: usize,
    rows: usize,
    block: f32,
    /// 채널별 격자
    grid: [Plane; 3],
}

impl Background {
    pub fn estimate(img: &RgbImage) -> Background {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let block = (w.max(h) as f32 / BACKGROUND_BLOCKS as f32).max(8.0);
        let cols = (w as f32 / block).ceil() as usize;
        let rows = (h as f32 / block).ceil() as usize;
        let raw = img.as_raw();

        // 블록마다 (밝은 화소 평균색, 중앙값/상위값 비)
        let stats: Vec<([f32; 3], f32, f32)> = (0..rows * cols)
            .into_par_iter()
            .map(|i| {
                let (bx, by) = (i % cols, i / cols);
                let x0 = (bx as f32 * block) as usize;
                let x1 = (((bx + 1) as f32 * block) as usize).min(w);
                let y0 = (by as f32 * block) as usize;
                let y1 = (((by + 1) as f32 * block) as usize).min(h);
                let mut hist = [0u32; 256];
                for y in y0..y1 {
                    for p in raw[(y * w + x0) * 3..(y * w + x1) * 3].as_chunks::<3>().0.iter() {
                        hist[luma_of([p[0], p[1], p[2]]) as usize] += 1;
                    }
                }
                let n: u32 = hist.iter().sum();
                let p50 = hist_percentile(&hist, n, 0.5);
                let p92 = hist_percentile(&hist, n, 0.92);
                let mut acc = [0f64; 3];
                let mut cnt = 0u32;
                for y in y0..y1 {
                    for p in raw[(y * w + x0) * 3..(y * w + x1) * 3].as_chunks::<3>().0.iter() {
                        if luma_of([p[0], p[1], p[2]]) as usize >= p92 {
                            acc[0] += p[0] as f64;
                            acc[1] += p[1] as f64;
                            acc[2] += p[2] as f64;
                            cnt += 1;
                        }
                    }
                }
                let c = cnt.max(1) as f64;
                let color = [(acc[0] / c) as f32, (acc[1] / c) as f32, (acc[2] / c) as f32];
                (color, p50 as f32, p92 as f32)
            })
            .collect();

        let paper = {
            let mut l: Vec<f32> = stats.iter().map(|s| s.2).collect();
            let k = (l.len() as f32 * 0.9) as usize;
            let k = k.min(l.len() - 1);
            l.select_nth_unstable_by(k, f32::total_cmp);
            l[k].max(1.0)
        };
        let is_paper = |s: &([f32; 3], f32, f32), strict: bool| {
            let (c, p50, p92) = *s;
            let chroma = c.iter().copied().fold(f32::MIN, f32::max) - c.iter().copied().fold(f32::MAX, f32::min);
            let uniform = p50 >= 0.72 * p92;
            let bright = p92 >= 0.25 * paper;
            uniform && bright && (!strict || chroma <= 45.0)
        };
        let mut valid: Vec<bool> = stats.iter().map(|s| is_paper(s, true)).collect();
        // 색지 전체가 종이면 채도 조건으로는 하나도 안 남는다. 그때는 채도를 보지 않는다
        if valid.iter().filter(|v| **v).count() < valid.len() / 7 {
            valid = stats.iter().map(|s| is_paper(s, false)).collect();
        }
        if valid.iter().filter(|v| **v).count() == 0 {
            valid.iter_mut().for_each(|v| *v = true);
        }

        let grid = [0, 1, 2].map(|c| {
            let mut p = Plane::new(cols, rows, 0.0);
            for (i, s) in stats.iter().enumerate() {
                p.data[i] = s.0[c];
            }
            let filled = pull_push(&p, &valid);
            box_blur(&box_blur(&filled, 1), 1)
        });
        Background { cols, rows, block, grid }
    }

    /// 화소 (x, y)의 배경색
    #[inline]
    pub fn at(&self, x: f32, y: f32) -> [f32; 3] {
        let gx = ((x + 0.5) / self.block - 0.5).clamp(0.0, (self.cols - 1) as f32);
        let gy = ((y + 0.5) / self.block - 0.5).clamp(0.0, (self.rows - 1) as f32);
        [self.grid[0].sample(gx, gy), self.grid[1].sample(gx, gy), self.grid[2].sample(gx, gy)]
    }
}

fn hist_percentile(hist: &[u32; 256], n: u32, q: f32) -> usize {
    let target = (n as f32 * q) as u32;
    let mut acc = 0;
    for (v, &c) in hist.iter().enumerate() {
        acc += c;
        if acc > target {
            return v;
        }
    }
    255
}

/// 비어 있는 칸을 둘레 값으로 메운다. 절반 크기로 줄여 가며 평균을 내고(pull), 다시 늘리며 빈 칸에 채운다(push)
pub(crate) fn pull_push(p: &Plane, valid: &[bool]) -> Plane {
    if valid.iter().all(|v| *v) || p.width * p.height <= 1 {
        return p.clone();
    }
    let (w, h) = (p.width, p.height);
    let (hw, hh) = (w.div_ceil(2), h.div_ceil(2));
    let mut coarse = Plane::new(hw, hh, 0.0);
    let mut coarse_valid = vec![false; hw * hh];
    for cy in 0..hh {
        for cx in 0..hw {
            let mut acc = 0.0;
            let mut n = 0;
            for y in (cy * 2)..(cy * 2 + 2).min(h) {
                for x in (cx * 2)..(cx * 2 + 2).min(w) {
                    if valid[y * w + x] {
                        acc += p.get(x, y);
                        n += 1;
                    }
                }
            }
            if n > 0 {
                coarse.set(cx, cy, acc / n as f32);
                coarse_valid[cy * hw + cx] = true;
            }
        }
    }
    if !coarse_valid.iter().any(|v| *v) {
        return p.clone();
    }
    let coarse = pull_push(&coarse, &coarse_valid);
    let mut out = p.clone();
    for y in 0..h {
        for x in 0..w {
            if !valid[y * w + x] {
                let v = coarse.sample((x as f32 + 0.5) / 2.0 - 0.5, (y as f32 + 0.5) / 2.0 - 0.5);
                out.set(x, y, v);
            }
        }
    }
    out
}

/// 배경으로 나누고 수준을 맞춘다
pub fn enhance(img: &RgbImage) -> RgbImage {
    let bg = Background::estimate(img);
    let w = img.width() as usize;
    let mut out = img.clone();
    let scale = 255.0 / (WHITE_POINT - BLACK_POINT);
    out.par_chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
        for (x, p) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            let b = bg.at(x as f32, y as f32);
            for c in 0..3 {
                let flat = p[c] as f32 / b[c].max(8.0) * 255.0;
                p[c] = ((flat - BLACK_POINT) * scale).clamp(0.0, 255.0) as u8;
            }
        }
    });
    out
}

fn to_gray_rgb(img: &RgbImage) -> RgbImage {
    let mut out = img.clone();
    out.par_chunks_mut(3).for_each(|p| {
        let v = luma_of([p[0], p[1], p[2]]).round().min(255.0) as u8;
        p.copy_from_slice(&[v, v, v]);
    });
    out
}

/// Sauvola: T = m * (1 + k * (s / R - 1))
fn binarize(img: &RgbImage) -> RgbImage {
    let (w, h) = (img.width(), img.height());
    let (sw, sh) = fit_long_side(w, h, 1200);
    let small = luma(&downscale_area(img, sw, sh));
    let scale = w as f32 / sw as f32;
    // 창 크기는 원본 긴 변의 약 4%(글자 몇 줄)
    let r = ((sw.max(sh) as f32 * 0.02) as usize).max(4);
    let mean = box_blur(&small, r);
    let sq = Plane { width: small.width, height: small.height, data: small.data.iter().map(|v| v * v).collect() };
    let mean_sq = box_blur(&sq, r);
    let mut threshold = Plane::new(small.width, small.height, 0.0);
    for i in 0..threshold.data.len() {
        let m = mean.data[i];
        let s = (mean_sq.data[i] - m * m).max(0.0).sqrt();
        threshold.data[i] = m * (1.0 + SAUVOLA_K * (s / SAUVOLA_R - 1.0));
    }
    let raw = img.as_raw();
    let mut out = RgbImage::new(w, h);
    let wu = w as usize;
    out.par_chunks_mut(wu * 3).enumerate().for_each(|(y, row)| {
        let ty = (y as f32 + 0.5) / scale - 0.5;
        for (x, p) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            let i = (y * wu + x) * 3;
            let v = luma_of([raw[i], raw[i + 1], raw[i + 2]]);
            let t = threshold.sample((x as f32 + 0.5) / scale - 0.5, ty);
            let o = if v > t { 255 } else { 0 };
            p.copy_from_slice(&[o, o, o]);
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    /// 왼쪽이 어두운 그림자와 노란 조명 아래의 줄글 페이지
    fn shaded_page(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| {
            let shade = 0.45 + 0.55 * (x as f32 / w as f32).sqrt();
            let line = (y / 10) % 3 == 0 && (x / 7) % 5 != 0 && x > 30 && x < w - 30;
            let base = if line { [30.0, 30.0, 30.0] } else { [235.0, 225.0, 190.0] };
            Rgb(base.map(|v: f32| (v * shade) as u8))
        })
    }

    fn paper_stats(img: &RgbImage) -> (f32, f32, f32) {
        // 글자 없는 줄의 밝기 평균과 표준편차, 파랑-빨강 차이
        let mut v = Vec::new();
        let mut cast = 0.0;
        for (_, y, p) in img.enumerate_pixels() {
            if (y / 10) % 3 == 1 {
                v.push(luma_of(p.0));
                cast += p.0[2] as f32 - p.0[0] as f32;
            }
        }
        let m = v.iter().sum::<f32>() / v.len() as f32;
        let s = (v.iter().map(|a| (a - m).powi(2)).sum::<f32>() / v.len() as f32).sqrt();
        (m, s, cast / v.len() as f32)
    }

    #[test]
    fn 보정은_그림자와_색을_걷어낸다() {
        let img = shaded_page(600, 800);
        let (m0, s0, c0) = paper_stats(&img);
        let out = apply_filter(&img, Filter::Enhanced);
        let (m1, s1, c1) = paper_stats(&out);
        assert!(m1 > 245.0 && m1 > m0, "종이 밝기 {m0} -> {m1}");
        assert!(s1 < s0 / 4.0, "종이 고르기 {s0} -> {s1}");
        assert!(c1.abs() < 6.0 && c0.abs() > 20.0, "색 치우침 {c0} -> {c1}");
    }

    #[test]
    fn 이진은_글자와_종이만_남긴다() {
        let img = shaded_page(600, 800);
        let out = apply_filter(&img, Filter::BlackWhite);
        let mut wrong = 0;
        let mut total = 0;
        for (x, y, p) in out.enumerate_pixels() {
            let line = (y / 10) % 3 == 0 && (x / 7) % 5 != 0 && x > 30 && x < 570;
            let gap = (y / 10) % 3 == 1;
            if line || gap {
                total += 1;
                let black = p.0[0] == 0;
                if black != line {
                    wrong += 1;
                }
            }
            assert!(p.0[0] == 0 || p.0[0] == 255);
        }
        assert!((wrong as f32) < total as f32 * 0.03, "틀린 화소 {wrong}/{total}");
    }

    #[test]
    fn 원본은_그대로다() {
        let img = shaded_page(64, 64);
        assert_eq!(apply_filter(&img, Filter::Original), img);
    }

    #[test]
    fn 빈_칸을_메운다() {
        let mut p = Plane::new(4, 4, 100.0);
        let mut valid = vec![true; 16];
        p.set(1, 1, 0.0);
        valid[5] = false;
        let f = pull_push(&p, &valid);
        assert!((f.get(1, 1) - 100.0).abs() < 1e-3);
    }
}

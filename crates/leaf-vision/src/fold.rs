//! 펼친 3단 접지 팜플렛을 세 면으로 나눈다.
//!
//! 두 페이지 분리(`split.rs`)의 접힌 선 찾기를 넓힌 것이다. 다른 점은 셋이다.
//! 1. 선이 두 개다. 1/3, 2/3 근처 두 구간에서 따로 찾는다. 안쪽으로 접혀 들어가는 면은 2~3mm 좁아 면 폭이 정확히 같지 않다
//! 2. 단서가 약하다. 책 가운데 같은 넓은 그림자 골짜기가 없고, 접은 자국(가는 어두운 선이나 밝은 선)과
//!    면마다 빛을 받는 각도가 달라 생기는 밝기 계단이 전부다. 글자 화소는 빼고 종이 밝기만 단면으로 낸다
//! 3. 원근이 남아 두 선의 기울기가 다를 수 있다(사다리꼴). 선마다 기울기를 따로 찾는다
//!
//! 사진(팜플렛 그림)의 가장자리도 밝기 계단을 만든다. 접은 자국은 위에서 아래까지 이어지므로,
//! 높이를 네 띠로 나눠 띠마다 점수를 내고 네 띠 중 세 띠 이상에서 보이는 선만 믿는다.
//! 그림자도 자국도 없는 사진은 두 면 여백이 만나는 글자 없는 세로 띠로 찾는다(`split.rs`와 같은 방법).
//!
//! 두 선을 다 확신하지 못하면 나누지 않고 `Trifold::Whole`을 돌려준다.
//!
//! @see docs/vision.md

use image::RgbImage;
use rayon::prelude::*;

use crate::perspective::{Corners, Interpolation, PixelPoint, warp_to_size};
use crate::split::percentile;
use crate::util::{Plane, downscale_area, fit_long_side, luma};

/// 이보다 낮으면 나누지 않는다
pub const MIN_FOLD_CONFIDENCE: f32 = 0.35;
/// 원근 보정 뒤 남는 기울기 범위(도)
const MAX_TILT_DEG: f32 = 6.0;
/// 접은 자국은 2~3화소 폭이라 기울기를 촘촘히 본다(높이 700화소에서 0.25도 = 끝에서 1.5화소)
const TILT_STEP_DEG: f32 = 0.25;
const ANALYSIS_LONG_SIDE: u32 = 1000;
/// 높이를 나누는 띠 수. 접은 자국은 이 중 `BANDS - 1`개 이상에서 보여야 한다
const BANDS: usize = 4;
/// 접힌 선을 찾는 구간(폭 대비). 첫 선은 1/3, 둘째 선은 2/3 근처
const LEFT_RANGE: (f32, f32) = (0.2, 0.47);
const RIGHT_RANGE: (f32, f32) = (0.53, 0.8);

/// 원본 해상도 기준 접힌 선 하나. 위쪽 끝 x와 아래쪽 끝 x로 기울기를 표현한다
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fold {
    pub x_top: f32,
    pub x_bottom: f32,
    /// 0~1
    pub confidence: f32,
    /// 접은 자국 세기(종이 밝기 대비). 골짜기, 밝은 선, 밝기 계단 중 가장 큰 것
    pub crease: f32,
    /// 글자 없는 띠 폭(이미지 폭 대비 0~1)
    pub blank_width: f32,
}

impl Fold {
    pub fn center_x(&self) -> f32 {
        (self.x_top + self.x_bottom) / 2.0
    }

    pub fn angle_deg(&self, height: u32) -> f32 {
        ((self.x_bottom - self.x_top) / height as f32).atan().to_degrees()
    }
}

/// 두 접힌 선. `confidence`는 두 선 중 약한 쪽에 가로세로 비와 면 폭 조건을 곱한 값
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Folds {
    pub left: Fold,
    pub right: Fold,
    pub confidence: f32,
}

impl Folds {
    /// 왼쪽부터 세 면의 폭(가운데 높이 기준, 이미지 폭 대비)
    pub fn panel_widths(&self, width: u32) -> [f32; 3] {
        let w = width as f32;
        let (a, b) = (self.left.center_x(), self.right.center_x());
        [a / w, (b - a) / w, (w - b) / w]
    }
}

#[derive(Debug, Clone)]
pub enum Trifold {
    /// 왼쪽부터 세 면
    Panels { panels: [RgbImage; 3], folds: Folds },
    /// 접힌 선을 확신하지 못했다. 원본을 통째 한 장으로 쓴다. folds는 가장 그럴듯했던 후보(진단용)
    Whole { folds: Folds },
}

/// 접힌 선 두 개를 찾는다. 신뢰도와 상관없이 가장 그럴듯한 후보를 돌려준다
pub fn find_folds(img: &RgbImage) -> Folds {
    let (aw, ah) = fit_long_side(img.width(), img.height(), ANALYSIS_LONG_SIDE);
    let small = downscale_area(img, aw, ah);
    let g = luma(&small);
    let (w, h) = (g.width, g.height);
    let paper = percentile(&g.data, 0.9).max(1.0);

    let tilts: Vec<f32> = {
        let n = (MAX_TILT_DEG / TILT_STEP_DEG).round() as i32;
        (-n..=n).map(|i| i as f32 * TILT_STEP_DEG).collect()
    };
    let profiles: Vec<TiltProfile> = tilts.par_iter().map(|&deg| tilt_profile(&g, deg, paper)).collect();

    let left = best_fold(&profiles, w, LEFT_RANGE, 1.0 / 3.0);
    let right = best_fold(&profiles, w, RIGHT_RANGE, 2.0 / 3.0);

    let scale_x = img.width() as f32 / w as f32;
    let to_fold = |c: FoldCandidate| {
        let tan = c.deg.to_radians().tan();
        let half = img.height() as f32 / 2.0;
        let cx = (c.x + 0.5) * scale_x - 0.5;
        Fold {
            x_top: cx - half * tan,
            x_bottom: cx + half * tan,
            confidence: c.confidence,
            crease: c.crease,
            blank_width: c.blank_width,
        }
    };
    let (left, right) = (to_fold(left), to_fold(right));

    // 펼친 3단 접지는 가로가 길다(A4 접지 1.41). 세로로 긴 사진은 접지가 아닐 가능성이 크다
    let aspect = w as f32 / h as f32;
    let aspect_factor = ((aspect - 0.85) / 0.35).clamp(0.2, 1.0);
    let widths = Folds { left, right, confidence: 0.0 }.panel_widths(img.width());
    let widths_ok = widths.iter().all(|&p| (0.2..=0.46).contains(&p));
    let width_factor = if widths_ok { 1.0 } else { 0.3 };
    let confidence = left.confidence.min(right.confidence) * aspect_factor * width_factor;
    Folds { left, right, confidence }
}

/// 접힌 선 두 개를 찾아 세 면으로 나눈다. 신뢰도가 낮으면 나누지 않는다
pub fn split_trifold(img: &RgbImage) -> Trifold {
    let folds = find_folds(img);
    if folds.confidence < MIN_FOLD_CONFIDENCE {
        return Trifold::Whole { folds };
    }
    Trifold::Panels { panels: cut_panels(img, &folds), folds }
}

/// 주어진 두 선으로 자른다. 선이 기울었으면 면마다 네 꼭짓점(위아래 선 끝)을 직사각형으로 편다
pub fn cut_panels(img: &RgbImage, folds: &Folds) -> [RgbImage; 3] {
    let (w, h) = (img.width(), img.height());
    let wf = w as f32;
    let clamp = |x: f32| x.clamp(1.0, wf - 1.0);
    let (lt, lb) = (clamp(folds.left.x_top), clamp(folds.left.x_bottom));
    let (rt, rb) = (clamp(folds.right.x_top).max(lt + 1.0), clamp(folds.right.x_bottom).max(lb + 1.0));
    let straight = (lt - lb).abs() < 0.5 && (rt - rb).abs() < 0.5;
    if straight {
        let a = folds.left.center_x().round().clamp(1.0, wf - 2.0) as u32;
        let b = (folds.right.center_x().round() as u32).clamp(a + 1, w - 1);
        let crop = |x0: u32, x1: u32| image::imageops::crop_imm(img, x0, 0, x1 - x0, h).to_image();
        return [crop(0, a), crop(a, b), crop(b, w)];
    }
    let hf = h as f32;
    let panel = |x0t: f32, x1t: f32, x1b: f32, x0b: f32| -> RgbImage {
        let p = |x: f32, y: f32| PixelPoint::new(x as f64, y as f64);
        let corners = Corners {
            top_left: p(x0t, 0.0),
            top_right: p(x1t, 0.0),
            bottom_right: p(x1b, hf),
            bottom_left: p(x0b, hf),
        };
        let width = ((x1t - x0t + x1b - x0b) / 2.0).round().max(1.0) as u32;
        // 네 꼭짓점이 사다리꼴이라 퇴화하지 않는다. 혹시 실패하면 바깥 상자를 자른다
        warp_to_size(img, &corners, width, h, Interpolation::Bicubic).unwrap_or_else(|_| {
            let x0 = x0t.min(x0b).max(0.0) as u32;
            let x1 = (x1t.max(x1b) as u32).clamp(x0 + 1, w);
            image::imageops::crop_imm(img, x0, 0, x1 - x0, h).to_image()
        })
    };
    [panel(0.0, lt, lb, 0.0), panel(lt, rt, rb, lb), panel(rt, wf, wf, rb)]
}

/// 기울기 하나에서 낸 단면들
struct TiltProfile {
    deg: f32,
    /// 띠마다 접은 자국 점수를 낸 뒤 네 띠 중 둘째로 약한 값(종이 밝기 대비)
    crease: Vec<f32>,
    /// 글자 화소 비율(높이 전체)
    ink: Vec<f32>,
}

#[derive(Debug, Clone, Copy)]
struct FoldCandidate {
    deg: f32,
    /// 분석 해상도 기준, 가운데 높이에서의 x
    x: f32,
    confidence: f32,
    crease: f32,
    blank_width: f32,
}

fn tilt_profile(g: &Plane, deg: f32, paper: f32) -> TiltProfile {
    let (w, h) = (g.width, g.height);
    let tan = deg.to_radians().tan();
    let ink_level = paper * 0.6;
    let y0 = h / 20;
    let y1 = h - h / 20;
    let band_h = ((y1 - y0) / BANDS).max(1);
    let cy = h as f32 / 2.0;
    // 열마다 띠별 종이 밝기 평균(글자 화소는 뺀다)과 전체 글자 비율
    let columns: Vec<([f32; BANDS], f32)> = (0..w)
        .into_par_iter()
        .map(|x| {
            let mut sum = [0.0f32; BANDS];
            let mut count = [0u32; BANDS];
            let mut total = [0u32; BANDS];
            let mut ink = 0u32;
            let mut n = 0u32;
            for y in y0..y1 {
                let sx = (x as f32 + (y as f32 - cy) * tan).round();
                if sx < 0.0 || sx >= w as f32 {
                    continue;
                }
                let b = ((y - y0) / band_h).min(BANDS - 1);
                let v = g.get(sx as usize, y);
                total[b] += 1;
                n += 1;
                if v < ink_level {
                    ink += 1;
                } else {
                    sum[b] += v;
                    count[b] += 1;
                }
            }
            let mut means = [f32::NAN; BANDS];
            for b in 0..BANDS {
                // 글자로 거의 덮인 띠는 종이 밝기를 믿을 수 없다
                if total[b] > 0 && count[b] * 3 >= total[b] {
                    means[b] = sum[b] / count[b] as f32;
                }
            }
            let ink = if n == 0 { f32::NAN } else { ink as f32 / n as f32 };
            (means, ink)
        })
        .collect();
    let r = (w / 120).max(2);
    let mut band_scores = vec![[0.0f32; BANDS]; w];
    for b in 0..BANDS {
        let p: Vec<f32> = columns.iter().map(|c| c.0[b]).collect();
        let p = smooth(&p, 1);
        for (x, score) in band_scores.iter_mut().enumerate() {
            score[b] = crease_score(&p, x, r) / paper;
        }
    }
    let crease = band_scores
        .into_iter()
        .map(|mut s| {
            s.sort_by(f32::total_cmp);
            // 한 띠는 그림이나 굵은 제목에 가려도 된다
            s[1]
        })
        .collect();
    TiltProfile { deg, crease, ink: columns.iter().map(|c| c.1).collect() }
}

/// 접은 자국 점수: 가는 골짜기(어두운 자국), 가는 봉우리(빛 받은 자국), 밝기 계단(면마다 다른 기울기) 중 큰 값
fn crease_score(p: &[f32], x: usize, r: usize) -> f32 {
    let w = p.len();
    let v = p[x];
    if !v.is_finite() || x < r || x + r >= w {
        return 0.0;
    }
    let left = &p[x - r..x];
    let right = &p[x + 1..=x + r];
    let (Some(lmax), Some(rmax), Some(lmean), Some(rmean)) = (fmax(left), fmax(right), fmean(left), fmean(right)) else {
        return 0.0;
    };
    let valley = (lmax.min(rmax) - v).max(0.0);
    let ridge = (v - lmean.max(rmean)).max(0.0);
    let step = (lmean - rmean).abs() * 0.6;
    valley.max(ridge).max(step)
}

fn fmax(s: &[f32]) -> Option<f32> {
    s.iter().copied().filter(|v| v.is_finite()).reduce(f32::max)
}

fn fmean(s: &[f32]) -> Option<f32> {
    let vals: Vec<f32> = s.iter().copied().filter(|v| v.is_finite()).collect();
    (vals.len() * 2 >= s.len()).then(|| vals.iter().sum::<f32>() / vals.len() as f32)
}

/// 한 구간에서 가장 그럴듯한 접힌 선. expected는 면 폭이 같을 때의 위치(폭 대비)
fn best_fold(profiles: &[TiltProfile], w: usize, range: (f32, f32), expected: f32) -> FoldCandidate {
    let lo = (range.0 * w as f32) as usize;
    let hi = ((range.1 * w as f32) as usize).min(w);
    let prior = |x: usize| 1.0 - 0.5 * ((x as f32 / w as f32 - expected).abs() / 0.15).min(1.0);
    let candidates: Vec<(FoldCandidate, f32)> = profiles
        .iter()
        .map(|t| {
            let (cx, cs) = (lo..hi)
                .map(|x| (x, t.crease[x] * prior(x)))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap_or((w / 2, 0.0));
            let crease = t.crease[cx];
            let (gs, ge) = widest_blank(&t.ink, lo, hi);
            let blank_width = (ge - gs) as f32 / w as f32;
            let crease_conf = ((crease - 0.012) / 0.04).clamp(0.0, 1.0);
            let blank_conf = ((blank_width - 0.012) / 0.03).clamp(0.0, 1.0) * 0.8;
            let in_band = cx >= gs && cx < ge;
            // 자국이 뚜렷하면 자국을 믿는다(면 여백은 양쪽이 다를 수 있다). 약하면 빈 띠 가운데
            let x = if crease_conf >= 0.5 || blank_conf < 0.3 || (in_band && crease_conf >= 0.25) {
                cx as f32
            } else {
                (gs + ge) as f32 / 2.0 - 0.5
            };
            let (strong, weak) = (crease_conf.max(blank_conf), crease_conf.min(blank_conf));
            let confidence = (strong + 0.25 * weak).min(1.0);
            (FoldCandidate { deg: t.deg, x, confidence, crease, blank_width }, cs)
        })
        .collect();
    // 신뢰도가 같으면(둘 다 꽉 찼으면) 자국 점수가 큰 기울기, 그것도 같으면 덜 기운 쪽
    candidates
        .into_iter()
        .max_by(|(a, sa), (b, sb)| {
            a.confidence
                .total_cmp(&b.confidence)
                .then(sa.total_cmp(sb))
                .then((-a.deg.abs()).total_cmp(&-b.deg.abs()))
        })
        .map(|(c, _)| c)
        .expect("기울기 후보가 있다")
}

/// [lo, hi) 안에 가운데가 있는 잉크 없는 가장 넓은 구간. 이미지 끝에 닿은 구간(바깥 여백)은 뺀다
fn widest_blank(ink: &[f32], lo: usize, hi: usize) -> (usize, usize) {
    let w = ink.len();
    let blank = |x: usize| ink[x].is_finite() && ink[x] < 0.003;
    let mut best = (lo, lo);
    let mut x = 0;
    while x < w {
        if !blank(x) {
            x += 1;
            continue;
        }
        let start = x;
        while x < w && blank(x) {
            x += 1;
        }
        let mid = (start + x) / 2;
        if start > 0 && x < w && (lo..hi).contains(&mid) && x - start > best.1 - best.0 {
            best = (start, x);
        }
    }
    best
}

fn smooth(p: &[f32], r: usize) -> Vec<f32> {
    (0..p.len())
        .map(|i| {
            let a = i.saturating_sub(r);
            let b = (i + r + 1).min(p.len());
            fmean(&p[a..b]).unwrap_or(f32::NAN)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    /// 글자 대신 막대를 늘어놓은 3단 접지. folds는 가운데 높이 기준 x, 기울기는 두 선이 같다.
    /// crease는 접은 자국 어둡기(0~1), tone은 가운데 면 밝기 배율(빛 받는 각도)
    fn trifold(w: u32, h: u32, folds: [f32; 2], tilt_deg: f32, crease: f32, tone: f32) -> RgbImage {
        let tan = tilt_deg.to_radians().tan();
        RgbImage::from_fn(w, h, |x, y| {
            let dy = y as f32 - h as f32 / 2.0;
            let f = folds.map(|f| f + dy * tan);
            let xf = x as f32;
            let (start, end, panel) = if xf < f[0] {
                (0.0, f[0], 0)
            } else if xf < f[1] {
                (f[0], f[1], 1)
            } else {
                (f[1], w as f32, 2)
            };
            let pw = end - start;
            let local = xf - start;
            let in_text = local > pw * 0.1 && local < pw * 0.9 && y > h / 12 && y < h * 11 / 12;
            let line = (y / 12) % 2 == 0 && ((x + (y / 24) * 23) / 9) % 6 != 0;
            let mut v = if in_text && line { 40.0 } else { 235.0 };
            if panel == 1 {
                v *= tone;
            }
            let d = f.iter().map(|fx| (xf - fx).abs()).fold(f32::MAX, f32::min);
            v *= 1.0 - crease * (-d * d / 2.0).exp();
            let v = v.clamp(0.0, 255.0) as u8;
            Rgb([v, v, (v as f32 * 0.97) as u8])
        })
    }

    #[test]
    fn 접은_자국이_있는_3단_접지를_나눈다() {
        let img = trifold(1500, 1060, [505.0, 1010.0], 0.0, 0.25, 1.0);
        let Trifold::Panels { panels, folds } = split_trifold(&img) else { panic!("나눠야 한다") };
        assert!((folds.left.center_x() - 505.0).abs() < 5.0, "{folds:?}");
        assert!((folds.right.center_x() - 1010.0).abs() < 5.0, "{folds:?}");
        assert_eq!(panels.iter().map(|p| p.width()).sum::<u32>(), 1500);
    }

    #[test]
    fn 면_밝기_계단만_있어도_찾는다() {
        // 자국 없이 가운데 면만 7% 어둡다(빛 받는 각도 차이)
        let img = trifold(1500, 1060, [490.0, 1000.0], 0.0, 0.0, 0.93);
        let folds = find_folds(&img);
        assert!(folds.confidence >= MIN_FOLD_CONFIDENCE, "{folds:?}");
        assert!((folds.left.center_x() - 490.0).abs() < 8.0, "{folds:?}");
        assert!((folds.right.center_x() - 1000.0).abs() < 8.0, "{folds:?}");
    }

    #[test]
    fn 기울어진_접힌_선을_찾는다() {
        let img = trifold(1500, 1060, [500.0, 1000.0], 2.5, 0.2, 1.0);
        let Trifold::Panels { panels, folds } = split_trifold(&img) else { panic!("나눠야 한다") };
        assert!((folds.left.angle_deg(1060) - 2.5).abs() < 0.5, "{folds:?}");
        assert!((folds.right.center_x() - 1000.0).abs() < 6.0, "{folds:?}");
        assert!(panels.iter().all(|p| p.height() == 1060));
    }

    #[test]
    fn 자국이_없으면_빈_띠로_찾는다() {
        let img = trifold(1500, 1060, [500.0, 1000.0], 0.0, 0.0, 1.0);
        let folds = find_folds(&img);
        assert!(folds.confidence >= MIN_FOLD_CONFIDENCE, "{folds:?}");
        assert!((folds.left.center_x() - 500.0).abs() < 20.0, "{folds:?}");
    }

    #[test]
    fn 글자가_이어지는_한_장은_나누지_않는다() {
        let img = RgbImage::from_fn(1500, 1060, |x, y| {
            let in_text = x > 100 && x < 1400 && y > 90 && y < 970;
            let line = (y / 12) % 2 == 0 && ((x + (y / 24) * 23) / 9) % 6 != 0;
            if in_text && line { Rgb([40, 40, 40]) } else { Rgb([235, 235, 230]) }
        });
        assert!(matches!(split_trifold(&img), Trifold::Whole { .. }));
    }

    #[test]
    fn 빈_종이는_나누지_않는다() {
        assert!(matches!(split_trifold(&RgbImage::from_pixel(900, 640, Rgb([240; 3]))), Trifold::Whole { .. }));
    }
}

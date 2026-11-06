//! 여러 단계가 같이 쓰는 이미지 도구. 큰 사진(1200만~4800만 화소)을 다루므로 행 단위로 rayon 병렬 처리한다.
//!
//! @see docs/vision.md

use image::{GrayImage, Luma, RgbImage};
use rayon::prelude::*;

/// f32 한 채널 영상. 값 범위는 쓰는 쪽이 정한다(보통 0~255)
#[derive(Debug, Clone)]
pub struct Plane {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl Plane {
    pub fn new(width: usize, height: usize, value: f32) -> Self {
        Self { width, height, data: vec![value; width * height] }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.width + x]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, v: f32) {
        self.data[y * self.width + x] = v;
    }

    /// 경계 밖은 가장자리 값으로 본다
    pub fn sample(&self, x: f32, y: f32) -> f32 {
        let x = x.clamp(0.0, (self.width - 1) as f32);
        let y = y.clamp(0.0, (self.height - 1) as f32);
        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let fx = x - x0 as f32;
        let fy = y - y0 as f32;
        let top = self.get(x0, y0) * (1.0 - fx) + self.get(x1, y0) * fx;
        let bottom = self.get(x0, y1) * (1.0 - fx) + self.get(x1, y1) * fx;
        top * (1.0 - fy) + bottom * fy
    }

    /// 양선형 확대 또는 축소. 축소 비율이 크면 먼저 `downscale_area`를 쓴다
    pub fn resize(&self, width: usize, height: usize) -> Plane {
        let sx = self.width as f32 / width as f32;
        let sy = self.height as f32 / height as f32;
        let mut out = Plane::new(width, height, 0.0);
        out.data.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
            let fy = (y as f32 + 0.5) * sy - 0.5;
            for (x, v) in row.iter_mut().enumerate() {
                *v = self.sample((x as f32 + 0.5) * sx - 0.5, fy);
            }
        });
        out
    }

    pub fn to_gray(&self) -> GrayImage {
        GrayImage::from_fn(self.width as u32, self.height as u32, |x, y| {
            Luma([self.get(x as usize, y as usize).clamp(0.0, 255.0) as u8])
        })
    }
}

/// BT.601 휘도(0~255)
#[inline]
pub fn luma_of(p: [u8; 3]) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

pub fn luma(img: &RgbImage) -> Plane {
    let w = img.width() as usize;
    let mut out = Plane::new(w, img.height() as usize, 0.0);
    let raw = img.as_raw();
    out.data.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let src = &raw[y * w * 3..(y + 1) * w * 3];
        for (v, p) in row.iter_mut().zip(src.as_chunks::<3>().0.iter()) {
            *v = luma_of([p[0], p[1], p[2]]);
        }
    });
    out
}

/// 면적 평균 축소. 큰 사진을 모델 입력 크기로 줄일 때 쓴다. 확대에 쓰면 최근접과 같다
pub fn downscale_area(img: &RgbImage, width: u32, height: u32) -> RgbImage {
    let (sw, sh) = (img.width() as usize, img.height() as usize);
    let (dw, dh) = (width as usize, height as usize);
    let raw = img.as_raw();
    let mut out = vec![0u8; dw * dh * 3];
    out.par_chunks_mut(dw * 3).enumerate().for_each(|(y, row)| {
        let y0 = y * sh / dh;
        let y1 = ((y + 1) * sh / dh).max(y0 + 1).min(sh);
        for x in 0..dw {
            let x0 = x * sw / dw;
            let x1 = ((x + 1) * sw / dw).max(x0 + 1).min(sw);
            let mut acc = [0u32; 3];
            for yy in y0..y1 {
                let line = &raw[(yy * sw + x0) * 3..(yy * sw + x1) * 3];
                for p in line.as_chunks::<3>().0.iter() {
                    acc[0] += p[0] as u32;
                    acc[1] += p[1] as u32;
                    acc[2] += p[2] as u32;
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            for c in 0..3 {
                row[x * 3 + c] = ((acc[c] + n / 2) / n) as u8;
            }
        }
    });
    RgbImage::from_raw(width, height, out).expect("크기 계산이 맞다")
}

/// 긴 변이 `long_side` 이하가 되게 줄인 크기. 이미 작으면 그대로
pub fn fit_long_side(width: u32, height: u32, long_side: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= long_side {
        return (width, height);
    }
    let s = long_side as f64 / long as f64;
    (((width as f64 * s).round() as u32).max(1), ((height as f64 * s).round() as u32).max(1))
}

/// 양선형 RGB 표본. 경계 밖은 가장자리 값
#[inline]
pub fn sample_rgb(raw: &[u8], width: usize, height: usize, x: f32, y: f32) -> [f32; 3] {
    let x = x.clamp(0.0, (width - 1) as f32);
    let y = y.clamp(0.0, (height - 1) as f32);
    let x0 = x as usize;
    let y0 = y as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let i00 = (y0 * width + x0) * 3;
    let i10 = (y0 * width + x1) * 3;
    let i01 = (y1 * width + x0) * 3;
    let i11 = (y1 * width + x1) * 3;
    let mut out = [0.0; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let top = raw[i00 + c] as f32 * (1.0 - fx) + raw[i10 + c] as f32 * fx;
        let bottom = raw[i01 + c] as f32 * (1.0 - fx) + raw[i11 + c] as f32 * fx;
        *o = top * (1.0 - fy) + bottom * fy;
    }
    out
}

/// 누적합 상자 흐림(반지름 r, 한 변 2r+1). 가로, 세로를 따로 돌린다
pub fn box_blur(src: &Plane, r: usize) -> Plane {
    if r == 0 {
        return src.clone();
    }
    let (w, h) = (src.width, src.height);
    let mut tmp = Plane::new(w, h, 0.0);
    tmp.data.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        blur_line(&src.data[y * w..(y + 1) * w], row, r);
    });
    // 세로는 전치해서 같은 함수를 쓴다
    let transposed = transpose(&tmp);
    let mut out_t = Plane::new(h, w, 0.0);
    out_t.data.par_chunks_mut(h).enumerate().for_each(|(x, col)| {
        blur_line(&transposed.data[x * h..(x + 1) * h], col, r);
    });
    transpose(&out_t)
}

fn blur_line(src: &[f32], dst: &mut [f32], r: usize) {
    let n = src.len();
    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0.0f64);
    let mut acc = 0.0f64;
    for &v in src {
        acc += v as f64;
        prefix.push(acc);
    }
    for (i, d) in dst.iter_mut().enumerate() {
        let a = i.saturating_sub(r);
        let b = (i + r + 1).min(n);
        *d = ((prefix[b] - prefix[a]) / (b - a) as f64) as f32;
    }
}

fn transpose(src: &Plane) -> Plane {
    let (w, h) = (src.width, src.height);
    let mut out = Plane::new(h, w, 0.0);
    out.data.par_chunks_mut(h).enumerate().for_each(|(x, col)| {
        for (y, v) in col.iter_mut().enumerate() {
            *v = src.data[y * w + x];
        }
    });
    out
}

/// 이진 마스크(0 또는 255)를 정사각형 커널로 팽창
pub fn dilate(mask: &GrayImage, r: u32) -> GrayImage {
    morph(mask, r, true)
}

/// 이진 마스크 침식
pub fn erode(mask: &GrayImage, r: u32) -> GrayImage {
    morph(mask, r, false)
}

fn morph(mask: &GrayImage, r: u32, grow: bool) -> GrayImage {
    if r == 0 {
        return mask.clone();
    }
    let plane = Plane {
        width: mask.width() as usize,
        height: mask.height() as usize,
        data: mask.as_raw().iter().map(|&v| if v > 127 { 1.0 } else { 0.0 }).collect(),
    };
    let blurred = box_blur(&plane, r as usize);
    let mut out = GrayImage::new(mask.width(), mask.height());
    for (o, &v) in out.iter_mut().zip(&blurred.data) {
        let on = if grow { v > 1e-4 } else { v > 1.0 - 1e-4 };
        *o = if on { 255 } else { 0 };
    }
    out
}

/// 4연결 성분. 성분마다 화소 목록을 돌려준다
pub fn components(mask: &GrayImage) -> Vec<Vec<(u32, u32)>> {
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    let raw = mask.as_raw();
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if seen[start] || raw[start] <= 127 {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut pixels = Vec::new();
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            pixels.push((x as u32, y as u32));
            let mut visit = |j: usize| {
                if !seen[j] && raw[j] > 127 {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        out.push(pixels);
    }
    out
}

/// 마스크를 다른 크기로 늘리거나 줄인다(최근접 후 0.5 문턱)
pub fn resize_mask(mask: &GrayImage, width: u32, height: u32) -> GrayImage {
    if mask.dimensions() == (width, height) {
        return mask.clone();
    }
    let plane = Plane {
        width: mask.width() as usize,
        height: mask.height() as usize,
        data: mask.as_raw().iter().map(|&v| v as f32).collect(),
    };
    let resized = plane.resize(width as usize, height as usize);
    let mut out = GrayImage::new(width, height);
    for (o, &v) in out.iter_mut().zip(&resized.data) {
        *o = if v >= 127.5 { 255 } else { 0 };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 상자_흐림은_평균을_보존한다() {
        let mut p = Plane::new(50, 40, 10.0);
        p.set(25, 20, 1010.0);
        let b = box_blur(&p, 2);
        let sum: f32 = b.data.iter().sum();
        assert!((sum - (50.0 * 40.0 * 10.0 + 1000.0)).abs() < 1.0);
        assert!((b.get(25, 20) - (10.0 + 1000.0 / 25.0)).abs() < 1e-3);
    }

    #[test]
    fn 면적_축소는_평균색을_낸다() {
        let img = RgbImage::from_fn(4, 4, |x, _| if x < 2 { image::Rgb([0, 0, 0]) } else { image::Rgb([200, 100, 50]) });
        let s = downscale_area(&img, 2, 1);
        assert_eq!(s.get_pixel(0, 0).0, [0, 0, 0]);
        assert_eq!(s.get_pixel(1, 0).0, [200, 100, 50]);
    }

    #[test]
    fn 연결_성분을_센다() {
        let mut m = GrayImage::new(10, 10);
        m.put_pixel(1, 1, Luma([255]));
        m.put_pixel(1, 2, Luma([255]));
        m.put_pixel(8, 8, Luma([255]));
        let c = components(&m);
        assert_eq!(c.len(), 2);
    }
}

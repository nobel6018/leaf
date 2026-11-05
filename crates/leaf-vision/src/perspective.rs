//! 원근 보정. 사진 속 문서 테두리 네 꼭짓점을 직사각형 페이지로 편다.
//!
//! 출력 픽셀마다 원본 좌표를 호모그래피로 거꾸로 찾아 보간한다(역방향 사상). 정방향으로 찍으면 구멍이 생긴다.
//! 플랫폼 API(Core Image `CIPerspectiveCorrection`)를 쓰지 않는 이유는 Windows에서도 같은 결과를 내기 위해서다.
//!
//! @see docs/pipeline.md

use image::RgbImage;

/// 픽셀 좌표(원점 왼쪽 위). 원본 사진 기준
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelPoint {
    pub x: f64,
    pub y: f64,
}

impl PixelPoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn distance(self, other: PixelPoint) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

/// 문서 테두리 네 꼭짓점(픽셀). 시계 방향: 왼쪽 위, 오른쪽 위, 오른쪽 아래, 왼쪽 아래
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corners {
    pub top_left: PixelPoint,
    pub top_right: PixelPoint,
    pub bottom_right: PixelPoint,
    pub bottom_left: PixelPoint,
}

impl Corners {
    /// 0~1 정규화 좌표를 이미지 크기에 맞춰 픽셀로 바꾼다
    pub fn from_normalized(points: [(f64, f64); 4], width: u32, height: u32) -> Self {
        let to_pixel = |(x, y): (f64, f64)| PixelPoint::new(x * width as f64, y * height as f64);
        Self {
            top_left: to_pixel(points[0]),
            top_right: to_pixel(points[1]),
            bottom_right: to_pixel(points[2]),
            bottom_left: to_pixel(points[3]),
        }
    }

    pub fn points(&self) -> [PixelPoint; 4] {
        [self.top_left, self.top_right, self.bottom_right, self.bottom_left]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interpolation {
    Bilinear,
    /// Catmull-Rom. 작은 글자의 획이 덜 뭉개진다. 기본값
    #[default]
    Bicubic,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PerspectiveError {
    #[error("꼭짓점이 한 줄에 놓이거나 겹쳐 직사각형으로 펼 수 없다")]
    Degenerate,
}

/// 3x3 호모그래피(행 우선, h[8] = 1)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Homography(pub [f64; 9]);

impl Homography {
    /// `from[i]`를 `to[i]`로 보내는 호모그래피. 8x8 연립방정식을 부분 피벗 가우스 소거로 푼다
    pub fn from_points(from: [PixelPoint; 4], to: [PixelPoint; 4]) -> Result<Self, PerspectiveError> {
        let mut a = [[0.0f64; 9]; 8];
        for i in 0..4 {
            let (x, y) = (from[i].x, from[i].y);
            let (u, v) = (to[i].x, to[i].y);
            a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        for col in 0..8 {
            let pivot = (col..8)
                .max_by(|&p, &q| a[p][col].abs().total_cmp(&a[q][col].abs()))
                .expect("범위가 비어 있지 않다");
            if a[pivot][col].abs() < 1e-12 {
                return Err(PerspectiveError::Degenerate);
            }
            a.swap(col, pivot);
            let pivot_row = a[col];
            for (row_index, row) in a.iter_mut().enumerate() {
                if row_index != col {
                    let factor = row[col] / pivot_row[col];
                    for (value, pivot_value) in row.iter_mut().zip(pivot_row.iter()).skip(col) {
                        *value -= factor * pivot_value;
                    }
                }
            }
        }
        let mut h = [0.0; 9];
        for i in 0..8 {
            h[i] = a[i][8] / a[i][i];
        }
        h[8] = 1.0;
        if h.iter().any(|v| !v.is_finite()) {
            return Err(PerspectiveError::Degenerate);
        }
        Ok(Self(h))
    }

    pub fn apply(&self, p: PixelPoint) -> PixelPoint {
        let h = &self.0;
        let w = h[6] * p.x + h[7] * p.y + h[8];
        PixelPoint::new((h[0] * p.x + h[1] * p.y + h[2]) / w, (h[3] * p.x + h[4] * p.y + h[5]) / w)
    }
}

/// 출력 크기: 마주 보는 두 변 중 긴 쪽. 사진에서 멀리 있는 변이 짧게 찍히므로 긴 쪽이 실제 해상도에 가깝다
pub fn output_size(corners: &Corners) -> (u32, u32) {
    let width = corners
        .top_left
        .distance(corners.top_right)
        .max(corners.bottom_left.distance(corners.bottom_right));
    let height = corners
        .top_left
        .distance(corners.bottom_left)
        .max(corners.top_right.distance(corners.bottom_right));
    (width.round().max(1.0) as u32, height.round().max(1.0) as u32)
}

/// 네 꼭짓점 안쪽을 직사각형으로 편다. 출력 크기는 `output_size`
pub fn warp(image: &RgbImage, corners: &Corners, interpolation: Interpolation) -> Result<RgbImage, PerspectiveError> {
    let (width, height) = output_size(corners);
    warp_to_size(image, corners, width, height, interpolation)
}

/// 출력 크기를 정해서 편다. 출력 직사각형 모서리가 꼭짓점에 정확히 닿는다
pub fn warp_to_size(
    image: &RgbImage,
    corners: &Corners,
    width: u32,
    height: u32,
    interpolation: Interpolation,
) -> Result<RgbImage, PerspectiveError> {
    let (w, h) = (width as f64, height as f64);
    let rect = [
        PixelPoint::new(0.0, 0.0),
        PixelPoint::new(w, 0.0),
        PixelPoint::new(w, h),
        PixelPoint::new(0.0, h),
    ];
    let to_source = Homography::from_points(rect, corners.points())?;
    let mut output = RgbImage::new(width, height);
    let row_bytes = width as usize * 3;
    if row_bytes == 0 {
        return Ok(output);
    }
    // 4800만 화소 사진도 한 장에 1초 안쪽으로 끝나게 줄 단위로 나눠 여러 코어에서 돈다
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16);
    let rows_per_chunk = (height as usize).div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (chunk_index, chunk) in output.chunks_mut(row_bytes * rows_per_chunk).enumerate() {
            let to_source = &to_source;
            scope.spawn(move || {
                for (row_offset, row) in chunk.chunks_mut(row_bytes).enumerate() {
                    let y = (chunk_index * rows_per_chunk + row_offset) as f64 + 0.5;
                    for x in 0..width as usize {
                        let source = to_source.apply(PixelPoint::new(x as f64 + 0.5, y));
                        // 픽셀 중심 좌표계에서 표본 좌표계로(정수 좌표 = 픽셀 중심)
                        let (sx, sy) = (source.x - 0.5, source.y - 0.5);
                        let pixel = match interpolation {
                            Interpolation::Bilinear => sample_bilinear(image, sx, sy),
                            Interpolation::Bicubic => sample_bicubic(image, sx, sy),
                        };
                        row[x * 3..x * 3 + 3].copy_from_slice(&pixel);
                    }
                }
            });
        }
    });
    Ok(output)
}

fn pixel_clamped(image: &RgbImage, x: i64, y: i64) -> [f64; 3] {
    let x = x.clamp(0, image.width() as i64 - 1) as u32;
    let y = y.clamp(0, image.height() as i64 - 1) as u32;
    let p = image.get_pixel(x, y).0;
    [p[0] as f64, p[1] as f64, p[2] as f64]
}

fn to_u8(values: [f64; 3]) -> [u8; 3] {
    values.map(|v| v.round().clamp(0.0, 255.0) as u8)
}

fn sample_bilinear(image: &RgbImage, x: f64, y: f64) -> [u8; 3] {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let p00 = pixel_clamped(image, x0, y0);
    let p10 = pixel_clamped(image, x0 + 1, y0);
    let p01 = pixel_clamped(image, x0, y0 + 1);
    let p11 = pixel_clamped(image, x0 + 1, y0 + 1);
    let mut out = [0.0; 3];
    for c in 0..3 {
        let top = p00[c] * (1.0 - fx) + p10[c] * fx;
        let bottom = p01[c] * (1.0 - fx) + p11[c] * fx;
        out[c] = top * (1.0 - fy) + bottom * fy;
    }
    to_u8(out)
}

/// Catmull-Rom 가중치(a = -0.5)
fn cubic_weight(t: f64) -> f64 {
    let t = t.abs();
    if t < 1.0 {
        1.5 * t.powi(3) - 2.5 * t.powi(2) + 1.0
    } else if t < 2.0 {
        -0.5 * t.powi(3) + 2.5 * t.powi(2) - 4.0 * t + 2.0
    } else {
        0.0
    }
}

fn sample_bicubic(image: &RgbImage, x: f64, y: f64) -> [u8; 3] {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let wx: [f64; 4] = std::array::from_fn(|i| cubic_weight(fx - (i as f64 - 1.0)));
    let wy: [f64; 4] = std::array::from_fn(|i| cubic_weight(fy - (i as f64 - 1.0)));
    let mut out = [0.0; 3];
    for (j, wyj) in wy.iter().enumerate() {
        for (i, wxi) in wx.iter().enumerate() {
            let p = pixel_clamped(image, x0 + i as i64 - 1, y0 + j as i64 - 1);
            let weight = wxi * wyj;
            for c in 0..3 {
                out[c] += p[c] * weight;
            }
        }
    }
    to_u8(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn corners(points: [(f64, f64); 4]) -> Corners {
        Corners {
            top_left: PixelPoint::new(points[0].0, points[0].1),
            top_right: PixelPoint::new(points[1].0, points[1].1),
            bottom_right: PixelPoint::new(points[2].0, points[2].1),
            bottom_left: PixelPoint::new(points[3].0, points[3].1),
        }
    }

    /// 8픽셀 칸 바둑판에 가로 그라데이션을 섞은 합성 페이지
    fn synthetic_page(width: u32, height: u32) -> RgbImage {
        RgbImage::from_fn(width, height, |x, y| {
            let checker = ((x / 8 + y / 8) % 2) as u8;
            let shade = (x * 200 / width.max(1)) as u8;
            if checker == 0 {
                Rgb([shade, 30, 220])
            } else {
                Rgb([240, 240 - shade / 2, 20])
            }
        })
    }

    /// 페이지를 기울어진 사각형 안에 그려 "찍은 사진"을 만든다. 사각형 밖은 회색 책상
    fn photograph(page: &RgbImage, quad: &Corners, canvas: (u32, u32)) -> RgbImage {
        let (pw, ph) = (page.width() as f64, page.height() as f64);
        let rect = [
            PixelPoint::new(0.0, 0.0),
            PixelPoint::new(pw, 0.0),
            PixelPoint::new(pw, ph),
            PixelPoint::new(0.0, ph),
        ];
        let to_page = Homography::from_points(quad.points(), rect).unwrap();
        RgbImage::from_fn(canvas.0, canvas.1, |x, y| {
            let p = to_page.apply(PixelPoint::new(x as f64 + 0.5, y as f64 + 0.5));
            if p.x < 0.0 || p.y < 0.0 || p.x >= pw || p.y >= ph {
                Rgb([90, 90, 90])
            } else {
                Rgb(sample_bicubic(page, p.x - 0.5, p.y - 0.5))
            }
        })
    }

    fn mean_abs_diff(a: &RgbImage, b: &RgbImage, margin: u32) -> f64 {
        assert_eq!(a.dimensions(), b.dimensions());
        let mut sum = 0.0;
        let mut count = 0.0;
        for y in margin..a.height() - margin {
            for x in margin..a.width() - margin {
                for c in 0..3 {
                    sum += (a.get_pixel(x, y)[c] as f64 - b.get_pixel(x, y)[c] as f64).abs();
                    count += 1.0;
                }
            }
        }
        sum / count
    }

    #[test]
    fn 호모그래피는_네_꼭짓점을_정확히_옮긴다() {
        let from = corners([(10.0, 20.0), (300.0, 5.0), (320.0, 410.0), (0.0, 400.0)]).points();
        let to = corners([(0.0, 0.0), (200.0, 0.0), (200.0, 300.0), (0.0, 300.0)]).points();
        let h = Homography::from_points(from, to).unwrap();
        for (f, t) in from.iter().zip(to.iter()) {
            let mapped = h.apply(*f);
            assert!((mapped.x - t.x).abs() < 1e-6 && (mapped.y - t.y).abs() < 1e-6, "{mapped:?} != {t:?}");
        }
    }

    #[test]
    fn 한_줄에_놓인_꼭짓점은_거부한다() {
        let line = corners([(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (30.0, 0.0)]);
        let image = synthetic_page(16, 16);
        assert_eq!(warp(&image, &line, Interpolation::Bilinear), Err(PerspectiveError::Degenerate));
    }

    #[test]
    fn 출력_크기는_마주_보는_변_중_긴_쪽이다() {
        let quad = corners([(0.0, 0.0), (100.0, 0.0), (120.0, 200.0), (-10.0, 200.0)]);
        assert_eq!(output_size(&quad), (130, 201));
    }

    #[test]
    fn 이미지_전체를_꼭짓점으로_주면_원본_그대로다() {
        let page = synthetic_page(64, 48);
        let quad = corners([(0.0, 0.0), (64.0, 0.0), (64.0, 48.0), (0.0, 48.0)]);
        for interpolation in [Interpolation::Bilinear, Interpolation::Bicubic] {
            let out = warp(&page, &quad, interpolation).unwrap();
            assert_eq!(out.dimensions(), (64, 48));
            assert!(mean_abs_diff(&out, &page, 0) < 0.01, "{interpolation:?}");
        }
    }

    #[test]
    fn 기울여_찍은_합성_페이지를_원래대로_편다() {
        let page = synthetic_page(240, 320);
        let quad = corners([(70.0, 40.0), (330.0, 70.0), (360.0, 420.0), (40.0, 390.0)]);
        let photo = photograph(&page, &quad, (400, 460));
        for interpolation in [Interpolation::Bilinear, Interpolation::Bicubic] {
            let restored = warp_to_size(&photo, &quad, 240, 320, interpolation).unwrap();
            // 두 번 보간한 결과라 경계(바둑판 칸 사이)에서만 조금 다르다
            let diff = mean_abs_diff(&restored, &page, 4);
            assert!(diff < 12.0, "{interpolation:?} 평균 차이 {diff}");
        }
    }

    #[test]
    fn 정규화_좌표를_픽셀로_바꾼다() {
        let c = Corners::from_normalized([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], 400, 300);
        assert_eq!(c.bottom_right, PixelPoint::new(400.0, 300.0));
    }
}

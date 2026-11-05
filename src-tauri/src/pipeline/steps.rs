//! 페이지 보정 단계. 단계마다 함수 하나이고, 아직 없는 단계는 입력을 그대로 돌려준다.
//! vision 팀 작업(leaf-vision)이 들어오면 이 파일의 함수 본문만 바꾼다.
//!
//! 순서: 원근 보정 → 두 페이지 분리 → 곡면 보정 → 손가락 지우기 → 필기 지우기 → 조명과 필터
//!
//! @see docs/pipeline.md

use image::RgbImage;
use leaf_core::processing::{Dewarp, Filter, PagePart, ProcessOptions};
use leaf_core::{PageLayout, Quad};
use leaf_vision::perspective::{self, Corners, Interpolation};

/// 출력 페이지 긴 변 상한. A4를 약 340dpi로 담는 크기다. 4800만 화소 원본을 그대로 두면 PDF가 너무 커지고 OCR도 느려진다
pub const MAX_OUTPUT_SIDE: u32 = 4000;

/// 꼭짓점이 사진 모서리에 거의 붙어 있으면 원근 보정을 건너뛴다(감지 실패 대신 사진 전체가 잡힌 경우)
const FULL_FRAME_TOLERANCE: f64 = 0.004;

pub struct CorrectInput<'a> {
    pub image: RgbImage,
    pub quad: Option<Quad>,
    pub layout: PageLayout,
    pub options: &'a ProcessOptions,
}

/// 원본 한 장을 출력 페이지 한 장 또는 두 장으로 만든다
pub fn correct_page(input: CorrectInput<'_>) -> Result<Vec<(PagePart, RgbImage)>, String> {
    let flat = perspective_step(input.image, input.quad)?;
    let mut outputs = Vec::new();
    for (part, image) in split_step(flat, input.layout) {
        let image = dewarp_step(image, input.options.dewarp);
        let image = if input.options.remove_fingers { remove_fingers_step(image) } else { image };
        let image = if input.options.remove_handwriting {
            remove_handwriting_step(image)
        } else {
            image
        };
        let image = filter_step(image, input.options.filter);
        outputs.push((part, image));
    }
    Ok(outputs)
}

/// 1단계: 원근 보정(leaf-vision perspective). 꼭짓점이 없으면 사진 전체를 크기만 맞춰 쓴다
pub fn perspective_step(image: RgbImage, quad: Option<Quad>) -> Result<RgbImage, String> {
    let Some(quad) = quad.filter(|q| !is_full_frame(q)) else {
        return Ok(limit_size(image));
    };
    let points = quad.points().map(|p| (p.x, p.y));
    let corners = Corners::from_normalized(points, image.width(), image.height());
    let (w, h) = perspective::output_size(&corners);
    let (w, h) = fit_within(w, h, MAX_OUTPUT_SIDE);
    perspective::warp_to_size(&image, &corners, w, h, Interpolation::Bicubic).map_err(|e| e.to_string())
}

/// 2단계: 펼친 책을 왼쪽, 오른쪽으로 나눈다.
/// 자리표시: 가운데에서 자른다. vision 팀의 접힌 선 찾기(leaf-vision)로 바꿀 곳
pub fn split_step(image: RgbImage, layout: PageLayout) -> Vec<(PagePart, RgbImage)> {
    match layout {
        PageLayout::Single => vec![(PagePart::Whole, image)],
        PageLayout::Spread => {
            let half = image.width() / 2;
            let left = image::imageops::crop_imm(&image, 0, 0, half, image.height()).to_image();
            let right = image::imageops::crop_imm(&image, half, 0, image.width() - half, image.height()).to_image();
            vec![(PagePart::Left, left), (PagePart::Right, right)]
        }
    }
}

/// 3단계: 곡면 보정. 자리표시(그대로). vision 팀의 UVDoc 계열 모델로 바꿀 곳
pub fn dewarp_step(image: RgbImage, mode: Dewarp) -> RgbImage {
    let _ = mode;
    image
}

/// 4단계: 손가락 지우기. 자리표시(그대로). vision 팀의 손 검출 + 인페인팅으로 바꿀 곳
pub fn remove_fingers_step(image: RgbImage) -> RgbImage {
    image
}

/// 5단계: 필기 지우기. 자리표시(그대로)
pub fn remove_handwriting_step(image: RgbImage) -> RgbImage {
    image
}

/// 6단계: 조명 보정과 필터. 자리표시(그대로). 흑백 필터가 인페인팅보다 먼저 오면 지운 자국이 도드라져 맨 끝에 둔다
pub fn filter_step(image: RgbImage, filter: Filter) -> RgbImage {
    let _ = filter;
    image
}

fn is_full_frame(quad: &Quad) -> bool {
    let near = |a: f64, b: f64| (a - b).abs() < FULL_FRAME_TOLERANCE;
    near(quad.top_left.x, 0.0)
        && near(quad.top_left.y, 0.0)
        && near(quad.top_right.x, 1.0)
        && near(quad.top_right.y, 0.0)
        && near(quad.bottom_right.x, 1.0)
        && near(quad.bottom_right.y, 1.0)
        && near(quad.bottom_left.x, 0.0)
        && near(quad.bottom_left.y, 1.0)
}

fn fit_within(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= max_side {
        return (w, h);
    }
    let scale = max_side as f64 / long as f64;
    (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1))
}

fn limit_size(image: RgbImage) -> RgbImage {
    let (w, h) = fit_within(image.width(), image.height(), MAX_OUTPUT_SIDE);
    if (w, h) == image.dimensions() {
        image
    } else {
        image::imageops::resize(&image, w, h, image::imageops::FilterType::CatmullRom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leaf_core::Point;

    fn quad(points: [(f64, f64); 4]) -> Quad {
        let p = |(x, y): (f64, f64)| Point { x, y };
        Quad {
            top_left: p(points[0]),
            top_right: p(points[1]),
            bottom_right: p(points[2]),
            bottom_left: p(points[3]),
        }
    }

    #[test]
    fn 펼친_페이지는_왼쪽과_오른쪽_두_장이_된다() {
        let image = RgbImage::new(101, 50);
        let options = ProcessOptions::default();
        let outputs = correct_page(CorrectInput {
            image,
            quad: None,
            layout: PageLayout::Spread,
            options: &options,
        })
        .unwrap();
        let parts: Vec<_> = outputs.iter().map(|(p, i)| (*p, i.width())).collect();
        assert_eq!(parts, vec![(PagePart::Left, 50), (PagePart::Right, 51)]);
    }

    #[test]
    fn 꼭짓점_안쪽만_잘라_편다() {
        let image = RgbImage::from_fn(200, 100, |x, _| if (50..150).contains(&x) { image::Rgb([255, 255, 255]) } else { image::Rgb([0, 0, 0]) });
        let out = perspective_step(image, Some(quad([(0.25, 0.0), (0.75, 0.0), (0.75, 1.0), (0.25, 1.0)]))).unwrap();
        assert_eq!(out.dimensions(), (100, 100));
        assert!(out.pixels().all(|p| p.0[0] > 200), "흰 영역만 남아야 한다");
    }

    #[test]
    fn 긴_변은_상한에_맞춰_줄인다() {
        assert_eq!(fit_within(8000, 6000, 4000), (4000, 3000));
        assert_eq!(fit_within(300, 200, 4000), (300, 200));
    }
}

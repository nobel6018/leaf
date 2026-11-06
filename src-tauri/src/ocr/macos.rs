//! Apple Vision 글자 인식. 기기 안에서 돌고 네트워크를 쓰지 않는다.
//!
//! @see docs/ocr.md

use image::RgbImage;
use scanshelf_core::ocr::{OcrLine, Rect};
use objc2::AnyThread;
use objc2::rc::autoreleasepool;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSDictionary, NSString};
use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};

use super::OcrError;
use crate::pipeline::platform::rgb_to_cgimage;

pub const ENGINE: &str = "vision-recognize-text";

/// 한국어를 맨 앞에 둔다. Vision은 앞 언어를 우선해 언어 보정을 한다
const LANGUAGES: [&str; 5] = ["ko-KR", "en-US", "ja-JP", "zh-Hans", "zh-Hant"];

/// 글줄마다 글을 읽는 방향 벡터(이미지 픽셀 기준, y는 위로)와 읽은 글자 무게를 돌려준다.
/// Vision은 돌아간 글자도 읽으므로 "어느 방향으로 돌려야 잘 읽히나"로는 고를 수 없다. 글줄 사각형의
/// 왼쪽 위에서 오른쪽 위로 가는 방향이 곧 읽는 방향이다
pub fn text_directions(image: &RgbImage) -> Result<Vec<(f64, f64, f64)>, OcrError> {
    autoreleasepool(|_| {
        let cgimage = rgb_to_cgimage(image);
        let options = NSDictionary::<NSString, AnyObject>::new();
        let handler =
            unsafe { VNImageRequestHandler::initWithCGImage_options(VNImageRequestHandler::alloc(), &cgimage, &options) };
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        let languages: Vec<_> = LANGUAGES.iter().map(|l| NSString::from_str(l)).collect();
        request.setRecognitionLanguages(&NSArray::from_retained_slice(&languages));
        let as_request: &VNRequest = &request;
        handler
            .performRequests_error(&NSArray::from_slice(&[as_request]))
            .map_err(|e| OcrError::Engine(e.localizedDescription().to_string()))?;
        let Some(results) = request.results() else {
            return Ok(Vec::new());
        };
        let (w, h) = (image.width() as f64, image.height() as f64);
        let mut out = Vec::with_capacity(results.len());
        for observation in results.iter() {
            let candidates = observation.topCandidates(1);
            let Some(best) = candidates.firstObject() else {
                continue;
            };
            let readable = best.string().to_string().chars().filter(|c| c.is_alphanumeric()).count();
            let (left, right) = unsafe { (observation.topLeft(), observation.topRight()) };
            out.push(((right.x - left.x) * w, (right.y - left.y) * h, readable as f64 * best.confidence() as f64));
        }
        Ok(out)
    })
}

pub fn recognize_lines(image: &RgbImage) -> Result<Vec<OcrLine>, OcrError> {
    autoreleasepool(|_| {
        let cgimage = rgb_to_cgimage(image);
        let options = NSDictionary::<NSString, AnyObject>::new();
        let handler =
            unsafe { VNImageRequestHandler::initWithCGImage_options(VNImageRequestHandler::alloc(), &cgimage, &options) };
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        let languages: Vec<_> = LANGUAGES.iter().map(|l| NSString::from_str(l)).collect();
        request.setRecognitionLanguages(&NSArray::from_retained_slice(&languages));
        let as_request: &VNRequest = &request;
        handler
            .performRequests_error(&NSArray::from_slice(&[as_request]))
            .map_err(|e| OcrError::Engine(e.localizedDescription().to_string()))?;
        let Some(results) = request.results() else {
            return Ok(Vec::new());
        };
        let mut lines = Vec::with_capacity(results.len());
        for observation in results.iter() {
            let candidates = observation.topCandidates(1);
            let Some(best) = candidates.firstObject() else {
                continue;
            };
            let b = unsafe { observation.boundingBox() };
            // Vision은 왼쪽 아래 원점이다
            lines.push(OcrLine {
                text: best.string().to_string(),
                bbox: Rect {
                    x: b.origin.x,
                    y: 1.0 - b.origin.y - b.size.height,
                    width: b.size.width,
                    height: b.size.height,
                },
                confidence: best.confidence(),
            });
        }
        Ok(lines)
    })
}

//! 페이지 글자 인식. 엔진은 플랫폼마다 다르고(`macos.rs`, 나중에 `windows.rs`), 블록 묶기는 공통이다.
//!
//! @see docs/ocr.md

pub mod blocks;
#[cfg(target_os = "macos")]
mod macos;

use image::RgbImage;
use scanshelf_core::ocr::OcrPage;

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("글자 인식 실패: {0}")]
    Engine(String),
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    #[error("이 플랫폼은 글자 인식을 아직 지원하지 않는다")]
    Unsupported,
}

/// 처리된 페이지 이미지에서 글자를 읽어 블록으로 묶는다
pub fn recognize(image: &RgbImage) -> Result<OcrPage, OcrError> {
    #[cfg(target_os = "macos")]
    {
        let lines = macos::recognize_lines(image)?;
        Ok(blocks::build_page(macos::ENGINE, image.width(), image.height(), lines))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = image;
        Err(OcrError::Unsupported)
    }
}

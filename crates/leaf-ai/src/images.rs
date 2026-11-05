use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};

use crate::error::AiError;

/// 긴 변이 `max_edge`를 넘으면 줄인다. 작은 이미지는 키우지 않는다
pub(crate) fn load_downscaled(path: &Path, max_edge: u32) -> Result<DynamicImage, AiError> {
    let image_error = |source| AiError::Image {
        path: path.to_path_buf(),
        source,
    };
    let image = image::ImageReader::open(path)
        .map_err(|source| AiError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .with_guessed_format()
        .map_err(|source| AiError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .decode()
        .map_err(image_error)?;
    if image.width().max(image.height()) <= max_edge {
        return Ok(image);
    }
    Ok(image.resize(max_edge, max_edge, FilterType::Triangle))
}

pub(crate) fn encode_jpeg(
    image: &DynamicImage,
    quality: u8,
    path: &Path,
) -> Result<Vec<u8>, AiError> {
    let mut bytes = Vec::new();
    // JPEG에는 알파 채널이 없어 RGB로 바꿔 쓴다
    let rgb = image.to_rgb8();
    JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode_image(&rgb)
        .map_err(|source| AiError::Image {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(bytes)
}

pub(crate) fn encode_png(image: &DynamicImage, path: &Path) -> Result<Vec<u8>, AiError> {
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image.to_rgb8())
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|source| AiError::Image {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(cursor.into_inner())
}

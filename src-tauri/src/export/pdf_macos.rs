//! 텍스트 레이어 PDF(macOS CoreGraphics + CoreText). 페이지 그림 위에 OCR 줄을 보이지 않는 글자로 겹쳐 그린다.
//! 미리보기와 Acrobat에서 한국어 검색과 복사가 된다. CoreText가 글꼴 일부와 ToUnicode 표를 같이 넣어 준다.
//!
//! @see docs/export.md

use std::path::Path;

use leaf_core::ocr::OcrPage;
use objc2_core_foundation::{
    CFDictionary, CFRetained, CFString, CFType, CFURL, CGAffineTransform, CGPoint, CGRect, CGSize, CFAttributedString,
};
use objc2_core_graphics::{
    CGColorRenderingIntent, CGContext, CGDataProvider, CGImage, CGPDFContextClose, CGPDFContextCreateWithURL,
    CGTextDrawingMode, kCGPDFContextCreator, kCGPDFContextTitle,
};
use objc2_core_text::{CTFont, CTLine, kCTFontAttributeName};

use super::PdfPage;

/// 긴 변을 A4 긴 변(842pt)에 맞춘다. 사진만으로는 실제 종이 크기를 알 수 없어 화면과 인쇄에 무난한 크기로 둔다
const PAGE_LONG_SIDE_PT: f64 = 842.0;
/// 맥 기본 한글 글꼴. 보이지 않는 글자라 모양은 상관없고, 한글과 영문 글리프가 다 있어야 한다
const TEXT_FONT: &str = "AppleSDGothicNeo-Regular";

pub fn write_pdf(path: &Path, title: &str, pages: &[PdfPage]) -> Result<(), String> {
    let url = CFURL::from_file_path(path).ok_or("PDF 경로가 올바르지 않다")?;
    let title_value = CFString::from_str(title);
    let creator = CFString::from_static_str("leaf");
    let keys: [&CFString; 2] = unsafe { [kCGPDFContextTitle, kCGPDFContextCreator] };
    let values: [&CFType; 2] = [&title_value, &creator];
    let info = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
    let context = unsafe { CGPDFContextCreateWithURL(Some(&url), std::ptr::null(), Some(info.as_ref())) }
        .ok_or("PDF를 만들지 못했다")?;
    for page in pages {
        draw_page(&context, page)?;
    }
    CGPDFContextClose(Some(&context));
    Ok(())
}

fn load_jpeg(path: &Path) -> Result<CFRetained<CGImage>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{} 읽기 실패: {e}", path.display()))?;
    let data = objc2_core_foundation::CFData::from_bytes(&bytes);
    let provider = CGDataProvider::with_cf_data(Some(&data)).ok_or("데이터 공급자 생성 실패")?;
    // JPEG 공급자로 만든 그림은 PDF에 다시 압축하지 않고 그대로 들어간다(DCTDecode)
    unsafe {
        CGImage::with_jpeg_data_provider(
            Some(&provider),
            std::ptr::null(),
            true,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
    .ok_or_else(|| format!("{}는 JPEG가 아니다", path.display()))
}

fn draw_page(context: &CGContext, page: &PdfPage) -> Result<(), String> {
    let image = load_jpeg(&page.image)?;
    let (pw, ph) = (CGImage::width(Some(&image)) as f64, CGImage::height(Some(&image)) as f64);
    let scale = PAGE_LONG_SIDE_PT / pw.max(ph).max(1.0);
    let (w, h) = (pw * scale, ph * scale);
    let media = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w, h));
    unsafe { CGContext::begin_page(Some(context), &media) };
    CGContext::draw_image(Some(context), media, Some(&image));
    if let Some(ocr) = &page.ocr {
        draw_text_layer(context, ocr, w, h);
    }
    CGContext::end_page(Some(context));
    Ok(())
}

fn draw_text_layer(context: &CGContext, ocr: &OcrPage, w: f64, h: f64) {
    CGContext::set_text_drawing_mode(Some(context), CGTextDrawingMode::Invisible);
    for line in ocr.blocks.iter().flat_map(|b| b.lines.iter()) {
        let text = line.text.trim();
        let (bw, bh) = (line.bbox.width * w, line.bbox.height * h);
        if text.is_empty() || bw <= 0.0 || bh <= 0.0 {
            continue;
        }
        // 줄 상자 높이의 대부분이 글자 몸통이다. 가로는 글꼴 폭과 실제 폭이 달라 비율로 늘이거나 줄인다
        let font_size = bh * 0.9;
        let font = unsafe { CTFont::with_name(&CFString::from_static_str(TEXT_FONT), font_size, std::ptr::null()) };
        let attr_keys: [&CFString; 1] = unsafe { [kCTFontAttributeName] };
        let attr_values: [&CFType; 1] = [&font];
        let attributes = CFDictionary::<CFString, CFType>::from_slices(&attr_keys, &attr_values);
        let string = CFString::from_str(text);
        let Some(attributed) = (unsafe { CFAttributedString::new(None, Some(&string), Some(attributes.as_ref())) }) else {
            continue;
        };
        let ct_line = unsafe { CTLine::with_attributed_string(&attributed) };
        let mut descent = 0.0;
        let width = unsafe { ct_line.typographic_bounds(std::ptr::null_mut(), &mut descent, std::ptr::null_mut()) };
        if width <= 0.0 {
            continue;
        }
        let stretch = bw / width;
        CGContext::set_text_matrix(
            Some(context),
            CGAffineTransform {
                a: stretch,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                tx: 0.0,
                ty: 0.0,
            },
        );
        // PDF는 원점이 왼쪽 아래다. 줄 상자 아래 변에서 글꼴 아래 여백만큼 올린 곳이 기준선이다
        let x = line.bbox.x * w;
        let baseline = h - (line.bbox.y + line.bbox.height) * h + descent * 0.5;
        CGContext::set_text_position(Some(context), x, baseline);
        unsafe { ct_line.draw(context) };
    }
}

/// PDFKit으로 PDF에서 글자를 다시 꺼낸다. 텍스트 레이어가 제대로 들어갔는지 확인하는 용도
#[cfg(test)]
pub fn extract_text(path: &Path) -> Option<String> {
    use objc2::AnyThread;
    use objc2_foundation::{NSString, NSURL};
    use objc2_pdf_kit::PDFDocument;
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let document = unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &url) }?;
    unsafe { document.string() }.map(|s| s.to_string())
}

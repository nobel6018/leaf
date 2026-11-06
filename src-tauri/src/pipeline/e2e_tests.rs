//! 합성 문서로 원본(HEIC, EXIF 회전) → 보정 → OCR → 검색 → PDF 텍스트 레이어까지 한 번에 확인한다.
//! 글자는 CoreText로 그려 실제 스캔이나 저작물을 쓰지 않는다.

use std::path::Path;

use image::{Rgb, RgbImage};
use scanshelf_core::ocr::OcrPage;
use scanshelf_core::processing::Processing;
use scanshelf_vision::perspective::{Homography, PixelPoint};
use objc2_core_foundation::{
    CFAttributedString, CFDictionary, CFNumber, CFString, CFType, CFURL, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, kCGColorSpaceSRGB};
use objc2_core_text::{CTFont, CTLine, kCTFontAttributeName};
use objc2_image_io::{CGImageDestination, kCGImagePropertyOrientation};

use super::{ProcessContext, platform, process_document};

const LINES: [&str; 3] = ["데이터센터 자율 로봇", "Autonomous Mobile Robot 2026", "적재 하중 20kg 반복 정밀도"];

/// 흰 종이에 검은 글자를 그린 합성 페이지
fn render_page(width: usize, height: usize) -> RgbImage {
    render_lines(width, height, &LINES)
}

fn render_lines(width: usize, height: usize, lines: &[&str]) -> RgbImage {
    let mut rgba = vec![0u8; width * height * 4];
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).unwrap();
    let context = unsafe {
        CGBitmapContextCreate(
            rgba.as_mut_ptr().cast(),
            width,
            height,
            8,
            width * 4,
            Some(&space),
            CGImageAlphaInfo::NoneSkipLast.0,
        )
    }
    .unwrap();
    CGContext::set_rgb_fill_color(Some(&context), 1.0, 1.0, 1.0, 1.0);
    CGContext::fill_rect(Some(&context), CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width as f64, height as f64)));
    let font = unsafe { CTFont::with_name(&CFString::from_static_str("AppleSDGothicNeo-Bold"), 56.0, std::ptr::null()) };
    for (i, text) in lines.iter().enumerate() {
        let keys: [&CFString; 1] = unsafe { [kCTFontAttributeName] };
        let values: [&CFType; 1] = [&font];
        let attributes = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
        let attributed = unsafe { CFAttributedString::new(None, Some(&CFString::from_str(text)), Some(attributes.as_ref())) }.unwrap();
        let line = unsafe { CTLine::with_attributed_string(&attributed) };
        CGContext::set_text_position(Some(&context), 60.0, height as f64 - 120.0 - i as f64 * 110.0);
        unsafe { line.draw(&context) };
    }
    drop(context);
    let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    RgbImage::from_raw(width as u32, height as u32, rgb).unwrap()
}

/// 페이지를 기울어진 사각형으로 회색 책상 위에 찍은 사진
fn photograph(page: &RgbImage, quad: [(f64, f64); 4], canvas: (u32, u32)) -> RgbImage {
    let (pw, ph) = (page.width() as f64, page.height() as f64);
    let rect = [
        PixelPoint::new(0.0, 0.0),
        PixelPoint::new(pw, 0.0),
        PixelPoint::new(pw, ph),
        PixelPoint::new(0.0, ph),
    ];
    let to_page = Homography::from_points(quad.map(|(x, y)| PixelPoint::new(x, y)), rect).unwrap();
    RgbImage::from_fn(canvas.0, canvas.1, |x, y| {
        let p = to_page.apply(PixelPoint::new(x as f64 + 0.5, y as f64 + 0.5));
        if p.x < 0.0 || p.y < 0.0 || p.x >= pw || p.y >= ph {
            Rgb([70, 72, 75])
        } else {
            *page.get_pixel(p.x as u32, p.y as u32)
        }
    })
}

/// iPhone처럼 센서 방향 그대로 저장하고 EXIF 회전(6: 시계 방향 90도)을 붙인 HEIC
fn write_rotated_heic(path: &Path, displayed: &RgbImage) {
    let stored = image::imageops::rotate270(displayed);
    let cgimage = platform::rgb_to_cgimage(&stored);
    let url = CFURL::from_file_path(path).unwrap();
    let destination =
        unsafe { CGImageDestination::with_url(&url, &CFString::from_static_str("public.heic"), 1, None) }.unwrap();
    let orientation = CFNumber::new_i32(6);
    let keys: [&CFString; 1] = unsafe { [kCGImagePropertyOrientation] };
    let values: [&CFType; 1] = [&orientation];
    let properties = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
    unsafe {
        destination.add_image(&cgimage, Some(properties.as_ref()));
        assert!(destination.finalize(), "HEIC 저장 실패");
    }
}

#[test]
fn 합성_문서를_처리하고_검색과_pdf_텍스트까지_나온다() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let document_id = uuid::Uuid::new_v4();
    let page_id = uuid::Uuid::new_v4();
    let dir = root.path().join(document_id.to_string());
    std::fs::create_dir_all(dir.join("raw")).unwrap();

    let canvas = (1500u32, 1100u32);
    let quad_px = [(180.0, 120.0), (1330.0, 160.0), (1370.0, 980.0), (140.0, 940.0)];
    let photo = photograph(&render_page(1200, 860), quad_px, canvas);
    write_rotated_heic(&dir.join(format!("raw/{page_id}.heic")), &photo);

    // 회전 적용 확인: 저장은 세로(1100x1500)지만 읽으면 가로(1500x1100)
    let decoded = platform::decode_oriented(&dir.join(format!("raw/{page_id}.heic")), None).unwrap();
    assert_eq!(decoded.dimensions(), canvas);

    let quad: Vec<serde_json::Value> = quad_px
        .iter()
        .map(|(x, y)| serde_json::json!({"x": x / canvas.0 as f64, "y": y / canvas.1 as f64}))
        .collect();
    let manifest = serde_json::json!({
        "schemaVersion": 1, "id": document_id, "name": "합성 예시로보틱스", "kind": "pamphlet",
        "session": "2026 예시박람회", "createdAt": "2026-11-05T01:20:00Z",
        "pages": [{
            "id": page_id, "raw": format!("raw/{page_id}.heic"), "capturedAt": "2026-11-05T01:20:03.412Z",
            "layout": "single",
            "quad": {"topLeft": quad[0], "topRight": quad[1], "bottomRight": quad[2], "bottomLeft": quad[3]}
        }]
    });
    std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();

    let io_lock = std::sync::Mutex::new(());
    let progress = std::sync::Mutex::new(Vec::new());
    let on_progress = |p: super::Progress| progress.lock().unwrap().push(p.stage);
    let ctx = ProcessContext {
        dir: &dir,
        cache: cache.path(),
        io_lock: &io_lock,
        force: false,
        models: &scanshelf_vision::Models::none(),
        on_progress: &on_progress,
    };
    let processing: Processing = process_document(&ctx).unwrap();
    assert_eq!(processing.outputs.len(), 1);
    assert_eq!(progress.lock().unwrap().last(), Some(&"done"));

    let output = &processing.outputs[0];
    let page = image::open(dir.join(&output.image)).unwrap();
    // 출력 크기는 꼭짓점 변 길이에서 나온다(긴 변 약 1230)
    assert!((1150..1300).contains(&page.width()), "너비 {}", page.width());

    let ocr: OcrPage = serde_json::from_slice(&std::fs::read(dir.join(output.ocr.as_ref().unwrap())).unwrap()).unwrap();
    let text = crate::search::compact(&ocr.text);
    assert!(text.contains("데이터센터"), "OCR: {}", ocr.text);
    assert!(text.contains("robot"), "OCR: {}", ocr.text);

    // 바뀐 것이 없으면 다시 처리하지 않는다
    progress.lock().unwrap().clear();
    let manifest_loaded = scanshelf_core::Manifest::load(&dir).unwrap();
    assert_eq!(super::stale_pages(&dir, cache.path(), &manifest_loaded, &processing), 0);
    process_document(&ctx).unwrap();
    assert!(!progress.lock().unwrap().contains(&"decode"));

    // 검색 색인
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::search::init_schema(&conn).unwrap();
    crate::search::index_document(&mut conn, &dir, false).unwrap();
    let hits = crate::search::search(&conn, "데이터센터").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].hits[0].page_number, 1);

    // 텍스트 레이어 PDF: PDFKit으로 다시 꺼낸 글자에 한국어가 있어야 한다
    let pdf = crate::export::export_to_dir(&dir, crate::export::ExportFormat::Pdf).unwrap();
    let extracted = crate::export::pdf_macos::extract_text(&pdf).unwrap_or_default();
    assert!(crate::search::compact(&extracted).contains("데이터센터"), "PDF 글자: {extracted}");
    let txt = crate::export::export_to_dir(&dir, crate::export::ExportFormat::Txt).unwrap();
    assert!(std::fs::read_to_string(txt).unwrap().starts_with("[1쪽]"));
    let zip = crate::export::export_to_dir(&dir, crate::export::ExportFormat::Jpg).unwrap();
    assert!(std::fs::metadata(zip).unwrap().len() > 10_000);
    if std::env::var_os("SCANSHELF_KEEP_E2E").is_some() {
        let keep = std::env::temp_dir().join("scanshelf-e2e");
        let _ = std::fs::remove_dir_all(&keep);
        std::fs::create_dir_all(&keep).unwrap();
        std::fs::copy(&pdf, keep.join("e2e.pdf")).unwrap();
        std::fs::copy(dir.join(&output.image), keep.join("page.jpg")).unwrap();
        eprintln!("결과를 {}에 남김", keep.display());
    }
}

/// 앱을 직접 띄워 볼 때 쓰는 합성 문서 만들기. 실행: SCANSHELF_SAMPLE_LIBRARY=<보관함 폴더> cargo test -p scanshelf -- --ignored 합성_예시
#[test]
#[ignore]
fn 합성_예시_문서를_보관함에_만든다() {
    let Some(library) = std::env::var_os("SCANSHELF_SAMPLE_LIBRARY") else {
        return;
    };
    let library = std::path::PathBuf::from(library);
    // (줄 글자, iPhone 꼭짓점 있음, 배치)
    type SamplePage<'a> = (Vec<&'a str>, bool, &'a str);
    let docs: [(&str, &str, &str, Vec<SamplePage>); 2] = [
        (
            "2026 예시박람회_예시로보틱스",
            "pamphlet",
            "2026 예시박람회",
            vec![
                (vec!["예시로보틱스", "데이터센터 랙 운반 로봇", "적재 하중 500kg"], true, "single"),
                (vec!["Autonomous Mobile Robot", "자율 주행 정밀도 10mm", "문의 sample@example.com"], false, "single"),
                (vec!["도입 사례", "예시 클라우드 센터 2025", "예시 통신사 2026"], true, "single"),
            ],
        ),
        (
            "예시 책",
            "book",
            "",
            vec![(vec!["1장 시작하며", "왼쪽 쪽 본문입니다", "오른쪽 쪽 본문입니다"], true, "spread")],
        ),
    ];
    for (name, kind, session, pages) in docs {
        let document_id = uuid::Uuid::new_v4();
        let dir = library.join(document_id.to_string());
        std::fs::create_dir_all(dir.join("raw")).unwrap();
        let canvas = (1500u32, 1100u32);
        let quad_px = [(180.0, 120.0), (1330.0, 160.0), (1370.0, 980.0), (140.0, 940.0)];
        let mut manifest_pages = Vec::new();
        for (lines, with_quad, layout) in pages {
            let page_id = uuid::Uuid::new_v4();
            let photo = photograph(&render_lines(1200, 860, &lines), quad_px, canvas);
            write_rotated_heic(&dir.join(format!("raw/{page_id}.heic")), &photo);
            let quad: Vec<serde_json::Value> = quad_px
                .iter()
                .map(|(x, y)| serde_json::json!({"x": x / canvas.0 as f64, "y": y / canvas.1 as f64}))
                .collect();
            let mut page = serde_json::json!({
                "id": page_id, "raw": format!("raw/{page_id}.heic"), "capturedAt": "2026-11-05T01:20:03.412Z",
                "layout": layout,
            });
            if with_quad {
                page["quad"] = serde_json::json!({"topLeft": quad[0], "topRight": quad[1], "bottomRight": quad[2], "bottomLeft": quad[3]});
            }
            manifest_pages.push(page);
        }
        let mut manifest = serde_json::json!({
            "schemaVersion": 1, "id": document_id, "name": name, "kind": kind,
            "createdAt": "2026-11-05T01:20:00Z", "pages": manifest_pages
        });
        if !session.is_empty() {
            manifest["session"] = serde_json::json!(session);
        }
        std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
        eprintln!("만든 문서: {}", dir.display());
    }
}

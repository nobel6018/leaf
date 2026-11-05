//! Finder에서 가져오기: "다음으로 열기", 창에 끌어다 놓기, 파일 메뉴의 "가져오기…".
//!
//! 그림(jpg, png, heic)과 PDF를 보관함의 새 문서로 만든다. iPhone이 찍은 문서와 같은 형식이며,
//! 이때만 Mac이 manifest를 쓴다(가져오기가 "찍은 기기" 역할을 한다). 원본은 `raw/`에 두고 꼭짓점은 비워
//! 처리 단계가 원본에서 다시 감지하게 한다. PDF는 쪽마다 300dpi JPEG로 그려 원본으로 둔다.
//!
//! 새 문서는 받는 중 폴더(`incoming/<문서 id>/`)에 다 만든 뒤 `Store::import`로 한 번에 보관함에 넣는다.
//!
//! @see docs/import.md
//! @see docs/document-format.md

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, SubsecRound, Utc};
use leaf_core::{DocumentKind, Manifest, Page, PageLayout, SCHEMA_VERSION};
use serde::Serialize;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

use crate::store::{ImportMode, RAW_DIR, Store};

/// 그대로 원본으로 쓰는 그림. 처리 단계가 ImageIO로 읽는다(EXIF 회전 포함)
pub const IMAGE_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "heic", "heif"];
pub const PDF_EXTENSION: &str = "pdf";
/// PDF를 그리는 해상도. OCR이 작은 글자까지 읽는 값
pub const PDF_DPI: f64 = 300.0;
/// 포스터 같은 큰 쪽이 수백 MB 비트맵이 되지 않게 긴 변을 이 픽셀로 막는다
pub const PDF_MAX_PIXELS: f64 = 7000.0;
const JPEG_QUALITY: u8 = 92;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Image,
    Pdf,
}

fn source_kind(path: &Path) -> Option<SourceKind> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        Some(SourceKind::Image)
    } else if ext == PDF_EXTENSION {
        Some(SourceKind::Pdf)
    } else {
        None
    }
}

/// 가져올 수 있는 파일만 남긴다. 절대 경로의 실제 그림, PDF 파일만 받는다.
/// 네트워크 경로(\\server\share)는 열기만 해도 인증 정보가 그 서버로 가므로 막는다
pub fn importable_files(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter(|p| {
            let s = p.to_string_lossy();
            let network = s.starts_with("\\\\") || s.starts_with("//");
            p.is_absolute() && !network && source_kind(p).is_some() && p.is_file()
        })
        .collect()
}

/// 만들 문서 하나
#[derive(Debug, Clone, PartialEq)]
struct Plan {
    name: String,
    kind: SourceKind,
    sources: Vec<PathBuf>,
}

/// 함께 고른 그림은 이름순으로 한 문서(그림 한 장이 한 쪽), PDF는 파일마다 한 문서
fn plan_import(paths: &[PathBuf]) -> Vec<Plan> {
    let mut images: Vec<PathBuf> = paths.iter().filter(|p| source_kind(p) == Some(SourceKind::Image)).cloned().collect();
    images.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().trim().to_string()).unwrap_or_default();
    let mut plans = Vec::new();
    if let Some(first) = images.first() {
        let name = match images.len() {
            1 => stem(first),
            n => format!("{} 외 {}장", stem(first), n - 1),
        };
        plans.push(Plan {
            name,
            kind: SourceKind::Image,
            sources: images,
        });
    }
    for pdf in paths.iter().filter(|p| source_kind(p) == Some(SourceKind::Pdf)) {
        plans.push(Plan {
            name: stem(pdf),
            kind: SourceKind::Pdf,
            sources: vec![pdf.clone()],
        });
    }
    plans
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportedDocument {
    pub document_id: String,
    pub name: String,
    pub pages: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkippedFile {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub documents: Vec<ImportedDocument>,
    pub skipped: Vec<SkippedFile>,
}

/// PDF 쪽을 그려 JPEG로 쓰는 함수. 쪽 수를 돌려준다. 테스트는 가짜를 넣는다
type PdfRenderer = fn(&Path, &mut dyn FnMut(image::RgbImage) -> Result<(), String>) -> Result<usize, String>;

fn milliseconds(t: DateTime<Utc>) -> DateTime<Utc> {
    t.trunc_subsecs(3)
}

fn modified_at(path: &Path) -> Option<DateTime<Utc>> {
    let modified = fs::metadata(path).and_then(|m| m.modified()).ok()?;
    Some(milliseconds(DateTime::<Utc>::from(modified)))
}

/// 받는 중 폴더에 원본과 manifest를 만들고 보관함으로 옮긴다. 실패하면 받는 중 폴더를 지운다
fn build_document(store: &Store, plan: &Plan, render_pdf: PdfRenderer) -> Result<ImportedDocument, String> {
    let id = Uuid::new_v4();
    let staging = store.staging_dir(id);
    let result = (|| {
        let raw_dir = staging.join(RAW_DIR);
        fs::create_dir_all(&raw_dir).map_err(|e| format!("{} 만들기 실패: {e}", raw_dir.display()))?;
        let now = milliseconds(Utc::now());
        let mut pages = Vec::new();
        match plan.kind {
            SourceKind::Image => {
                for source in &plan.sources {
                    let page_id = Uuid::new_v4();
                    let ext = source.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                    let ext = if ext == "jpeg" { "jpg".to_string() } else { ext };
                    let rel = format!("{RAW_DIR}/{page_id}.{ext}");
                    fs::copy(source, staging.join(&rel)).map_err(|e| format!("{} 복사 실패: {e}", source.display()))?;
                    pages.push(page(page_id, rel, modified_at(source).unwrap_or(now)));
                }
            }
            SourceKind::Pdf => {
                let source = &plan.sources[0];
                render_pdf(source, &mut |image| {
                    let page_id = Uuid::new_v4();
                    let rel = format!("{RAW_DIR}/{page_id}.jpg");
                    crate::pipeline::write_jpeg(&staging.join(&rel), &image, JPEG_QUALITY)?;
                    pages.push(page(page_id, rel, now));
                    Ok(())
                })?;
            }
        }
        if pages.is_empty() {
            return Err("가져올 쪽이 없다".to_string());
        }
        let manifest = Manifest {
            schema_version: SCHEMA_VERSION,
            id,
            name: if plan.name.is_empty() { "가져온 문서".into() } else { plan.name.clone() },
            kind: DocumentKind::Document,
            session: None,
            created_at: now,
            pages,
        };
        let count = manifest.pages.len();
        store.import(&manifest, &staging, ImportMode::Move).map_err(|e| e.to_string())?;
        Ok(ImportedDocument {
            document_id: id.to_string(),
            name: manifest.name,
            pages: count,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn page(id: Uuid, raw: String, captured_at: DateTime<Utc>) -> Page {
    Page {
        id,
        raw,
        captured_at,
        layout: PageLayout::Single,
        quad: None,
    }
}

fn run_import(store: &Store, paths: Vec<PathBuf>, render_pdf: PdfRenderer) -> ImportResult {
    let accepted = importable_files(paths.iter().cloned());
    let mut result = ImportResult::default();
    for path in paths.iter().filter(|p| !accepted.contains(p)) {
        result.skipped.push(SkippedFile {
            path: path.to_string_lossy().into_owned(),
            reason: "jpg, png, heic 그림이나 PDF 파일이 아닙니다".into(),
        });
    }
    for plan in plan_import(&accepted) {
        let _busy = crate::busy::start(format!("가져오기: {}", plan.name));
        match build_document(store, &plan, render_pdf) {
            Ok(document) => result.documents.push(document),
            Err(reason) => result.skipped.extend(plan.sources.iter().map(|p| SkippedFile {
                path: p.to_string_lossy().into_owned(),
                reason: reason.clone(),
            })),
        }
    }
    result
}

/// 가져온 뒤 검색 색인에 넣고 library-changed를 보낸다. lib.rs가 이 이벤트를 받아 처리 줄에 넣는다
#[tauri::command]
pub async fn import_files(app: tauri::AppHandle, paths: Vec<String>) -> Result<ImportResult, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let result = tauri::async_runtime::spawn_blocking(move || run_import(&Store::new(&app_data), paths, render_pdf_pages))
        .await
        .map_err(|e| e.to_string())?;
    for document in &result.documents {
        crate::search::refresh_document(&app, &document.document_id);
        if let Ok(id) = Uuid::parse_str(&document.document_id) {
            crate::transfer::emit_library_changed(&app, id);
        }
    }
    Ok(result)
}

/// 파일 고르기 창. 취소하면 빈 목록
#[tauri::command]
pub async fn pick_import_files(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let mut extensions: Vec<&str> = IMAGE_EXTENSIONS.to_vec();
    extensions.push(PDF_EXTENSION);
    let picked = app
        .dialog()
        .file()
        .set_title("leaf로 가져오기")
        .add_filter("그림, PDF", &extensions)
        .blocking_pick_files()
        .unwrap_or_default();
    Ok(picked
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

#[cfg(target_os = "macos")]
fn render_pdf_pages(path: &Path, on_page: &mut dyn FnMut(image::RgbImage) -> Result<(), String>) -> Result<usize, String> {
    pdf_macos::render(path, PDF_DPI, PDF_MAX_PIXELS, on_page)
}

#[cfg(not(target_os = "macos"))]
fn render_pdf_pages(_path: &Path, _on_page: &mut dyn FnMut(image::RgbImage) -> Result<(), String>) -> Result<usize, String> {
    Err("PDF 가져오기는 아직 macOS에서만 된다".into())
}

/// PDF 쪽을 CoreGraphics로 비트맵에 그린다. 쪽 회전(/Rotate)과 자르기 상자(CropBox)를 따른다
#[cfg(target_os = "macos")]
mod pdf_macos {
    use std::path::Path;

    use objc2_core_foundation::{CFURL, CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGInterpolationQuality, CGPDFBox, CGPDFDocument,
        CGPDFPage, kCGColorSpaceSRGB,
    };

    /// 72pt가 1인치. 회전을 적용한 쪽 크기에 dpi를 곱하고, 긴 변이 max_pixels를 넘으면 줄인다
    pub fn pixel_size(width_pt: f64, height_pt: f64, rotation: i32, dpi: f64, max_pixels: f64) -> (usize, usize) {
        let (w, h) = if rotation.rem_euclid(180) == 90 { (height_pt, width_pt) } else { (width_pt, height_pt) };
        let mut scale = dpi / 72.0;
        let long = w.max(h) * scale;
        if long > max_pixels {
            scale *= max_pixels / long;
        }
        (((w * scale).round() as usize).max(1), ((h * scale).round() as usize).max(1))
    }

    pub fn render(
        path: &Path,
        dpi: f64,
        max_pixels: f64,
        on_page: &mut dyn FnMut(image::RgbImage) -> Result<(), String>,
    ) -> Result<usize, String> {
        let url = CFURL::from_file_path(path).ok_or("PDF 경로가 올바르지 않다")?;
        let document = CGPDFDocument::with_url(Some(&url)).ok_or_else(|| format!("{}를 PDF로 열지 못했다", path.display()))?;
        if CGPDFDocument::is_encrypted(Some(&document)) && !CGPDFDocument::is_unlocked(Some(&document)) {
            return Err("암호가 걸린 PDF는 가져오지 못한다".into());
        }
        let count = CGPDFDocument::number_of_pages(Some(&document));
        for number in 1..=count {
            let page = CGPDFDocument::page(Some(&document), number).ok_or_else(|| format!("{number}쪽을 읽지 못했다"))?;
            on_page(render_page(&page, dpi, max_pixels))?;
        }
        Ok(count)
    }

    fn render_page(page: &CGPDFPage, dpi: f64, max_pixels: f64) -> image::RgbImage {
        let crop = CGPDFPage::box_rect(Some(page), CGPDFBox::CropBox);
        let rotation = CGPDFPage::rotation_angle(Some(page));
        let (width, height) = pixel_size(crop.size.width, crop.size.height, rotation, dpi, max_pixels);
        let mut rgba = vec![255u8; width * height * 4];
        let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).expect("sRGB 색 공간은 항상 있다");
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
        .expect("RGBA 비트맵 문맥 생성");
        // 투명한 PDF 바탕은 흰 종이로 본다(버퍼를 255로 채웠다)
        CGContext::set_interpolation_quality(Some(&context), CGInterpolationQuality::High);
        // GetDrawingTransform은 키우지 않으므로, 쪽 크기(pt) 그대로의 변환(회전, 원점 맞추기)을 구한 뒤 배율을 따로 건다
        let (w_pt, h_pt) = if rotation.rem_euclid(180) == 90 {
            (crop.size.height, crop.size.width)
        } else {
            (crop.size.width, crop.size.height)
        };
        let target = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w_pt, h_pt));
        let transform = CGPDFPage::drawing_transform(Some(page), CGPDFBox::CropBox, target, 0, true);
        CGContext::scale_ctm(Some(&context), width as f64 / w_pt, height as f64 / h_pt);
        CGContext::concat_ctm(Some(&context), transform);
        CGContext::draw_pdf_page(Some(&context), Some(page));
        drop(context);
        let mut rgb = Vec::with_capacity(width * height * 3);
        for pixel in rgba.as_chunks::<4>().0 {
            rgb.extend_from_slice(&pixel[..3]);
        }
        image::RgbImage::from_raw(width as u32, height as u32, rgb).expect("버퍼 크기가 맞다")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use leaf_core::ocr::OcrPage;
    use leaf_core::processing::{OutputPage, PIPELINE_VERSION, PagePart, ProcessOptions, Processing};

    /// 처리까지 끝난 합성 문서. 쪽마다 OCR 글자를 넣는다
    pub(crate) fn write_sample_document(root: &Path, name: &str, texts: &[&str]) -> PathBuf {
        let id = Uuid::new_v4();
        let dir = root.join(id.to_string());
        let mut pages = Vec::new();
        let mut outputs = Vec::new();
        for text in texts {
            let page_id = Uuid::new_v4();
            pages.push(page(page_id, format!("raw/{page_id}.jpg"), milliseconds(Utc::now())));
            let ocr = OcrPage {
                engine: "test".into(),
                image_width: 10,
                image_height: 10,
                text: text.to_string(),
                blocks: vec![],
            };
            crate::library::write_json(&dir.join(format!("ocr/{page_id}.json")), &ocr).unwrap();
            outputs.push(OutputPage {
                id: page_id.to_string(),
                source_page: page_id,
                part: PagePart::Whole,
                image: format!("pages/{page_id}.jpg"),
                ocr: Some(format!("ocr/{page_id}.json")),
                options: ProcessOptions::default(),
                pipeline_version: PIPELINE_VERSION,
                processed_at: Utc::now(),
            });
        }
        fs::create_dir_all(&dir).unwrap();
        Manifest {
            schema_version: SCHEMA_VERSION,
            id,
            name: name.into(),
            kind: DocumentKind::Document,
            session: None,
            created_at: milliseconds(Utc::now()),
            pages,
        }
        .save(&dir)
        .unwrap();
        crate::library::write_json(
            &dir.join(leaf_core::processing::PROCESSING_FILE),
            &Processing {
                defaults: ProcessOptions::default(),
                outputs,
            },
        )
        .unwrap();
        dir
    }

    fn write_jpeg_file(path: &Path) {
        let image = image::RgbImage::from_pixel(8, 6, image::Rgb([200, 30, 30]));
        crate::pipeline::write_jpeg(path, &image, 80).unwrap();
    }

    fn fake_pdf(_path: &Path, on_page: &mut dyn FnMut(image::RgbImage) -> Result<(), String>) -> Result<usize, String> {
        for _ in 0..3 {
            on_page(image::RgbImage::from_pixel(10, 14, image::Rgb([255, 255, 255])))?;
        }
        Ok(3)
    }

    fn empty_pdf(_path: &Path, _on_page: &mut dyn FnMut(image::RgbImage) -> Result<(), String>) -> Result<usize, String> {
        Ok(0)
    }

    #[test]
    fn 그림은_이름순으로_한_문서_pdf는_파일마다_한_문서() {
        let paths = vec![
            PathBuf::from("/s/scan_002.HEIC"),
            PathBuf::from("/s/계약서.pdf"),
            PathBuf::from("/s/scan_001.jpg"),
            PathBuf::from("/s/부록.PDF"),
        ];

        let plans = plan_import(&paths);

        assert_eq!(plans.len(), 3);
        assert_eq!(plans[0].name, "scan_001 외 1장");
        assert_eq!(plans[0].sources, vec![PathBuf::from("/s/scan_001.jpg"), PathBuf::from("/s/scan_002.HEIC")]);
        assert_eq!((plans[1].name.as_str(), plans[1].kind), ("계약서", SourceKind::Pdf));
        assert_eq!(plans[2].name, "부록");
    }

    #[test]
    fn 네트워크_경로와_상대_경로와_모르는_형식은_가져오지_않는다() {
        let dir = tempfile::tempdir().unwrap();
        let jpg = dir.path().join("a.jpg");
        let txt = dir.path().join("a.txt");
        fs::write(&jpg, b"x").unwrap();
        fs::write(&txt, b"x").unwrap();

        let kept = importable_files([
            jpg.clone(),
            txt,
            dir.path().join("없는.png"),
            PathBuf::from("a.jpg"),
            PathBuf::from("//server/share/a.jpg"),
        ]);

        assert_eq!(kept, vec![jpg]);
    }

    #[test]
    fn 그림을_가져오면_manifest와_원본이_보관함에_생긴다() {
        let data = tempfile::tempdir().unwrap();
        let store = Store::new(data.path());
        let src = tempfile::tempdir().unwrap();
        let a = src.path().join("영수증 1.jpeg");
        let b = src.path().join("영수증 2.png");
        write_jpeg_file(&a);
        image::RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 0])).save(&b).unwrap();

        let result = run_import(&store, vec![b.clone(), a.clone()], fake_pdf);

        assert!(result.skipped.is_empty(), "{:?}", result.skipped);
        assert_eq!(result.documents.len(), 1);
        let doc = &result.documents[0];
        assert_eq!((doc.name.as_str(), doc.pages), ("영수증 1 외 1장", 2));
        let dir = store.document_dir(Uuid::parse_str(&doc.document_id).unwrap());
        let manifest = Manifest::load(&dir).unwrap();
        assert_eq!(manifest.kind, DocumentKind::Document);
        assert!(manifest.pages.iter().all(|p| p.quad.is_none() && p.layout == PageLayout::Single));
        assert!(manifest.pages[0].raw.ends_with(".jpg"), "jpeg는 jpg로");
        assert!(manifest.pages[1].raw.ends_with(".png"));
        assert_eq!(fs::read(dir.join(&manifest.pages[0].raw)).unwrap(), fs::read(&a).unwrap(), "원본 그대로 복사");
        assert_eq!(manifest.created_at.timestamp_subsec_nanos() % 1_000_000, 0, "밀리초 단위");
        assert!(a.is_file() && b.is_file(), "Finder 원본은 그대로 둔다");
        assert!(!store.staging_dir(manifest.id).exists(), "받는 중 폴더를 비운다");
    }

    #[test]
    fn pdf는_쪽마다_jpeg_원본이_된다() {
        let data = tempfile::tempdir().unwrap();
        let store = Store::new(data.path());
        let src = tempfile::tempdir().unwrap();
        let pdf = src.path().join("설명서.pdf");
        fs::write(&pdf, b"%PDF-1.4").unwrap();

        let result = run_import(&store, vec![pdf], fake_pdf);

        assert_eq!(result.documents[0].pages, 3);
        let dir = store.document_dir(Uuid::parse_str(&result.documents[0].document_id).unwrap());
        let manifest = Manifest::load(&dir).unwrap();
        assert_eq!(manifest.name, "설명서");
        for page in &manifest.pages {
            let image = image::open(dir.join(&page.raw)).unwrap();
            assert_eq!((image.width(), image.height()), (10, 14));
        }
    }

    #[test]
    fn 쪽이_없는_pdf는_건너뛰고_받는_중_폴더를_남기지_않는다() {
        let data = tempfile::tempdir().unwrap();
        let store = Store::new(data.path());
        let src = tempfile::tempdir().unwrap();
        let pdf = src.path().join("빈.pdf");
        fs::write(&pdf, b"%PDF-1.4").unwrap();
        let txt = src.path().join("메모.txt");
        fs::write(&txt, b"x").unwrap();

        let result = run_import(&store, vec![pdf, txt], empty_pdf);

        assert!(result.documents.is_empty());
        assert_eq!(result.skipped.len(), 2);
        let incoming = data.path().join("incoming");
        assert!(fs::read_dir(&incoming).map(|mut d| d.next().is_none()).unwrap_or(true));
        assert!(fs::read_dir(data.path().join("library")).map(|mut d| d.next().is_none()).unwrap_or(true));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn 쪽_크기는_300dpi에_회전을_따르고_긴_변을_막는다() {
        assert_eq!(pdf_macos::pixel_size(595.0, 842.0, 0, 300.0, 7000.0), (2479, 3508));
        assert_eq!(pdf_macos::pixel_size(595.0, 842.0, 90, 300.0, 7000.0), (3508, 2479));
        let (w, h) = pdf_macos::pixel_size(2384.0, 3370.0, 0, 300.0, 7000.0);
        assert_eq!(h, 7000);
        assert!(w < 7000);
    }

    /// CoreGraphics로 합성 PDF(200x100pt, 왼쪽 절반 검정)를 만들어 실제로 그려 본다
    #[cfg(target_os = "macos")]
    #[test]
    fn 합성_pdf를_300dpi로_그린다() {
        use objc2_core_foundation::{CFURL, CGPoint, CGRect, CGSize};
        use objc2_core_graphics::{CGContext, CGPDFContextClose, CGPDFContextCreateWithURL};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("합성.pdf");
        let url = CFURL::from_file_path(&path).unwrap();
        let media = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(200.0, 100.0));
        let context = unsafe { CGPDFContextCreateWithURL(Some(&url), &media, None) }.unwrap();
        for _ in 0..2 {
            unsafe { CGContext::begin_page(Some(&context), &media) };
            CGContext::set_rgb_fill_color(Some(&context), 0.0, 0.0, 0.0, 1.0);
            CGContext::fill_rect(Some(&context), CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(100.0, 100.0)));
            CGContext::end_page(Some(&context));
        }
        CGPDFContextClose(Some(&context));
        drop(context);

        let mut images = Vec::new();
        let count = pdf_macos::render(&path, 300.0, 7000.0, &mut |image| {
            images.push(image);
            Ok(())
        })
        .unwrap();

        assert_eq!(count, 2);
        let image = &images[0];
        assert_eq!((image.width(), image.height()), (833, 417));
        assert!(image.get_pixel(100, 200).0[0] < 30, "왼쪽은 검정");
        assert!(image.get_pixel(700, 200).0[0] > 225, "오른쪽은 흰 종이");
    }
}

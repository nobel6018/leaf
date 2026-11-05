//! 처리 파이프라인: 원본(raw/) → 보정한 페이지(pages/) → OCR(ocr/) → processing.json.
//!
//! 페이지마다 입력 지문(원본 파일, 꼭짓점, 배치, 옵션, PIPELINE_VERSION)을 캐시에 남기고,
//! 지문이 같은 페이지는 건너뛴다. 꼭짓점 하나를 고치면 그 페이지만 다시 처리한다.
//!
//! @see docs/pipeline.md

pub mod platform;
pub mod steps;
pub mod worker;
#[cfg(all(test, target_os = "macos"))]
mod e2e_tests;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use image::RgbImage;
use leaf_core::processing::{OutputPage, PIPELINE_VERSION, PagePart, ProcessOptions, Processing};
use leaf_core::{Manifest, Page, Quad};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub use worker::{JobState, Pipeline};

use leaf_core::fsio::write_atomic;

use crate::library::{load_processing, read_json, write_json};

/// 썸네일 너비. 보관함 표지와 문서 화면 페이지 목록이 같이 쓴다
pub const THUMB_WIDTH: u32 = 360;
const JPEG_QUALITY: u8 = 90;

pub fn thumb_path(cache: &Path, document_id: &str, output_id: &str) -> PathBuf {
    cache.join("thumbs").join(document_id).join(format!("{output_id}.jpg"))
}

pub fn preview_path(cache: &Path, document_id: &str, page_id: &str) -> PathBuf {
    cache.join("previews").join(document_id).join(format!("{page_id}.jpg"))
}

fn state_path(cache: &Path, document_id: &str) -> PathBuf {
    cache.join("state").join(format!("{document_id}.json"))
}

/// 캐시에 두는 처리 상태. 계약(processing.json)에 입력 지문 칸이 없어 Mac 캐시에 둔다(docs/pipeline.md 참고)
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PipelineState {
    #[serde(default)]
    pages: BTreeMap<Uuid, PageState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageState {
    fingerprint: String,
    #[serde(default)]
    detected_quad: Option<Quad>,
}

fn load_state(cache: &Path, document_id: &str) -> PipelineState {
    read_json(&state_path(cache, document_id)).ok().flatten().unwrap_or_default()
}

/// Mac이 다시 감지한 꼭짓점. 화면이 iPhone 감지값이 없는 페이지에 보여 준다
pub fn detected_quads(cache: &Path, document_id: &str) -> HashMap<Uuid, Quad> {
    load_state(cache, document_id)
        .pages
        .into_iter()
        .filter_map(|(id, s)| s.detected_quad.map(|q| (id, q)))
        .collect()
}

/// 이 값이 바뀌면 페이지를 다시 처리한다
fn fingerprint(dir: &Path, page: &Page, options: &ProcessOptions) -> String {
    let raw = dir.join(&page.raw);
    let (len, modified) = fs::metadata(&raw)
        .map(|m| {
            let modified = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis());
            (m.len(), modified)
        })
        .unwrap_or((0, 0));
    let input = serde_json::json!({
        "pipeline": PIPELINE_VERSION,
        "raw": page.raw,
        "len": len,
        "modified": modified.to_string(),
        "quad": page.quad,
        "layout": page.layout,
        "options": options,
    });
    hex::encode(Sha256::digest(input.to_string().as_bytes()))
}

fn page_is_current(
    dir: &Path,
    page: &Page,
    options: &ProcessOptions,
    processing: &Processing,
    state: &PipelineState,
) -> bool {
    let outputs: Vec<&OutputPage> = processing.outputs.iter().filter(|o| o.source_page == page.id).collect();
    if outputs.is_empty() {
        return false;
    }
    let versions_ok = outputs
        .iter()
        .all(|o| o.pipeline_version >= PIPELINE_VERSION && o.options == *options && dir.join(&o.image).exists());
    let fingerprint_ok = state
        .pages
        .get(&page.id)
        .is_some_and(|s| s.fingerprint == fingerprint(dir, page, options));
    versions_ok && fingerprint_ok
}

/// 다시 처리할 페이지 수. 보관함 목록에서 부르므로 이미지는 열지 않고 파일 정보만 본다
pub fn stale_pages(dir: &Path, cache: &Path, manifest: &Manifest, processing: &Processing) -> usize {
    let state = load_state(cache, &manifest.id.to_string());
    let options = processing.defaults;
    let current_ids: Vec<Uuid> = manifest.pages.iter().map(|p| p.id).collect();
    let orphan = processing.outputs.iter().any(|o| !current_ids.contains(&o.source_page));
    let stale = manifest
        .pages
        .iter()
        .filter(|page| !page_is_current(dir, page, &options, processing, &state))
        .count();
    // 지운 페이지의 출력이 남아 있으면 정리하러 한 번 더 돌아야 한다
    stale + usize::from(orphan)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub document_id: String,
    pub done: usize,
    pub total: usize,
    /// 지금 하는 일: `decode`, `correct`, `ocr`, `done`
    pub stage: &'static str,
}

pub fn write_jpeg(path: &Path, image: &RgbImage, quality: u8) -> Result<(), String> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode_image(image)
        .map_err(|e| format!("JPEG 인코딩 실패: {e}"))?;
    write_atomic(path, &bytes).map_err(|e| format!("{} 저장 실패: {e}", path.display()))
}

pub struct ProcessContext<'a> {
    pub dir: &'a Path,
    pub cache: &'a Path,
    /// 문서 단위로 processing.json 읽고 쓰기를 묶는다(화면의 옵션 저장과 겹치지 않게)
    pub io_lock: &'a std::sync::Mutex<()>,
    pub force: bool,
    pub models: &'a leaf_vision::Models,
    pub on_progress: &'a dyn Fn(Progress),
}

/// 문서 하나를 처리한다. 바뀐 페이지만 다시 하고, 페이지마다 processing.json을 저장해 중간에 꺼져도 한 일은 남는다
pub fn process_document(ctx: &ProcessContext<'_>) -> Result<Processing, String> {
    let dir = ctx.dir;
    let (manifest, _) = crate::library::load_applied(dir)?;
    let document_id = manifest.id.to_string();
    let options = load_processing(dir).defaults;
    let mut state = load_state(ctx.cache, &document_id);
    let total = manifest.pages.len();
    let report = |done: usize, stage: &'static str| {
        (ctx.on_progress)(Progress {
            document_id: document_id.clone(),
            done,
            total,
            stage,
        })
    };

    let mut outputs_by_page: HashMap<Uuid, Vec<OutputPage>> = HashMap::new();
    for output in load_processing(dir).outputs {
        outputs_by_page.entry(output.source_page).or_default().push(output);
    }

    for (index, page) in manifest.pages.iter().enumerate() {
        let current = {
            let processing = load_processing(dir);
            page_is_current(dir, page, &options, &processing, &state)
        };
        if current && !ctx.force {
            continue;
        }
        report(index, "decode");
        let raw_path = dir.join(&page.raw);
        let image = platform::decode_oriented(&raw_path, None).map_err(|e| e.to_string())?;
        let mut detected_quad = None;
        let quad = match page.quad {
            Some(q) => Some(q),
            None => {
                detected_quad = platform::detect_document_quad(&image);
                detected_quad
            }
        };
        report(index, "correct");
        let parts = steps::correct_page(steps::CorrectInput {
            image,
            quad,
            layout: page.layout,
            options: &options,
            models: ctx.models,
        })?;
        let mut page_outputs = Vec::new();
        for (part, image) in parts {
            page_outputs.push(write_output(ctx, &document_id, page.id, part, &image, &options, || {
                report(index, "ocr")
            })?);
        }
        let old = outputs_by_page.insert(page.id, page_outputs.clone()).unwrap_or_default();
        // 펼침 ↔ 낱장을 바꾸면 출력 id가 달라진다. 남은 옛 파일을 지운다
        for stale in old.iter().filter(|o| !page_outputs.iter().any(|n| n.id == o.id)) {
            remove_output_files(dir, ctx.cache, &document_id, stale);
        }
        state.pages.insert(
            page.id,
            PageState {
                fingerprint: fingerprint(dir, page, &options),
                detected_quad,
            },
        );
        write_json(&state_path(ctx.cache, &document_id), &state)?;
        save_outputs(ctx, &manifest, &outputs_by_page)?;
    }

    // 지운 페이지의 출력도 정리한다(원본은 그대로 둔다)
    let live: Vec<Uuid> = manifest.pages.iter().map(|p| p.id).collect();
    let removed: Vec<Uuid> = outputs_by_page.keys().filter(|id| !live.contains(id)).copied().collect();
    for id in removed {
        for output in outputs_by_page.remove(&id).unwrap_or_default() {
            remove_output_files(dir, ctx.cache, &document_id, &output);
        }
        state.pages.remove(&id);
    }
    write_json(&state_path(ctx.cache, &document_id), &state)?;
    let processing = save_outputs(ctx, &manifest, &outputs_by_page)?;
    report(total, "done");
    Ok(processing)
}

fn write_output(
    ctx: &ProcessContext<'_>,
    document_id: &str,
    source: Uuid,
    part: PagePart,
    image: &RgbImage,
    options: &ProcessOptions,
    on_ocr: impl Fn(),
) -> Result<OutputPage, String> {
    let id = part.output_id(source);
    let image_rel = format!("pages/{id}.jpg");
    write_jpeg(&ctx.dir.join(&image_rel), image, JPEG_QUALITY)?;
    let thumb_height = (image.height() as f64 * THUMB_WIDTH as f64 / image.width().max(1) as f64).round().max(1.0) as u32;
    let thumb = image::imageops::thumbnail(image, THUMB_WIDTH, thumb_height);
    write_jpeg(&thumb_path(ctx.cache, document_id, &id), &thumb, 80)?;
    on_ocr();
    // 글자 인식이 실패해도 페이지 이미지는 남긴다. ocr이 없으면 화면이 "글자 없음"으로 보여 준다
    let ocr_rel = format!("ocr/{id}.json");
    let ocr = match crate::ocr::recognize(image) {
        Ok(page) => {
            write_json(&ctx.dir.join(&ocr_rel), &page)?;
            Some(ocr_rel)
        }
        Err(error) => {
            eprintln!("OCR 실패 ({id}): {error}");
            None
        }
    };
    Ok(OutputPage {
        id,
        source_page: source,
        part,
        image: image_rel,
        ocr,
        options: *options,
        pipeline_version: PIPELINE_VERSION,
        processed_at: Utc::now(),
    })
}

fn remove_output_files(dir: &Path, cache: &Path, document_id: &str, output: &OutputPage) {
    let _ = fs::remove_file(dir.join(&output.image));
    if let Some(ocr) = &output.ocr {
        let _ = fs::remove_file(dir.join(ocr));
    }
    let _ = fs::remove_file(thumb_path(cache, document_id, &output.id));
}

/// 문서 페이지 순서대로 저장한다. 옵션 기본값은 저장 순간의 파일 값을 따른다(처리 중에 화면이 바꿨을 수 있다)
fn save_outputs(
    ctx: &ProcessContext<'_>,
    manifest: &Manifest,
    outputs_by_page: &HashMap<Uuid, Vec<OutputPage>>,
) -> Result<Processing, String> {
    let _guard = ctx.io_lock.lock().unwrap_or_else(|e| e.into_inner());
    let mut processing = load_processing(ctx.dir);
    processing.outputs = manifest
        .pages
        .iter()
        .flat_map(|page| outputs_by_page.get(&page.id).cloned().unwrap_or_default())
        .collect();
    processing.save(ctx.dir).map_err(|e| e.to_string())?;
    Ok(processing)
}

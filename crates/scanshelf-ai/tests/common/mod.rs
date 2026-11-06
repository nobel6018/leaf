//! 합성 문서 폴더와 가짜 CLI. 실제 회사 정보는 쓰지 않는다
#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use scanshelf_core::ocr::{BlockKind, OcrBlock, OcrLine, OcrPage, Rect};
use scanshelf_core::processing::{OutputPage, PIPELINE_VERSION, PagePart, ProcessOptions, Processing};
use scanshelf_core::{DocumentKind, Manifest, Page, PageLayout, SCHEMA_VERSION};
use uuid::Uuid;

pub struct FakePage {
    pub blocks: Vec<(BlockKind, Vec<&'static str>)>,
    /// None이면 OCR 결과 없음
    pub has_ocr: bool,
    pub image_size: Option<(u32, u32)>,
}

impl FakePage {
    pub fn text(blocks: Vec<(BlockKind, Vec<&'static str>)>) -> Self {
        Self {
            blocks,
            has_ocr: true,
            image_size: Some((60, 80)),
        }
    }
}

pub struct FakeDocument {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

pub fn rect(y: f64) -> Rect {
    Rect {
        x: 0.1,
        y,
        width: 0.8,
        height: 0.03,
    }
}

pub fn make_document(
    root: &Path,
    name: &str,
    session: Option<&str>,
    pages: Vec<FakePage>,
) -> FakeDocument {
    let id = Uuid::new_v4();
    let dir = root.join(id.to_string());
    fs::create_dir_all(dir.join("raw")).unwrap();
    fs::create_dir_all(dir.join("pages")).unwrap();
    fs::create_dir_all(dir.join("ocr")).unwrap();
    // 정오(UTC)라 어느 시간대에서 돌려도 촬영일이 같다
    let created_at = Utc.with_ymd_and_hms(2026, 11, 5, 12, 0, 0).unwrap();

    let mut manifest_pages = Vec::new();
    let mut outputs = Vec::new();
    for page in pages {
        let page_id = Uuid::new_v4();
        manifest_pages.push(Page {
            extra_rotation: 0,
            id: page_id,
            raw: format!("raw/{page_id}.heic"),
            captured_at: created_at,
            layout: PageLayout::Single,
            quad: None,
        });
        let output_id = PagePart::Whole.output_id(page_id);
        let image = format!("pages/{output_id}.jpg");
        if let Some((w, h)) = page.image_size {
            let img = image::RgbImage::from_fn(w, h, |x, y| {
                image::Rgb([(x % 256) as u8, (y % 256) as u8, 200])
            });
            img.save(dir.join(&image)).unwrap();
        }
        let ocr = page.has_ocr.then(|| {
            let mut y = 0.05;
            let blocks: Vec<OcrBlock> = page
                .blocks
                .iter()
                .map(|(kind, lines)| {
                    let block_lines: Vec<OcrLine> = lines
                        .iter()
                        .map(|text| {
                            y += 0.04;
                            OcrLine {
                                text: text.to_string(),
                                bbox: rect(y),
                                confidence: 0.99,
                            }
                        })
                        .collect();
                    OcrBlock {
                        kind: *kind,
                        text: lines.join("\n"),
                        bbox: rect(y),
                        lines: block_lines,
                    }
                })
                .collect();
            let text = blocks
                .iter()
                .map(|b| b.text.clone())
                .collect::<Vec<_>>()
                .join("\n\n");
            let ocr_page = OcrPage {
                engine: "vision-recognize-documents".into(),
                image_width: 60,
                image_height: 80,
                text,
                blocks,
            };
            let relative = format!("ocr/{output_id}.json");
            fs::write(dir.join(&relative), serde_json::to_vec(&ocr_page).unwrap()).unwrap();
            relative
        });
        outputs.push(OutputPage {
            id: output_id,
            source_page: page_id,
            part: PagePart::Whole,
            image,
            ocr,
            options: ProcessOptions::default(),
            pipeline_version: PIPELINE_VERSION,
            processed_at: created_at,
        });
    }

    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        id,
        name: name.to_string(),
        kind: DocumentKind::Pamphlet,
        session: session.map(str::to_string),
        created_at,
        pages: manifest_pages,
    };
    manifest.save(&dir).unwrap();
    let processing = Processing {
        defaults: ProcessOptions::default(),
        outputs,
    };
    fs::write(
        dir.join("processing.json"),
        serde_json::to_vec_pretty(&processing).unwrap(),
    )
    .unwrap();
    FakeDocument { dir, manifest }
}

/// 부를 때마다 받은 프롬프트를 `prompt-N.txt`에, 인자를 `args-N.txt`에 남기고 `resp-N.txt`를 찍는 가짜 claude
pub struct FakeCli {
    pub dir: PathBuf,
    pub program: PathBuf,
}

impl FakeCli {
    pub fn new(root: &Path, responses: &[&str]) -> Self {
        let dir = root.join(format!("fake-cli-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).unwrap();
        for (i, response) in responses.iter().enumerate() {
            fs::write(dir.join(format!("resp-{}.txt", i + 1)), response).unwrap();
        }
        let script = r#"#!/bin/sh
dir="$(dirname "$0")"
n=$(cat "$dir/count" 2>/dev/null || echo 0)
n=$((n+1))
echo "$n" > "$dir/count"
for a in "$@"; do printf '[%s]\n' "$a"; done > "$dir/args-$n.txt"
cat > "$dir/prompt-$n.txt"
cat "$dir/resp-$n.txt"
"#;
        Self::with_script(dir, script)
    }

    pub fn script(root: &Path, body: &str) -> Self {
        let dir = root.join(format!("fake-cli-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).unwrap();
        Self::with_script(dir, &format!("#!/bin/sh\n{body}\n"))
    }

    fn with_script(dir: PathBuf, script: &str) -> Self {
        let program = dir.join("claude");
        fs::write(&program, script).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir, program }
    }

    pub fn calls(&self) -> usize {
        fs::read_to_string(self.dir.join("count"))
            .map(|s| s.trim().parse().unwrap())
            .unwrap_or(0)
    }

    pub fn prompt(&self, n: usize) -> String {
        fs::read_to_string(self.dir.join(format!("prompt-{n}.txt"))).unwrap()
    }

    pub fn args(&self, n: usize) -> String {
        fs::read_to_string(self.dir.join(format!("args-{n}.txt"))).unwrap()
    }
}

pub const SUMMARY_JSON: &str = r#"{
  "company": "Orbitek Demo",
  "country": "독일",
  "event": "팜플렛에 적힌 전시회",
  "products": "랙 운반용 자율 이동 로봇",
  "keySpecs": "최대 하중 800 kg, 속도 1.2 m/s",
  "targetMarket": "데이터센터 시공사",
  "references": "",
  "relevance": ["competitor", "supplier"],
  "relevanceNote": "데이터센터 랙 운반 로봇을 판다",
  "contact": "www.orbitek-demo.example, info@orbitek-demo.example"
}"#;

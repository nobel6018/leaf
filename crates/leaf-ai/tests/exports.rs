mod common;

use std::fs;

use chrono::Utc;
use common::{FakePage, make_document};
use leaf_ai::bundle::BUNDLE_MARKDOWN;
use leaf_ai::{BundleOptions, DocxOptions, export_ai_bundle, export_docx};
use leaf_core::ocr::BlockKind;
use leaf_core::summary::{PamphletSummary, Relevance};

fn document(root: &std::path::Path) -> common::FakeDocument {
    let mut big = FakePage::text(vec![
        (BlockKind::Title, vec!["Orbitek Demo"]),
        (BlockKind::Paragraph, vec!["랙을 옮기는", "자율 이동 로봇"]),
        (
            BlockKind::List,
            vec!["\u{2022} 하중 800 kg", "\u{2022} 속도 1.2 m/s"],
        ),
    ]);
    big.image_size = Some((3000, 1000));
    make_document(
        root,
        "2026 예시박람회_오르비텍",
        Some("2026 예시박람회"),
        vec![
            big,
            FakePage::text(vec![(
                BlockKind::Table,
                vec!["모델", "X1", "하중", "800 kg"],
            )]),
        ],
    )
}

#[test]
fn word_파일에_블록_글자와_한글_글꼴이_들어간다() {
    let root = tempfile::tempdir().unwrap();
    let doc = document(root.path());

    let path = export_docx(&doc.dir, &DocxOptions::default()).unwrap();

    assert_eq!(path, doc.dir.join("exports/2026 예시박람회_오르비텍.docx"));
    let bytes = fs::read(&path).unwrap();
    let parsed = docx_rs::read_docx(&bytes).unwrap().json();
    for text in [
        "Orbitek Demo",
        "랙을 옮기는 자율 이동 로봇",
        "하중 800 kg",
        "X1",
    ] {
        assert!(parsed.contains(text), "{text} 없음");
    }
    assert!(parsed.contains("Apple SD Gothic Neo") || parsed.contains("Malgun Gothic"));
}

#[test]
fn word_파일에_페이지_이미지를_넣을_수_있다() {
    let root = tempfile::tempdir().unwrap();
    let doc = document(root.path());
    let without = fs::read(export_docx(&doc.dir, &DocxOptions::default()).unwrap()).unwrap();

    let options = DocxOptions {
        include_images: true,
        ..DocxOptions::default()
    };
    let with = fs::read(export_docx(&doc.dir, &options).unwrap()).unwrap();

    assert!(with.len() > without.len());
    assert!(docx_rs::read_docx(&with).is_ok());
}

#[test]
fn ai_묶음은_마크다운과_줄인_jpeg를_담는다() {
    let root = tempfile::tempdir().unwrap();
    let doc = document(root.path());
    let summary = PamphletSummary {
        company: "Orbitek Demo".into(),
        country: "독일".into(),
        event: "2026 예시박람회".into(),
        date: "2026-11-05".into(),
        products: "랙 운반 | 로봇".into(),
        key_specs: String::new(),
        target_market: String::new(),
        references: String::new(),
        relevance: vec![Relevance::Competitor],
        relevance_note: "같은 시장".into(),
        contact: String::new(),
        source_file: String::new(),
        generated_by: "claude -p".into(),
        generated_at: Utc::now(),
        edited_fields: vec![],
    };
    fs::write(
        doc.dir.join("summary.json"),
        serde_json::to_vec(&summary).unwrap(),
    )
    .unwrap();

    let dir = export_ai_bundle(&doc.dir, &BundleOptions::default()).unwrap();

    assert_eq!(dir, doc.dir.join("exports/2026 예시박람회_오르비텍-ai"));
    let md = fs::read_to_string(dir.join(BUNDLE_MARKDOWN)).unwrap();
    assert!(md.contains("| 박람회(세션) | 2026 예시박람회 |"));
    assert!(md.contains("| 우리 회사와의 관련성 | 경쟁사 |"));
    assert!(md.contains("| 주력 제품/기술 | 랙 운반 \\| 로봇 |"));
    let page1 = md
        .find("<!-- page 1 -->\n### 페이지 1\n\n![페이지 1](images/page-01.jpg)")
        .unwrap();
    let page2 = md.find("<!-- page 2 -->").unwrap();
    assert!(page1 < page2);
    assert!(md.contains("자율 이동 로봇"));

    let image = image::open(dir.join("images/page-01.jpg")).unwrap();
    assert_eq!((image.width(), image.height()), (2000, 667));
    let small = image::open(dir.join("images/page-02.jpg")).unwrap();
    assert_eq!(
        (small.width(), small.height()),
        (60, 80),
        "작은 이미지는 키우지 않는다"
    );
}

#[test]
fn ai_묶음을_다시_만들면_예전_파일이_남지_않는다() {
    let root = tempfile::tempdir().unwrap();
    let doc = document(root.path());
    let dir = export_ai_bundle(&doc.dir, &BundleOptions::default()).unwrap();
    fs::write(dir.join("images/stale.jpg"), b"x").unwrap();

    let options = BundleOptions {
        include_images: false,
        ..BundleOptions::default()
    };
    let again = export_ai_bundle(&doc.dir, &options).unwrap();

    assert_eq!(again, dir);
    assert!(!dir.join("images/stale.jpg").exists());
    assert!(!dir.join("images/page-01.jpg").exists());
    let leftovers: Vec<_> = fs::read_dir(doc.dir.join("exports"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}

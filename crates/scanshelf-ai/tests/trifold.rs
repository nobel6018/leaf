//! 3단 접지 출력(`{"panel": n}`, id `-P1`~`-P3`)이 AI 묶음 순서에 그대로 들어가는지 본다. 합성 데이터만 쓴다

use std::fs;

use chrono::{TimeZone, Utc};
use scanshelf_ai::document::LoadedDocument;
use scanshelf_core::overrides::Edits;
use scanshelf_core::processing::{OutputPage, PIPELINE_VERSION, PagePart, ProcessOptions, Processing};
use scanshelf_core::{DocumentKind, Manifest, Page, PageLayout, SCHEMA_VERSION};
use uuid::Uuid;

#[test]
fn 세_면_출력은_원본_순서와_면_순서를_따른다() {
    let root = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let dir = root.path().join(id.to_string());
    fs::create_dir_all(dir.join("pages")).unwrap();
    let created_at = Utc.with_ymd_and_hms(2026, 11, 5, 12, 0, 0).unwrap();
    let (fold, cover) = (Uuid::new_v4(), Uuid::new_v4());
    let page = |pid: Uuid, layout| Page { extra_rotation: 0, id: pid, raw: format!("raw/{pid}.heic"), captured_at: created_at, layout, quad: None };
    Manifest {
        schema_version: SCHEMA_VERSION,
        id,
        name: "합성 3단 접지".into(),
        kind: DocumentKind::Pamphlet,
        session: None,
        created_at,
        pages: vec![page(fold, PageLayout::Trifold), page(cover, PageLayout::Single)],
        revision: None,
    }
    .save(&dir)
    .unwrap();
    // Mac에서 표지를 앞으로 옮겼다. processing.json은 아직 옛 순서
    Edits { page_order: Some(vec![cover, fold]), ..Edits::default() }.save(&dir).unwrap();
    let output = |source: Uuid, part: PagePart| {
        let oid = part.output_id(source);
        let image = format!("pages/{oid}.jpg");
        image::RgbImage::from_pixel(8, 8, image::Rgb([200, 200, 200])).save(dir.join(&image)).unwrap();
        OutputPage {
            id: oid,
            source_page: source,
            part,
            image,
            ocr: None,
            options: ProcessOptions::default(),
            pipeline_version: PIPELINE_VERSION,
            processed_at: created_at,
        }
    };
    let outputs = vec![
        output(fold, PagePart::Panel(0)),
        output(fold, PagePart::Panel(1)),
        output(fold, PagePart::Panel(2)),
        output(cover, PagePart::Whole),
    ];
    Processing { defaults: ProcessOptions::default(), outputs }.save(&dir).unwrap();

    let raw = fs::read_to_string(dir.join("processing.json")).unwrap();
    assert!(raw.contains("\"panel\": 2"), "면은 {{\"panel\": n}}으로 저장된다");

    let document = LoadedDocument::load(&dir).unwrap();
    let ids: Vec<&str> = document.pages.iter().map(|p| p.output.id.as_str()).collect();
    let want = [cover.to_string(), format!("{fold}-P1"), format!("{fold}-P2"), format!("{fold}-P3")];
    assert_eq!(ids, want.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(document.pages.iter().map(|p| p.number).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
}

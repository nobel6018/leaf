use std::fs;
use std::path::Path;

use scanshelf_core::overrides::Edits;
use scanshelf_core::{Manifest, Point, Quad};
use uuid::Uuid;

fn manifest() -> Manifest {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/manifest-v1.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn 맥에서_고친_이름과_꼭짓점이_우선한다() {
    let original = manifest();
    let page = original.pages[1].id;
    let quad = Quad {
        top_left: Point { x: 0.1, y: 0.1 },
        top_right: Point { x: 0.9, y: 0.1 },
        bottom_right: Point { x: 0.9, y: 0.9 },
        bottom_left: Point { x: 0.1, y: 0.9 },
    };
    let mut edits = Edits { name: Some("고친 이름".into()), ..Default::default() };
    edits.quads.insert(page, quad);

    let merged = edits.apply(&original);

    assert_eq!(merged.name, "고친 이름");
    assert_eq!(merged.pages[1].quad, Some(quad));
}

#[test]
fn 순서에_없는_새_페이지는_뒤에_붙는다() {
    let mut original = manifest();
    let first = original.pages[0].id;
    let second = original.pages[1].id;
    let mut added = original.pages[0].clone();
    added.id = Uuid::new_v4();
    original.pages.push(added.clone());
    let edits = Edits { page_order: Some(vec![second, first]), ..Default::default() };

    let merged = edits.apply(&original);

    let ids: Vec<Uuid> = merged.pages.iter().map(|p| p.id).collect();
    assert_eq!(ids, vec![second, first, added.id]);
}

#[test]
fn 지운_페이지는_목록에서_빠진다() {
    let original = manifest();
    let edits = Edits { deleted_pages: vec![original.pages[0].id], ..Default::default() };

    let merged = edits.apply(&original);

    assert_eq!(merged.pages.len(), 1);
}

use std::fs;
use std::path::Path;

use leaf_core::{DocumentKind, Manifest, ManifestError, PageLayout, SCHEMA_VERSION};

fn fixture() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/manifest-v1.json");
    fs::read_to_string(path).unwrap()
}

fn document_with(json: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("manifest.json"), json).unwrap();
    dir
}

#[test]
fn 공용_예시_manifest를_읽는다() {
    let dir = document_with(&fixture());

    let manifest = Manifest::load(dir.path()).unwrap();

    assert_eq!(manifest.kind, DocumentKind::Pamphlet);
    assert_eq!(manifest.pages.len(), 2);
    assert_eq!(manifest.pages[1].layout, PageLayout::Spread);
    assert!(manifest.pages[1].quad.is_none());
}

#[test]
fn 저장했다_다시_읽으면_같다() {
    let source = document_with(&fixture());
    let manifest = Manifest::load(source.path()).unwrap();
    let target = tempfile::tempdir().unwrap();

    manifest.save(target.path()).unwrap();

    assert_eq!(Manifest::load(target.path()).unwrap(), manifest);
    let leftovers = fs::read_dir(target.path())
        .unwrap()
        .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn 더_새로운_버전은_거부한다() {
    let json = fixture().replace("\"schemaVersion\": 1", &format!("\"schemaVersion\": {}", SCHEMA_VERSION + 1));
    let dir = document_with(&json);

    let result = Manifest::load(dir.path());

    assert!(matches!(result, Err(ManifestError::UnsupportedVersion { .. })));
}

#[test]
fn 문서_폴더_밖을_가리키는_경로는_거부한다() {
    let json = fixture().replacen("raw/1e9f4b7a", "../../etc/1e9f4b7a", 1);
    let dir = document_with(&json);

    let result = Manifest::load(dir.path());

    assert!(matches!(result, Err(ManifestError::InvalidPage { .. })));
}

#[test]
fn 범위를_벗어난_꼭짓점은_거부한다() {
    let json = fixture().replace("\"x\": 0.95", "\"x\": 1.5");
    let dir = document_with(&json);

    let result = Manifest::load(dir.path());

    assert!(matches!(result, Err(ManifestError::InvalidPage { .. })));
}

#[test]
fn 세_면_접지와_면_출력_id를_읽고_쓴다() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/manifest-v1-trifold.json");
    let dir = document_with(&fs::read_to_string(path).unwrap());

    let manifest = Manifest::load(dir.path()).unwrap();

    assert_eq!(manifest.pages[0].layout, PageLayout::Trifold);
    let part = leaf_core::processing::PagePart::Panel(2);
    assert_eq!(serde_json::to_string(&part).unwrap(), r#"{"panel":2}"#);
    assert!(part.output_id(manifest.pages[0].id).ends_with("-P3"));
}

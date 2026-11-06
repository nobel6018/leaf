//! iCloud inbox 가져오기를 임시 폴더로 확인한다. iCloud 자리표시(`.이름.icloud`)도 흉내 낸다.
//!
//! @see docs/transfer.md

use std::fs;
use std::path::{Path, PathBuf};

use scanshelf_core::Manifest;
use scanshelf_lib::inbox::scan_inbox;
use scanshelf_lib::store::Store;

fn fixture_manifest() -> Manifest {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/manifest-v1.json");
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

struct Setup {
    _root: tempfile::TempDir,
    inbox: PathBuf,
    store: Store,
    manifest: Manifest,
    doc: PathBuf,
}

fn setup() -> Setup {
    let root = tempfile::tempdir().unwrap();
    let inbox = root.path().join("inbox");
    let store = Store::new(&root.path().join("appdata"));
    let manifest = fixture_manifest();
    let doc = inbox.join(manifest.id.to_string());
    fs::create_dir_all(doc.join("raw")).unwrap();
    Setup {
        _root: root,
        inbox,
        store,
        manifest,
        doc,
    }
}

fn write_raw(doc: &Path, rel: &str) {
    fs::write(doc.join(rel), format!("synthetic {rel}")).unwrap();
}

fn placeholder(doc: &Path, rel: &str) -> PathBuf {
    let path = doc.join(rel);
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    path.with_file_name(format!(".{name}.icloud"))
}

#[test]
fn 준비된_문서는_보관함으로_가져오고_inbox에서_지운다() {
    let s = setup();
    for page in &s.manifest.pages {
        write_raw(&s.doc, &page.raw);
    }
    s.manifest.save(&s.doc).unwrap();

    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| panic!("받을 파일이 없다"));

    assert_eq!(scan.imported, vec![s.manifest.id]);
    assert_eq!(scan.pending, 0);
    assert!(!s.doc.exists());
    let library = s.store.document_dir(s.manifest.id);
    assert_eq!(Manifest::load(&library).unwrap(), s.manifest);
    for page in &s.manifest.pages {
        assert!(library.join(&page.raw).is_file());
    }
}

#[test]
fn manifest가_오기_전에는_가져오지_않는다() {
    let s = setup();
    for page in &s.manifest.pages {
        write_raw(&s.doc, &page.raw);
    }

    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});

    assert!(scan.imported.is_empty());
    assert_eq!(scan.pending, 1);
    assert!(!s.store.document_dir(s.manifest.id).exists());
}

#[test]
fn 아직_받지_않은_원본은_내려받기를_요청하고_받아지면_가져온다() {
    let s = setup();
    let (first, second) = (&s.manifest.pages[0].raw, &s.manifest.pages[1].raw);
    write_raw(&s.doc, first);
    fs::write(placeholder(&s.doc, second), b"").unwrap();
    s.manifest.save(&s.doc).unwrap();

    let mut requested = Vec::new();
    let scan = scan_inbox(&s.inbox, &s.store, &mut |p| requested.push(p.to_path_buf()));
    assert!(scan.imported.is_empty());
    assert_eq!(scan.pending, 1);
    assert_eq!(requested, vec![s.doc.join(second)]);

    // iCloud가 내려받으면 자리표시가 실제 파일로 바뀐다
    fs::remove_file(placeholder(&s.doc, second)).unwrap();
    write_raw(&s.doc, second);
    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});
    assert_eq!(scan.imported, vec![s.manifest.id]);
}

#[test]
fn manifest가_자리표시면_내려받기를_요청한다() {
    let s = setup();
    fs::write(placeholder(&s.doc, "manifest.json"), b"").unwrap();

    let mut requested = Vec::new();
    let scan = scan_inbox(&s.inbox, &s.store, &mut |p| requested.push(p.to_path_buf()));
    assert_eq!(scan.pending, 1);
    assert_eq!(requested, vec![s.doc.join("manifest.json")]);
}

#[test]
fn 폴더_이름과_manifest_id가_다르면_가져오지_않는다() {
    let s = setup();
    let other = s.inbox.join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(other.join("raw")).unwrap();
    for page in &s.manifest.pages {
        write_raw(&other, &page.raw);
    }
    s.manifest.save(&other).unwrap();

    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});
    assert!(scan.imported.is_empty());
    assert_eq!(scan.failed.len(), 1);
    assert!(other.exists());
}

#[test]
fn 이미_있는_문서면_새_원본만_더하고_편집은_그대로_둔다() {
    let s = setup();
    for page in &s.manifest.pages {
        write_raw(&s.doc, &page.raw);
    }
    s.manifest.save(&s.doc).unwrap();
    scan_inbox(&s.inbox, &s.store, &mut |_| {});
    let library = s.store.document_dir(s.manifest.id);
    fs::write(library.join("edits.json"), b"{\"name\":\"edited\"}").unwrap();

    // iPhone이 한 장 더 찍어 다시 보냈다. 앞 원본은 다시 올리지 않았다
    let mut manifest = s.manifest.clone();
    let mut extra = manifest.pages[0].clone();
    extra.id = uuid::Uuid::new_v4();
    extra.raw = format!("raw/{}.heic", extra.id);
    manifest.pages.push(extra.clone());
    fs::create_dir_all(s.doc.join("raw")).unwrap();
    write_raw(&s.doc, &extra.raw);
    manifest.save(&s.doc).unwrap();

    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});
    assert_eq!(scan.imported, vec![manifest.id]);
    assert_eq!(Manifest::load(&library).unwrap().pages.len(), 3);
    assert_eq!(fs::read(library.join("edits.json")).unwrap(), b"{\"name\":\"edited\"}");
}

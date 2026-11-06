//! iCloud inbox 가져오기를 임시 폴더로 확인한다. iCloud 자리표시(`.이름.icloud`)도 흉내 낸다.
//!
//! @see docs/transfer.md

use std::fs;
use std::path::{Path, PathBuf};

use scanshelf_core::Manifest;
use scanshelf_lib::inbox::{Receipt, prune_receipts, received_dir, scan_inbox};
use scanshelf_lib::store::{ImportMode, Store, list_raw_files};
use sha2::{Digest, Sha256};

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

/// iPhone이 한 장 더 찍고 그 장의 원본과 manifest만 올린 모습. 앞 장은 Mac에 있다고 믿어 올리지 않는다
fn next_capture(s: &Setup, previous: &Manifest) -> Manifest {
    let mut manifest = previous.clone();
    let mut extra = manifest.pages[0].clone();
    extra.id = uuid::Uuid::new_v4();
    extra.raw = format!("raw/{}.heic", extra.id);
    manifest.pages.push(extra.clone());
    manifest.revision = Some(previous.revision.unwrap_or(0) + 1);
    fs::create_dir_all(s.doc.join("raw")).unwrap();
    write_raw(&s.doc, &extra.raw);
    manifest.save(&s.doc).unwrap();
    manifest
}

fn import_first(s: &Setup) -> Manifest {
    let mut manifest = s.manifest.clone();
    manifest.revision = Some(1);
    for page in &manifest.pages {
        write_raw(&s.doc, &page.raw);
    }
    manifest.save(&s.doc).unwrap();
    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| panic!("받을 파일이 없다"));
    assert_eq!(scan.imported, vec![manifest.id]);
    manifest
}

fn read_receipt(s: &Setup) -> Receipt {
    let path = received_dir(&s.inbox).join(format!("{}.json", s.manifest.id));
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn raw_paths(manifest: &Manifest) -> Vec<String> {
    let mut paths: Vec<String> = manifest.pages.iter().map(|p| p.raw.clone()).collect();
    paths.sort();
    paths
}

#[test]
fn 가져온_뒤_다음_촬영이_앞_원본_없이_와도_합치고_편집과_처리_결과는_그대로_둔다() {
    let s = setup();
    let first = import_first(&s);
    let library = s.store.document_dir(first.id);
    fs::write(library.join("edits.json"), b"{\"name\":\"edited\"}").unwrap();
    fs::write(library.join("processing.json"), b"{\"outputs\":[]}").unwrap();

    let second = next_capture(&s, &first);
    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| panic!("받을 파일이 없다"));

    assert_eq!(scan.imported, vec![first.id]);
    assert_eq!(scan.pending, 0);
    assert_eq!(Manifest::load(&library).unwrap(), second);
    assert_eq!(fs::read(library.join("edits.json")).unwrap(), b"{\"name\":\"edited\"}");
    assert_eq!(fs::read(library.join("processing.json")).unwrap(), b"{\"outputs\":[]}");
    assert!(!s.doc.exists());
}

#[test]
fn 보관함에_있는_원본은_자리표시로_남아_있어도_기다리지_않는다() {
    let s = setup();
    let first = import_first(&s);

    let second = next_capture(&s, &first);
    fs::write(placeholder(&s.doc, &first.pages[0].raw), b"").unwrap();
    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| panic!("내려받을 필요가 없다"));

    assert_eq!(scan.imported, vec![second.id]);
}

#[test]
fn 보관함에도_inbox에도_없는_원본이_있으면_기다린다() {
    let s = setup();
    let first = import_first(&s);

    let second = next_capture(&s, &first);
    fs::remove_file(s.doc.join(&second.pages[2].raw)).unwrap();
    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});

    assert!(scan.imported.is_empty());
    assert_eq!(scan.pending, 1);
    assert_eq!(Manifest::load(&s.store.document_dir(first.id)).unwrap(), first);
}

#[test]
fn 가져온_뒤_받은_원본과_manifest_해시를_확인_파일에_적는다() {
    let s = setup();
    let first = import_first(&s);

    let receipt = read_receipt(&s);

    assert_eq!(receipt.document_id, first.id);
    assert_eq!(receipt.raw, raw_paths(&first));
    let written = fs::read(s.store.document_dir(first.id).join("manifest.json")).unwrap();
    assert_eq!(receipt.manifest_sha256, Some(hex::encode(Sha256::digest(written))));
}

#[test]
fn 확인_파일을_못_쓰면_inbox를_지우지_않고_다음에_다시_적는다() {
    let s = setup();
    fs::write(received_dir(&s.inbox), b"a file blocks the folder").unwrap();
    let mut manifest = s.manifest.clone();
    manifest.revision = Some(1);
    for page in &manifest.pages {
        write_raw(&s.doc, &page.raw);
    }
    manifest.save(&s.doc).unwrap();

    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});
    assert_eq!(scan.failed.len(), 1);
    assert_eq!(scan.pending, 1);
    assert!(s.doc.join("manifest.json").is_file());
    assert!(s.doc.join(&manifest.pages[0].raw).is_file());

    fs::remove_file(received_dir(&s.inbox)).unwrap();
    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});
    assert!(scan.failed.is_empty());
    assert_eq!(read_receipt(&s).raw, raw_paths(&manifest));
    assert!(!s.doc.exists());
}

#[test]
fn manifest에_없는_파일은_다음_장일_수_있어_지우지_않는다() {
    let s = setup();
    let mut manifest = s.manifest.clone();
    manifest.revision = Some(1);
    for page in &manifest.pages {
        write_raw(&s.doc, &page.raw);
    }
    manifest.save(&s.doc).unwrap();
    // 다음 장의 원본은 왔고 그 장을 적은 manifest는 아직이다
    let next = format!("raw/{}.heic", uuid::Uuid::new_v4());
    write_raw(&s.doc, &next);

    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});

    assert_eq!(scan.imported, vec![manifest.id]);
    assert!(s.doc.join(&next).is_file());
    assert!(!s.doc.join("manifest.json").exists());
    assert!(!s.store.document_dir(manifest.id).join(&next).exists());

    // manifest가 오기 전까지는 기다린다. 비기 전에는 폴더를 지우지 않는다
    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});
    assert_eq!(scan.pending, 1);
    assert!(s.doc.join(&next).is_file());

    // iPhone이 지운 장이었다면 원본만 사라지고, 다음 훑기가 빈 폴더를 치운다
    fs::remove_file(s.doc.join(&next)).unwrap();
    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});
    assert_eq!(scan.pending, 0);
    assert!(!s.doc.exists());
}

#[test]
fn 다시_찍고_지운_장은_manifest에서_빠지고_지운_장의_원본은_보관함에_들어가지_않는다() {
    let s = setup();
    let first = import_first(&s);

    // 두 번째 장을 다시 찍었다. 옛 원본은 Mac이 이미 받았고 새 원본만 온다
    let mut retaken = first.clone();
    let old = retaken.pages[1].clone();
    let mut new_page = old.clone();
    new_page.id = uuid::Uuid::new_v4();
    new_page.raw = format!("raw/{}.heic", new_page.id);
    retaken.pages[1] = new_page.clone();
    retaken.revision = Some(2);
    fs::create_dir_all(s.doc.join("raw")).unwrap();
    write_raw(&s.doc, &new_page.raw);
    // 아직 가져가지 않은 다른 장을 찍었다가 지운 흔적. manifest에 없으니 들어가면 안 된다
    let deleted = format!("raw/{}.heic", uuid::Uuid::new_v4());
    write_raw(&s.doc, &deleted);
    retaken.save(&s.doc).unwrap();

    scan_inbox(&s.inbox, &s.store, &mut |_| {});

    let library = s.store.document_dir(first.id);
    let loaded = Manifest::load(&library).unwrap();
    assert_eq!(loaded, retaken);
    assert!(library.join(&new_page.raw).is_file());
    assert!(!library.join(&deleted).exists());
    // Mac은 받은 원본을 지우지 않는다. 목록에서만 빠진다
    assert!(library.join(&old.raw).is_file());
}

#[test]
fn 늦게_도착한_옛_manifest는_새_manifest를_덮지_않고_inbox에서_치운다() {
    let s = setup();
    let first = import_first(&s);
    let second = next_capture(&s, &first);
    scan_inbox(&s.inbox, &s.store, &mut |_| {});
    let library = s.store.document_dir(first.id);
    assert_eq!(Manifest::load(&library).unwrap(), second);

    // iPhone에서 한 장을 지운 판(3)을 Wi-Fi로 받았다
    let mut third = second.clone();
    let removed = third.pages.remove(1);
    third.revision = Some(3);
    let staging = s.store.staging_dir(first.id);
    s.store.import(&third, &staging, ImportMode::Move).unwrap();

    // 지운 장이 들어 있는 판(2)이 iCloud에서 늦게 왔다. 원본이 빠져 있어도 기다리지 않는다
    fs::create_dir_all(s.doc.join("raw")).unwrap();
    second.save(&s.doc).unwrap();
    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});

    assert_eq!(scan.imported, vec![first.id]);
    assert_eq!(scan.pending, 0);
    let loaded = Manifest::load(&library).unwrap();
    assert_eq!(loaded, third);
    assert!(!loaded.pages.iter().any(|p| p.id == removed.id));
    assert!(!s.doc.exists());
}

#[test]
fn 옛_manifest의_새_원본은_보관함에_넣지_않는다() {
    let s = setup();
    let first = import_first(&s);
    let library = s.store.document_dir(first.id);
    let mut newer = first.clone();
    newer.revision = Some(5);
    newer.save(&library).unwrap();

    let stale = next_capture(&s, &first);
    assert_eq!(stale.revision, Some(2));
    scan_inbox(&s.inbox, &s.store, &mut |_| {});

    assert!(!library.join(&stale.pages[2].raw).exists());
    assert_eq!(Manifest::load(&library).unwrap(), newer);
}

#[test]
fn 와이파이로_받은_문서가_inbox로도_오면_페이지가_늘지_않는다() {
    let s = setup();
    let mut manifest = s.manifest.clone();
    manifest.revision = Some(1);
    let staging = s.store.staging_dir(manifest.id);
    fs::create_dir_all(staging.join("raw")).unwrap();
    for page in &manifest.pages {
        write_raw(&staging, &page.raw);
    }
    s.store.import(&manifest, &staging, ImportMode::Move).unwrap();
    let library = s.store.document_dir(manifest.id);
    let before = fs::read(library.join(&manifest.pages[0].raw)).unwrap();

    for page in &manifest.pages {
        fs::write(s.doc.join(&page.raw), b"same page via icloud").unwrap();
    }
    manifest.save(&s.doc).unwrap();
    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});

    assert_eq!(scan.imported, vec![manifest.id]);
    let loaded = Manifest::load(&library).unwrap();
    assert_eq!(loaded.pages.len(), manifest.pages.len());
    assert_eq!(list_raw_files(&library).unwrap().len(), manifest.pages.len());
    // 이미 있는 원본은 덮어쓰지 않는다
    assert_eq!(fs::read(library.join(&manifest.pages[0].raw)).unwrap(), before);
    assert!(!s.doc.exists());
}

#[test]
fn 보관함에서_문서를_지웠으면_확인_파일을_비워_다시_올리게_한다() {
    let s = setup();
    let first = import_first(&s);
    assert!(!read_receipt(&s).raw.is_empty());
    fs::remove_dir_all(s.store.document_dir(first.id)).unwrap();

    // iPhone은 앞 장이 Mac에 있다고 믿고 새 장만 올렸다
    let second = next_capture(&s, &first);
    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});
    assert_eq!(scan.pending, 1);
    let receipt = read_receipt(&s);
    assert!(receipt.raw.is_empty());
    assert_eq!(receipt.manifest_sha256, None);

    // 빈 확인 파일을 본 iPhone이 전부 다시 올리면 문서가 다시 만들어진다
    for page in &second.pages {
        write_raw(&s.doc, &page.raw);
    }
    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});
    assert_eq!(scan.imported, vec![second.id]);
    assert_eq!(read_receipt(&s).raw, raw_paths(&second));
}

#[test]
fn 처음_오는_문서의_원본이_늦어도_확인_파일을_만들지_않는다() {
    let s = setup();
    s.manifest.save(&s.doc).unwrap();

    let scan = scan_inbox(&s.inbox, &s.store, &mut |_| {});

    assert_eq!(scan.pending, 1);
    assert!(!received_dir(&s.inbox).exists());
}

#[test]
fn 오래된_확인_파일만_지운다() {
    let s = setup();
    import_first(&s);
    let path = received_dir(&s.inbox).join(format!("{}.json", s.manifest.id));

    prune_receipts(&s.inbox, std::time::Duration::from_secs(3600));
    assert!(path.is_file());

    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
    fs::File::options().write(true).open(&path).unwrap().set_modified(old).unwrap();
    prune_receipts(&s.inbox, std::time::Duration::from_secs(3600));
    assert!(!path.exists());
}

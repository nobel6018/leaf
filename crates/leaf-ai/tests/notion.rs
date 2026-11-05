mod common;

use std::fs;

use chrono::Utc;
use common::{FakePage, make_document};
use leaf_ai::export_notion_csv;
use leaf_ai::notion::{NOTION_COLUMNS, session_document_dirs};
use leaf_core::ocr::BlockKind;
use leaf_core::summary::{PamphletSummary, Relevance};

fn summary(company: &str) -> PamphletSummary {
    PamphletSummary {
        company: company.into(),
        country: "대한민국".into(),
        event: "2026 예시박람회".into(),
        date: "2026-11-05".into(),
        products: "랙 운반 로봇, \"AMR\" 시리즈".into(),
        key_specs: "하중 800 kg\r\n속도 1.2 m/s".into(),
        target_market: "데이터센터".into(),
        references: String::new(),
        relevance: vec![Relevance::Competitor, Relevance::PartnerCandidate],
        relevance_note: "같은 시장".into(),
        contact: "info@example.com".into(),
        source_file: format!("2026 예시박람회_{company}"),
        generated_by: "claude -p".into(),
        generated_at: Utc::now(),
        edited_fields: vec![],
    }
}

fn write_summary(dir: &std::path::Path, summary: &PamphletSummary) {
    fs::write(
        dir.join("summary.json"),
        serde_json::to_vec(summary).unwrap(),
    )
    .unwrap();
}

#[test]
fn bom과_crlf가_붙고_csv_리더로_다시_읽으면_값이_같다() {
    let root = tempfile::tempdir().unwrap();
    let a = make_document(root.path(), "a", Some("2026 예시박람회"), vec![]);
    let b = make_document(root.path(), "b", Some("2026 예시박람회"), vec![]);
    write_summary(&a.dir, &summary("가나 로보틱스"));
    write_summary(&b.dir, &summary("Demo, Inc."));
    let out = root.path().join("out/notion.csv");

    let report = export_notion_csv(&[a.dir.clone(), b.dir.clone()], &out).unwrap();

    assert_eq!(report.rows, 2);
    assert!(report.skipped.is_empty());
    let bytes = fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"\xEF\xBB\xBF"));
    assert!(String::from_utf8_lossy(&bytes).contains("원본 파일명\r\n"));

    let mut reader = csv::Reader::from_reader(&bytes[3..]);
    let headers: Vec<String> = reader
        .headers()
        .unwrap()
        .iter()
        .map(str::to_string)
        .collect();
    assert_eq!(headers, NOTION_COLUMNS);
    let rows: Vec<csv::StringRecord> = reader.records().map(Result::unwrap).collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(&rows[1][0], "Demo, Inc.");
    assert_eq!(&rows[0][4], "랙 운반 로봇, \"AMR\" 시리즈");
    assert_eq!(&rows[0][5], "하중 800 kg\n속도 1.2 m/s");
    assert_eq!(&rows[0][8], "경쟁사, 협력 후보");
    assert_eq!(&rows[0][10], "2026 예시박람회_가나 로보틱스");
}

#[test]
fn bom이_붙은_채로_읽어도_첫_열_이름이_맞다() {
    let root = tempfile::tempdir().unwrap();
    let a = make_document(root.path(), "a", None, vec![]);
    write_summary(&a.dir, &summary("가나"));
    let out = root.path().join("notion.csv");

    export_notion_csv(&[a.dir], &out).unwrap();

    let mut reader = csv::Reader::from_path(&out).unwrap();
    assert_eq!(&reader.headers().unwrap()[0], "회사명");
}

#[test]
fn 정리가_없는_문서는_건너뛰고_알려_준다() {
    let root = tempfile::tempdir().unwrap();
    let a = make_document(root.path(), "a", None, vec![]);
    let b = make_document(root.path(), "b", None, vec![]);
    write_summary(&a.dir, &summary("가나"));
    fs::write(b.dir.join("summary.json"), b"{ broken").unwrap();
    let missing = make_document(root.path(), "c", None, vec![]);

    let report = export_notion_csv(
        &[a.dir, b.dir.clone(), missing.dir.clone()],
        &root.path().join("n.csv"),
    )
    .unwrap();

    assert_eq!(report.rows, 1);
    let skipped: Vec<_> = report.skipped.iter().map(|s| s.dir.clone()).collect();
    assert_eq!(skipped, vec![b.dir, missing.dir]);
}

#[test]
fn 세션이_같은_문서를_촬영_순서로_찾고_맥에서_고친_세션을_따른다() {
    let root = tempfile::tempdir().unwrap();
    let page = || vec![FakePage::text(vec![(BlockKind::Title, vec!["x"])])];
    let a = make_document(root.path(), "a", Some("박람회 A"), page());
    let b = make_document(root.path(), "b", Some("박람회 B"), page());
    let c = make_document(root.path(), "c", Some("박람회 C"), page());
    fs::write(c.dir.join("edits.json"), r#"{"session": "박람회 A"}"#).unwrap();

    let mut found = session_document_dirs(root.path(), "박람회 A");
    found.sort();
    let mut expected = vec![a.dir, c.dir];
    expected.sort();
    assert_eq!(found, expected);
    assert!(!session_document_dirs(root.path(), "박람회 A").contains(&b.dir));
}

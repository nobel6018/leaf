mod common;

use common::{FakeCli, FakePage, make_document};
use leaf_ai::{LlmConfig, NameSuggestion, suggest_document_name};
use leaf_core::ocr::BlockKind;

#[test]
fn 표지_회사명으로_세션_회사명을_추천하고_edits_json은_쓰지_않는다() {
    let root = tempfile::tempdir().unwrap();
    let doc = make_document(
        root.path(),
        "이름 없음",
        Some("2026 예시박람회"),
        vec![
            FakePage::text(vec![(BlockKind::Title, vec!["Orbitek Demo GmbH"])]),
            FakePage::text(vec![(
                BlockKind::Paragraph,
                vec!["둘째 페이지는 보내지 않는다"],
            )]),
        ],
    );
    let cli = FakeCli::new(root.path(), &[r#"{"company": "Orbitek/Demo"}"#]);

    let suggestion =
        suggest_document_name(&doc.dir, &LlmConfig::with_program(&cli.program)).unwrap();

    assert_eq!(
        suggestion,
        Some(NameSuggestion {
            company: "Orbitek_Demo".into(),
            name: "2026 예시박람회_Orbitek_Demo".into(),
        })
    );
    assert!(!doc.dir.join("edits.json").exists());
    let prompt = cli.prompt(1);
    assert!(prompt.contains("Orbitek Demo GmbH"));
    assert!(!prompt.contains("둘째 페이지"));
}

#[test]
fn 세션이_없으면_회사명만_추천한다() {
    let root = tempfile::tempdir().unwrap();
    let doc = make_document(
        root.path(),
        "이름 없음",
        None,
        vec![FakePage::text(vec![(BlockKind::Title, vec!["예시 정밀"])])],
    );
    let cli = FakeCli::new(root.path(), &[r#"{"company": "예시 정밀"}"#]);

    let suggestion =
        suggest_document_name(&doc.dir, &LlmConfig::with_program(&cli.program)).unwrap();

    assert_eq!(suggestion.unwrap().name, "예시 정밀");
}

#[test]
fn 회사명을_못_찾으면_none() {
    let root = tempfile::tempdir().unwrap();
    let doc = make_document(
        root.path(),
        "이름 없음",
        None,
        vec![FakePage::text(vec![(
            BlockKind::Title,
            vec!["SMART SOLUTION"],
        )])],
    );
    let cli = FakeCli::new(root.path(), &[r#"{"company": ""}"#]);

    let suggestion =
        suggest_document_name(&doc.dir, &LlmConfig::with_program(&cli.program)).unwrap();

    assert_eq!(suggestion, None);
}

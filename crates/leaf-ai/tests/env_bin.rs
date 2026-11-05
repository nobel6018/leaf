//! 환경 변수를 바꾸는 테스트라 다른 테스트와 같은 프로세스에서 돌리지 않으려고 파일을 나눴다. 테스트는 하나만 둔다

mod common;

use common::{FakeCli, FakePage, SUMMARY_JSON, make_document};
use leaf_ai::cli::CLAUDE_BIN_ENV;
use leaf_ai::{LlmConfig, summarize_pamphlet};
use leaf_core::ocr::BlockKind;

#[test]
fn leaf_claude_bin으로_실행_파일을_바꿔_끼운다() {
    let root = tempfile::tempdir().unwrap();
    let doc = make_document(
        root.path(),
        "문서",
        None,
        vec![FakePage::text(vec![(
            BlockKind::Title,
            vec!["Orbitek Demo"],
        )])],
    );
    let cli = FakeCli::new(root.path(), &[SUMMARY_JSON]);
    // 이 파일의 유일한 테스트라 다른 스레드가 환경 변수를 읽지 않는다
    unsafe { std::env::set_var(CLAUDE_BIN_ENV, &cli.program) };

    let config = LlmConfig::default();
    assert_eq!(config.resolve_program().unwrap(), cli.program);
    let summary = summarize_pamphlet(&doc.dir, &config).unwrap();

    assert_eq!(summary.company, "Orbitek Demo");
    assert_eq!(cli.calls(), 1);
}

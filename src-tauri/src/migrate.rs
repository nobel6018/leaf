//! 앱 이름을 바꾸기 전(`com.nobel6018.leaf`)의 데이터를 옮긴다. 번들 id가 바뀌면 데이터 폴더도 달라져, 옮기지 않으면
//! 이미 받은 문서와 내려받은 모델이 보이지 않는다. 이미 새 폴더에 있는 항목은 건드리지 않아 여러 번 돌아도 안전하다.
//!
//! 기기 목록은 옮기지 않는다. 페어링 토큰이 옛 이름의 키체인 항목에 있어 다시 페어링해야 한다.
//!
//! @see docs/transfer.md

use std::fs;
use std::path::Path;

pub const LEGACY_APP_ID: &str = "com.nobel6018.leaf";

/// 옮길 폴더. CoreML 캐시는 다시 만들어지므로 그대로 두어도 된다
const MOVED_DIRS: [&str; 2] = ["library", "models"];

/// app_data_dir: 새 데이터 폴더. 옛 폴더는 같은 부모 아래에 있다. 옮긴 항목 수를 돌려준다
pub fn migrate_legacy_data(app_data_dir: &Path) -> usize {
    let Some(parent) = app_data_dir.parent() else {
        return 0;
    };
    let legacy = parent.join(LEGACY_APP_ID);
    let mut moved = 0;
    for dir in MOVED_DIRS {
        moved += move_entries(&legacy.join(dir), &app_data_dir.join(dir));
    }
    moved
}

fn move_entries(from: &Path, to: &Path) -> usize {
    let Ok(entries) = fs::read_dir(from) else {
        return 0;
    };
    if let Err(error) = fs::create_dir_all(to) {
        eprintln!("{} 만들기 실패: {error}", to.display());
        return 0;
    }
    let mut moved = 0;
    for entry in entries.flatten() {
        let target = to.join(entry.file_name());
        if target.exists() {
            continue;
        }
        match fs::rename(entry.path(), &target) {
            Ok(()) => moved += 1,
            Err(error) => eprintln!("{} 옮기기 실패: {error}", entry.path().display()),
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn 옛_문서와_모델을_옮기고_이미_있는_것은_덮지_않는다() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_APP_ID);
        let new = root.path().join("com.nobel6018.scanshelf");
        write(&legacy.join("library/a/manifest.json"), "옛 문서 a");
        write(&legacy.join("library/b/manifest.json"), "옛 문서 b");
        write(&legacy.join("models/uvdoc.onnx"), "모델");
        write(&new.join("library/b/manifest.json"), "새 문서 b");

        assert_eq!(migrate_legacy_data(&new), 2);

        assert_eq!(fs::read_to_string(new.join("library/a/manifest.json")).unwrap(), "옛 문서 a");
        assert_eq!(fs::read_to_string(new.join("library/b/manifest.json")).unwrap(), "새 문서 b");
        assert!(new.join("models/uvdoc.onnx").exists());
        // 겹친 문서의 옛 사본은 지우지 않는다
        assert!(legacy.join("library/b/manifest.json").exists());
    }

    #[test]
    fn 옛_폴더가_없으면_아무것도_하지_않고_다시_돌려도_안전하다() {
        let root = tempfile::tempdir().unwrap();
        let new = root.path().join("com.nobel6018.scanshelf");
        assert_eq!(migrate_legacy_data(&new), 0);
        assert!(!new.exists());
    }
}

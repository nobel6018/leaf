//! macOS 앱 메뉴(leaf)의 "leaf에 관하여" 아래에 "업데이트 확인하기…"를 넣는다. 누르면 화면에 check-update를 보낸다.
//! 설치와 다시 켜기는 화면(src/update/useAppUpdate.ts)이 사람이 버튼을 누를 때만 한다.
//!
//! @see docs/update.md

#[cfg(target_os = "macos")]
pub fn add_update_menu(app: &tauri::App) -> tauri::Result<()> {
    use tauri::Emitter;
    use tauri::menu::{Menu, MenuItem, MenuItemKind};

    let menu = Menu::default(app.handle())?;
    if let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() {
        let label = if system_prefers_korean() { "업데이트 확인하기…" } else { "Check for Updates…" };
        let item = MenuItem::with_id(app, "check-update", label, true, None::<&str>)?;
        app_menu.insert(&item, 1)?;
    }
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        if event.id() == "check-update" {
            let _ = app.emit("check-update", ());
        }
    });
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn add_update_menu(_app: &tauri::App) -> tauri::Result<()> {
    Ok(())
}

/// 시스템의 첫 번째 선호 언어가 한국어인지. 앱 메뉴는 웹 화면이 아니라 시스템 언어를 따른다
#[cfg(target_os = "macos")]
fn system_prefers_korean() -> bool {
    objc2_foundation::NSLocale::preferredLanguages()
        .firstObject()
        .is_some_and(|language| language.to_string().starts_with("ko"))
}

import AppIntents

// 단축어를 따로 만들지 않아도 단축어 앱, Spotlight, 동작 버튼 목록에 바로 보이는 leaf 동작.
// 무료 Apple ID 서명에서도 된다(App Intents는 entitlement가 필요 없다).
// @see docs/ios.md "스캔 시작 동작"

struct LeafShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: StartScanIntent(),
            phrases: [
                "\(.applicationName) 스캔 시작",
                "\(.applicationName)로 스캔",
                "\(.applicationName)로 \(\.$kind) 스캔",
            ],
            shortTitle: "스캔 시작",
            systemImageName: "doc.viewfinder"
        )
    }
}

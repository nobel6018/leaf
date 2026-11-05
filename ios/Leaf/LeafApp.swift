import SwiftUI

@main
struct LeafApp: App {
    @State private var app = AppModel()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            NavigationStack {
                DocumentsView()
            }
            .environment(app)
            .tint(.green)
            .onChange(of: scenePhase) { _, phase in
                // 보내기는 앱이 앞에 있을 때만 한다(docs/ios.md 알려진 한계). 앞으로 올 때마다 밀린 것을 보내고 기한 지난 원본을 지운다
                guard phase == .active else { return }
                Task {
                    await app.sendPending()
                    app.applyRetention()
                }
                // 새 버전 확인은 하루에 한 번만 GitHub에 묻는다(docs/ios.md "업데이트 알림")
                Task { await app.checkForUpdate() }
            }
        }
    }
}

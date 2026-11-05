import AppIntents
import SwiftUI
import WidgetKit

// 제어 센터, 잠금 화면, 동작 버튼에 놓는 "스캔 시작" 버튼(iOS 18 컨트롤).
// 무료 Apple ID로도 서명되지만 확장이라 App ID를 하나 더 쓴다(7일에 10개 한도). docs/ios.md "스캔 시작 동작"

@main
struct LeafControlsBundle: WidgetBundle {
    var body: some Widget {
        StartScanControl()
    }
}

struct StartScanControl: ControlWidget {
    static let kind = "com.nobel6018.leaf.start-scan"

    var body: some ControlWidgetConfiguration {
        StaticControlConfiguration(kind: Self.kind) {
            ControlWidgetButton(action: StartScanIntent()) {
                Label("스캔 시작", systemImage: "doc.viewfinder")
            }
        }
        .displayName("leaf 스캔 시작")
        .description("마지막에 고른 종류와 묶음으로 새 문서 촬영을 엽니다.")
    }
}

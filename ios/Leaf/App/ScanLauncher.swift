import Foundation

// "스캔 시작" 동작(동작 버튼, 단축어, Spotlight, 제어 센터)이 앱을 연 뒤 새 문서 촬영으로 들어가는 길.
// 인텐트는 앱을 앞으로 띄우고 요청만 남긴다. 화면이 아직 없을 수 있어(앱이 꺼져 있다 켜질 때) 문서 목록이 나타날 때 가져간다.
// @see docs/ios.md "스캔 시작 동작"

/// 새 문서를 무엇으로 시작할지
struct ScanStartPlan: Equatable, Sendable {
    var kind: DocumentKind
    var session: String
    var layout: PageLayout

    /// 종류를 정하지 않았으면 마지막에 고른 종류. 묶음과 배치는 앱에서 마지막에 쓴 값을 이어 간다
    @MainActor
    static func make(requested: DocumentKind?, settings: AppSettings) -> ScanStartPlan {
        let kind = requested ?? settings.lastKind
        return ScanStartPlan(kind: kind, session: settings.lastSession, layout: settings.lastLayout(for: kind))
    }
}

struct ScanRequest: Equatable, Sendable {
    /// 같은 종류로 두 번 눌러도 다른 요청으로 보이게 한다
    var id = UUID()
    var kind: DocumentKind?
}

@MainActor
@Observable
final class ScanLauncher {
    static let shared = ScanLauncher()

    private(set) var pending: ScanRequest?

    func request(kind: DocumentKind?) {
        pending = ScanRequest(kind: kind)
    }

    /// 화면이 요청을 한 번만 처리하게 꺼내면서 지운다
    func take() -> ScanRequest? {
        defer { pending = nil }
        return pending
    }
}

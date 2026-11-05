import Foundation

// 기기 안 설정. 비밀 값(Mac 토큰)은 여기 두지 않고 키체인에 둔다.
// @see docs/ios.md

/// 짝지은 Mac. 주소는 바뀔 수 있어 보낼 때마다 Bonjour 이름으로 다시 찾고, 못 찾으면 마지막 주소를 쓴다
struct PairedMac: Codable, Equatable, Sendable {
    var serviceName: String
    var macName: String
    var lastAddress: MacAddress?
    var pairedAt: Date
}

@MainActor
@Observable
final class AppSettings {
    /// 원본 자동 삭제 기간 선택지. 0은 지우지 않음
    static let retentionChoices = [1, 3, 7, 14, 30, 0]
    static let defaultRetentionDays = 7

    private let defaults: UserDefaults

    /// 묶음 이름(박람회명, 책 시리즈 등). 바꾸기 전까지 이어지는 새 문서에 붙는다
    var lastSession: String {
        didSet { defaults.set(lastSession, forKey: Keys.lastSession) }
    }

    /// 마지막에 고른 종류. 책 한 권이나 같은 묶음을 찍는 동안 매번 고르지 않게 하고, "스캔 시작" 동작도 이 값을 쓴다
    var lastKind: DocumentKind {
        didSet { defaults.set(lastKind.rawValue, forKey: Keys.lastKind) }
    }

    /// 업데이트 확인(docs/ios.md "업데이트 알림"). 하루에 한 번만 GitHub에 묻는다
    var lastUpdateCheckAt: Date? {
        didSet { defaults.set(lastUpdateCheckAt, forKey: Keys.lastUpdateCheckAt) }
    }

    /// 마지막 확인에서 찾은 새 버전. 확인하지 않는 날에도 알림을 이어서 보여 준다
    var availableUpdate: AvailableUpdate? {
        didSet {
            if let availableUpdate, let data = try? JSONEncoder.leaf.encode(availableUpdate) {
                defaults.set(data, forKey: Keys.availableUpdate)
            } else {
                defaults.removeObject(forKey: Keys.availableUpdate)
            }
        }
    }

    /// 알림을 닫은 버전. 더 새 버전이 나오면 다시 알린다
    var dismissedUpdateVersion: String? {
        didSet { defaults.set(dismissedUpdateVersion, forKey: Keys.dismissedUpdateVersion) }
    }

    /// 보낸 것이 확인된 원본을 지우기까지의 일수. 0이면 지우지 않는다
    var retentionDays: Int {
        didSet { defaults.set(retentionDays, forKey: Keys.retentionDays) }
    }

    var sensitivity: AutoCaptureSensitivity {
        didSet { defaults.set(sensitivity.rawValue, forKey: Keys.sensitivity) }
    }

    var autoCaptureEnabled: Bool {
        didSet { defaults.set(autoCaptureEnabled, forKey: Keys.autoCapture) }
    }

    var pairedMac: PairedMac? {
        didSet {
            if let pairedMac, let data = try? JSONEncoder.leaf.encode(pairedMac) {
                defaults.set(data, forKey: Keys.pairedMac)
            } else {
                defaults.removeObject(forKey: Keys.pairedMac)
            }
        }
    }

    /// Mac이 이 iPhone을 알아보는 id. 앱을 지웠다 깔면 새로 생기고 다시 짝지어야 한다
    let deviceID: UUID

    var retentionPolicyDays: Int? {
        retentionDays > 0 ? retentionDays : nil
    }

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        lastSession = defaults.string(forKey: Keys.lastSession) ?? ""
        lastKind = defaults.string(forKey: Keys.lastKind).flatMap(DocumentKind.init(rawValue:)) ?? .document
        lastUpdateCheckAt = defaults.object(forKey: Keys.lastUpdateCheckAt) as? Date
        availableUpdate = defaults.data(forKey: Keys.availableUpdate).flatMap { try? JSONDecoder.leaf.decode(AvailableUpdate.self, from: $0) }
        dismissedUpdateVersion = defaults.string(forKey: Keys.dismissedUpdateVersion)
        retentionDays = defaults.object(forKey: Keys.retentionDays) as? Int ?? Self.defaultRetentionDays
        sensitivity = defaults.string(forKey: Keys.sensitivity).flatMap(AutoCaptureSensitivity.init(rawValue:)) ?? .normal
        autoCaptureEnabled = defaults.object(forKey: Keys.autoCapture) as? Bool ?? true
        pairedMac = defaults.data(forKey: Keys.pairedMac).flatMap { try? JSONDecoder.leaf.decode(PairedMac.self, from: $0) }
        if let text = defaults.string(forKey: Keys.deviceID), let id = UUID(uuidString: text) {
            deviceID = id
        } else {
            deviceID = UUID()
            defaults.set(deviceID.uuidString, forKey: Keys.deviceID)
        }
    }

    /// 종류마다 마지막에 고른 배치. 없으면 종류의 기본 배치(책은 펼친 두 페이지)
    func lastLayout(for kind: DocumentKind) -> PageLayout {
        let stored = defaults.string(forKey: Keys.lastLayout(kind)).flatMap(PageLayout.init(rawValue:))
        guard let stored, kind.layouts.contains(stored) else { return kind.defaultLayout }
        return stored
    }

    func setLastLayout(_ layout: PageLayout, for kind: DocumentKind) {
        guard kind.layouts.contains(layout) else { return }
        defaults.set(layout.rawValue, forKey: Keys.lastLayout(kind))
    }

    private enum Keys {
        static let lastSession = "leaf.lastSession"
        // 처음에 화면의 @AppStorage("lastDocumentKind")로 두었던 키를 그대로 써서 기억한 값을 잃지 않는다
        static let lastKind = "lastDocumentKind"
        static let lastUpdateCheckAt = "leaf.lastUpdateCheckAt"
        static let availableUpdate = "leaf.availableUpdate"
        static let dismissedUpdateVersion = "leaf.dismissedUpdateVersion"
        static func lastLayout(_ kind: DocumentKind) -> String { "leaf.lastLayout.\(kind.rawValue)" }
        static let retentionDays = "leaf.retentionDays"
        static let sensitivity = "leaf.sensitivity"
        static let autoCapture = "leaf.autoCapture"
        static let pairedMac = "leaf.pairedMac"
        static let deviceID = "leaf.deviceID"
    }
}

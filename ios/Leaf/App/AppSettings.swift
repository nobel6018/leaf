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

    /// 박람회 이름. 바꾸기 전까지 이어지는 새 문서에 붙는다
    var lastSession: String {
        didSet { defaults.set(lastSession, forKey: Keys.lastSession) }
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

    private enum Keys {
        static let lastSession = "leaf.lastSession"
        static let retentionDays = "leaf.retentionDays"
        static let sensitivity = "leaf.sensitivity"
        static let autoCapture = "leaf.autoCapture"
        static let pairedMac = "leaf.pairedMac"
        static let deviceID = "leaf.deviceID"
    }
}

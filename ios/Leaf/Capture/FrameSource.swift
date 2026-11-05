import AVFoundation
import Foundation

// 촬영 화면이 쓰는 프레임 공급자. 실기기는 CameraService, 카메라가 없는 시뮬레이터는 SyntheticFrameSource.
// @see docs/capture.md

struct CapturedPhoto: Sendable {
    /// HEIC. 방향은 EXIF에 들어 있다(픽셀은 센서 방향 그대로일 수 있다)
    var data: Data
    var capturedAt: Date
}

enum FrameSourceError: Error, LocalizedError {
    case cameraUnavailable
    case notAuthorized
    case captureFailed(String)

    var errorDescription: String? {
        switch self {
        case .cameraUnavailable: "카메라를 쓸 수 없습니다"
        case .notAuthorized: "카메라 권한이 필요합니다. 설정 > leaf에서 카메라를 켜 주세요"
        case let .captureFailed(reason): "촬영하지 못했습니다: \(reason)"
        }
    }
}

protocol FrameSource: AnyObject, Sendable {
    /// 분석 프레임을 1초에 약 `FrameThrottle.analysisFPS`번 넘긴다. 콜백은 분석 큐에서 불린다
    func start(onFrame: @escaping @Sendable (AnalysisFrame) -> Void) async throws
    func stop()
    func capturePhoto() async throws -> CapturedPhoto
}

/// 카메라는 초당 30장을 주지만 분석은 10번 정도로 줄여 발열을 낮춘다
struct FrameThrottle: Sendable {
    static let analysisFPS: Double = 10

    private var lastTime: TimeInterval = -.infinity

    mutating func shouldAnalyze(at time: TimeInterval) -> Bool {
        guard time - lastTime >= 1 / Self.analysisFPS * 0.95 else { return false }
        lastTime = time
        return true
    }
}

enum DevicePlatform {
    static let isSimulator: Bool = {
        #if targetEnvironment(simulator)
            true
        #else
            false
        #endif
    }()
}

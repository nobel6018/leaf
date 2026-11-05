import Foundation

// 거치대 자동 촬영의 판단. 카메라 없이 테스트하려고 측정값만 받는 순수 타입으로 둔다.
// 측정(문서 감지, 움직임, 손, 직전 촬영본과의 거리)은 FrameAnalyzer가 만든다.
// @see docs/capture.md

/// 기준값. 실제 팜플렛과 책으로 맞춘다. 민감도 설정은 이 값 묶음을 바꾼다
struct AutoCaptureTuning: Equatable, Sendable {
    /// 문서가 보이고 화면이 멈춘 상태가 이만큼 이어지면 찍는다
    var stableDuration: TimeInterval
    /// 연속 분석 프레임(축소 흑백)의 평균 밝기 차이(0~1). 이보다 작으면 멈춘 것으로 본다
    var stillMotion: Double
    /// 촬영 뒤 이보다 크게 움직이면 종이를 바꾸는 중으로 본다
    var changeMotion: Double
    /// 화면 대비 문서 넓이(0~1). 이보다 작으면 문서로 보지 않는다(책상 위 작은 종이, 오감지)
    var minimumDocumentArea: Double
    /// 직전 촬영본과 feature print 거리가 이보다 가까우면 같은 장으로 보고 건너뛴다
    var duplicateDistance: Float
    /// 촬영 뒤 직전 촬영본과 이만큼 멀어지면 다른 장이 놓인 것으로 본다
    var changeDistance: Float

    static let normal = AutoCaptureTuning(
        stableDuration: 0.6,
        stillMotion: 0.018,
        changeMotion: 0.06,
        minimumDocumentArea: 0.12,
        duplicateDistance: 0.35,
        changeDistance: 0.55
    )

    /// 둔하게: 더 오래 멈춰야 찍는다. 흔들리는 거치대나 조명이 깜빡이는 곳
    static let low = AutoCaptureTuning(
        stableDuration: 1.0,
        stillMotion: 0.012,
        changeMotion: 0.08,
        minimumDocumentArea: 0.18,
        duplicateDistance: 0.4,
        changeDistance: 0.6
    )

    /// 민감하게: 빨리 찍는다. 단단한 거치대에서 빠르게 넘길 때
    static let high = AutoCaptureTuning(
        stableDuration: 0.4,
        stillMotion: 0.025,
        changeMotion: 0.05,
        minimumDocumentArea: 0.08,
        duplicateDistance: 0.3,
        changeDistance: 0.5
    )
}

enum AutoCaptureSensitivity: String, CaseIterable, Codable, Sendable {
    case low
    case normal
    case high

    var tuning: AutoCaptureTuning {
        switch self {
        case .low: .low
        case .normal: .normal
        case .high: .high
        }
    }

    var label: String {
        switch self {
        case .low: "둔하게"
        case .normal: "보통"
        case .high: "민감하게"
        }
    }
}

/// 분석 프레임 하나에서 잰 값
struct FrameMeasurement: Equatable, Sendable {
    /// 단조 증가하는 초. 테스트는 0부터 준다
    var time: TimeInterval
    /// 감지한 문서 테두리. 없으면 nil
    var quad: Quad?
    /// 직전 분석 프레임과의 차이(0~1)
    var motion: Double
    var handVisible: Bool
    /// 직전 촬영본과의 feature print 거리. 직전 촬영본이 없거나 아직 재지 않았으면 nil
    var distanceFromLastCapture: Float?
}

enum AutoCapturePhase: Equatable, Sendable {
    /// 문서가 안정되기를 기다린다. `steadySince`는 안정이 시작된 시각
    case waiting(steadySince: TimeInterval?)
    /// 셔터를 눌렀고 사진이 오기를 기다린다
    case capturing
    /// 방금 찍은 장이 그대로 있다. 종이가 바뀌기를 기다린다
    case waitingForChange
}

enum AutoCaptureDecision: Equatable, Sendable {
    case none
    case capture
    /// 안정됐지만 직전 촬영본과 같은 장이라 찍지 않았다
    case skippedDuplicate
}

/// 화면 안내 문구용 상태
enum AutoCaptureStatus: Equatable, Sendable {
    case noDocument
    case handVisible
    case moving
    case steadying(progress: Double)
    case capturing
    case waitingForChange
}

struct AutoCaptureEngine: Sendable {
    var tuning: AutoCaptureTuning
    private(set) var phase: AutoCapturePhase = .waiting(steadySince: nil)
    private(set) var status: AutoCaptureStatus = .noDocument
    /// 직전 촬영본이 있는가. 있으면 찍기 전에 거리를 재야 한다
    private(set) var hasLastCapture = false

    init(tuning: AutoCaptureTuning = .normal) {
        self.tuning = tuning
    }

    /// feature print 계산은 비싸다(프레임당 수십 ms). 쓸 때만 재게 분석기에 알려 준다
    var wantsDistance: Bool {
        guard hasLastCapture else { return false }
        switch phase {
        case .waitingForChange: return true
        case let .waiting(steadySince): return steadySince != nil
        case .capturing: return false
        }
    }

    mutating func process(_ frame: FrameMeasurement) -> AutoCaptureDecision {
        switch phase {
        case .capturing:
            status = .capturing
            return .none

        case .waitingForChange:
            status = .waitingForChange
            if sceneChanged(frame) {
                phase = .waiting(steadySince: nil)
                status = blockingStatus(frame) ?? .steadying(progress: 0)
            }
            return .none

        case let .waiting(steadySince):
            if let blocking = blockingStatus(frame) {
                phase = .waiting(steadySince: nil)
                status = blocking
                return .none
            }
            let since = steadySince ?? frame.time
            phase = .waiting(steadySince: since)
            let elapsed = frame.time - since
            status = .steadying(progress: min(1, elapsed / tuning.stableDuration))
            // 프레임 시각은 0.1초 간격이라도 부동소수 오차가 있다. 1ms 안쪽은 다 찬 것으로 본다
            guard elapsed + 0.001 >= tuning.stableDuration else { return .none }

            if hasLastCapture {
                // 거리를 아직 못 쟀으면 찍지 않고 다음 프레임을 기다린다. 같은 장을 다시 찍는 것보다 늦게 찍는 편이 낫다
                guard let distance = frame.distanceFromLastCapture else { return .none }
                if distance < tuning.duplicateDistance {
                    phase = .waitingForChange
                    status = .waitingForChange
                    return .skippedDuplicate
                }
            }
            phase = .capturing
            status = .capturing
            return .capture
        }
    }

    /// 자동이든 수동이든 사진이 저장되면 부른다. 실패하면 다시 기다린다
    mutating func captureFinished(succeeded: Bool) {
        if succeeded {
            hasLastCapture = true
            phase = .waitingForChange
            status = .waitingForChange
        } else {
            phase = .waiting(steadySince: nil)
            status = .noDocument
        }
    }

    /// 수동 셔터를 누른 순간. 자동 판단이 같은 장을 겹쳐 찍지 않게 막는다
    mutating func manualCaptureStarted() {
        phase = .capturing
        status = .capturing
    }

    /// 새 문서를 시작했거나 마지막 장을 지웠다. 같은 장을 다시 찍을 수 있어야 한다
    mutating func forgetLastCapture() {
        hasLastCapture = false
        phase = .waiting(steadySince: nil)
    }

    private func blockingStatus(_ frame: FrameMeasurement) -> AutoCaptureStatus? {
        guard let quad = frame.quad, quad.area >= tuning.minimumDocumentArea else { return .noDocument }
        if frame.handVisible { return .handVisible }
        if frame.motion > tuning.stillMotion { return .moving }
        return nil
    }

    private func sceneChanged(_ frame: FrameMeasurement) -> Bool {
        if frame.quad == nil || frame.handVisible { return true }
        if frame.motion >= tuning.changeMotion { return true }
        if let distance = frame.distanceFromLastCapture, distance >= tuning.changeDistance { return true }
        return false
    }
}

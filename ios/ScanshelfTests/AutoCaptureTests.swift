import Foundation
import Testing
@testable import Scanshelf

struct AutoCaptureTests {
    private let page = Quad(
        topLeft: .init(x: 0.1, y: 0.1),
        topRight: .init(x: 0.9, y: 0.1),
        bottomRight: .init(x: 0.9, y: 0.9),
        bottomLeft: .init(x: 0.1, y: 0.9)
    )

    private func frame(
        _ time: TimeInterval,
        quad: Quad?? = .none,
        motion: Double = 0,
        hand: Bool = false,
        distance: Float? = nil
    ) -> FrameMeasurement {
        FrameMeasurement(
            time: time,
            quad: quad ?? page,
            motion: motion,
            handVisible: hand,
            distanceFromLastCapture: distance
        )
    }

    /// 0.1초 간격(분석 10fps)으로 프레임을 넣고 결정을 모은다
    private func run(
        _ engine: inout AutoCaptureEngine,
        from start: TimeInterval,
        to end: TimeInterval,
        make: (TimeInterval) -> FrameMeasurement
    ) -> [AutoCaptureDecision] {
        var decisions: [AutoCaptureDecision] = []
        var time = start
        while time <= end + 0.0001 {
            decisions.append(engine.process(make(time)))
            time += 0.1
        }
        return decisions
    }

    @Test func 문서가_안정되면_정해진_시간_뒤에_찍는다() {
        var engine = AutoCaptureEngine(tuning: .normal)

        let early = run(&engine, from: 0, to: 0.5) { frame($0) }
        let atThreshold = engine.process(frame(0.6))

        #expect(!early.contains(.capture))
        #expect(atThreshold == .capture)
        #expect(engine.phase == .capturing)
    }

    @Test func 문서가_없으면_찍지_않는다() {
        var engine = AutoCaptureEngine()

        let decisions = run(&engine, from: 0, to: 2) { frame($0, quad: .some(nil)) }

        #expect(!decisions.contains(.capture))
        #expect(engine.status == .noDocument)
    }

    @Test func 손이_보이면_안정돼도_찍지_않는다() {
        var engine = AutoCaptureEngine()

        let decisions = run(&engine, from: 0, to: 2) { frame($0, hand: true) }

        #expect(!decisions.contains(.capture))
        #expect(engine.status == .handVisible)
    }

    @Test func 움직이면_안정_시간을_처음부터_다시_잰다() {
        var engine = AutoCaptureEngine()
        _ = run(&engine, from: 0, to: 0.5) { frame($0) }

        _ = engine.process(frame(0.6, motion: 0.2))
        let afterMove = run(&engine, from: 0.7, to: 1.2) { frame($0) }
        let later = engine.process(frame(1.3))

        #expect(!afterMove.contains(.capture))
        #expect(later == .capture)
    }

    @Test func 너무_작은_테두리는_문서로_보지_않는다() {
        var engine = AutoCaptureEngine()
        let tiny = Quad(
            topLeft: .init(x: 0.45, y: 0.45),
            topRight: .init(x: 0.55, y: 0.45),
            bottomRight: .init(x: 0.55, y: 0.55),
            bottomLeft: .init(x: 0.45, y: 0.55)
        )

        let decisions = run(&engine, from: 0, to: 2) { frame($0, quad: .some(tiny)) }

        #expect(!decisions.contains(.capture))
    }

    @Test func 찍은_뒤에는_종이가_바뀔_때까지_다시_찍지_않는다() {
        var engine = AutoCaptureEngine()
        _ = run(&engine, from: 0, to: 0.6) { frame($0) }
        engine.captureFinished(succeeded: true)

        let decisions = run(&engine, from: 0.7, to: 3) { frame($0, distance: 0.1) }

        #expect(!decisions.contains(.capture))
        #expect(engine.phase == .waitingForChange)
        #expect(engine.wantsDistance)
    }

    @Test func 손으로_종이를_바꾸면_다음_장을_찍는다() {
        var engine = AutoCaptureEngine()
        _ = run(&engine, from: 0, to: 0.6) { frame($0) }
        engine.captureFinished(succeeded: true)

        _ = run(&engine, from: 0.7, to: 1.0) { frame($0, hand: true, distance: 0.3) }
        let next = run(&engine, from: 1.1, to: 1.8) { frame($0, distance: 0.8) }

        #expect(next.contains(.capture))
    }

    @Test func 종이를_바꿨다가_같은_장을_다시_놓으면_건너뛴다() {
        var engine = AutoCaptureEngine()
        _ = run(&engine, from: 0, to: 0.6) { frame($0) }
        engine.captureFinished(succeeded: true)

        _ = engine.process(frame(0.7, hand: true))
        let decisions = run(&engine, from: 0.8, to: 1.5) { frame($0, distance: 0.1) }

        #expect(decisions.contains(.skippedDuplicate))
        #expect(!decisions.contains(.capture))
        #expect(engine.phase == .waitingForChange)
    }

    @Test func 거리를_아직_못_쟀으면_기다린다() {
        var engine = AutoCaptureEngine()
        _ = run(&engine, from: 0, to: 0.6) { frame($0) }
        engine.captureFinished(succeeded: true)
        _ = engine.process(frame(0.7, quad: .some(nil)))

        let withoutDistance = run(&engine, from: 0.8, to: 2) { frame($0) }
        let withDistance = engine.process(frame(2.1, distance: 0.9))

        #expect(!withoutDistance.contains(.capture))
        #expect(withDistance == .capture)
    }

    @Test func 촬영이_실패하면_다시_기다린다() {
        var engine = AutoCaptureEngine()
        _ = run(&engine, from: 0, to: 0.6) { frame($0) }

        engine.captureFinished(succeeded: false)
        let retry = run(&engine, from: 0.7, to: 1.3) { frame($0) }

        #expect(retry.contains(.capture))
        #expect(!engine.hasLastCapture)
    }

    @Test func 마지막_장을_지우면_같은_장을_다시_찍을_수_있다() {
        var engine = AutoCaptureEngine()
        _ = run(&engine, from: 0, to: 0.6) { frame($0) }
        engine.captureFinished(succeeded: true)

        engine.forgetLastCapture()
        let decisions = run(&engine, from: 0.7, to: 1.3) { frame($0, distance: 0.05) }

        #expect(decisions.contains(.capture))
    }

    @Test func 수동_셔터_중에는_자동으로_찍지_않는다() {
        var engine = AutoCaptureEngine()

        engine.manualCaptureStarted()
        let decisions = run(&engine, from: 0, to: 2) { frame($0) }

        #expect(!decisions.contains(.capture))
    }

    @Test func 민감하게는_보통보다_빨리_찍는다() {
        var sensitive = AutoCaptureEngine(tuning: AutoCaptureSensitivity.high.tuning)
        var normal = AutoCaptureEngine(tuning: AutoCaptureSensitivity.normal.tuning)

        let fast = run(&sensitive, from: 0, to: 0.4) { frame($0) }
        let slow = run(&normal, from: 0, to: 0.4) { frame($0) }

        #expect(fast.contains(.capture))
        #expect(!slow.contains(.capture))
    }

    /// 펼친 3단 접지(A4 가로, 1.414:1)가 세로 4:3 화면에서 차지하는 테두리. 가로 폭 비율만 주면 세로는 비율로 정해진다
    private func unfoldedSheet(widthFraction width: Double, ratio: Double = 297.0 / 210.0) -> Quad {
        // 화면은 가로 3, 세로 4. 정규화 세로 = 폭 * 3 / 비율 / 4
        let height = width * 3 / ratio / 4
        let left = (1 - width) / 2
        let top = (1 - height) / 2
        return Quad(
            topLeft: .init(x: left, y: top),
            topRight: .init(x: left + width, y: top),
            bottomRight: .init(x: left + width, y: top + height),
            bottomLeft: .init(x: left, y: top + height)
        )
    }

    @Test func 펼친_3단_접지는_어느_민감도에서도_문서로_보고_찍는다() {
        // 화면 폭의 80%를 채운 가로 종이. 넓이는 약 0.34로 가장 높은 하한(둔하게 0.18)보다 크다
        let sheet = unfoldedSheet(widthFraction: 0.8)
        #expect(sheet.area > AutoCaptureTuning.low.minimumDocumentArea)

        for sensitivity in AutoCaptureSensitivity.allCases {
            var engine = AutoCaptureEngine(tuning: sensitivity.tuning)
            let decisions = run(&engine, from: 0, to: 1.2) { frame($0, quad: .some(sheet)) }
            #expect(decisions.contains(.capture), "민감도 \(sensitivity.rawValue)")
        }
    }

    @Test func 거치대를_낮춰_폭의_60퍼센트만_채워도_찍는다() {
        // 세로 화면에 가로 종이를 놓으면 위아래가 비어 넓이가 줄어든다. 폭 60%면 넓이 약 0.19
        let sheet = unfoldedSheet(widthFraction: 0.6)
        var engine = AutoCaptureEngine(tuning: .low)

        let decisions = run(&engine, from: 0, to: 1.2) { frame($0, quad: .some(sheet)) }

        #expect(decisions.contains(.capture))
    }
}

import Foundation
import UIKit

// 촬영 화면 하나의 상태. 프레임 분석 결과를 자동 촬영 판단에 넣고, 찍은 사진을 문서에 더한다.
// @see docs/capture.md

@MainActor
@Observable
final class CaptureModel {
    /// 자동 촬영 뒤 "찍었다"는 표시를 보여 주는 시간
    static let flashDuration: TimeInterval = 0.25

    let source: any FrameSource
    private(set) var document: Manifest
    /// 미리보기에 그릴 테두리(표시 방향, 0~1)
    private(set) var quad: Quad?
    private(set) var status: AutoCaptureStatus = .noDocument
    private(set) var isSaving = false
    private(set) var usingHint = false
    /// 늘어날 때마다 화면이 번쩍인다
    private(set) var flashCount = 0
    private(set) var message: String?
    private(set) var cameraError: String?
    /// 다음 촬영으로 바꿀 페이지
    var retakeTarget: UUID?
    /// 다음 촬영의 배치. 바꾸면 이 종류의 다음 새 문서도 이 배치로 시작한다
    var layout: PageLayout {
        didSet { app.settings.setLastLayout(layout, for: document.kind) }
    }
    var autoEnabled: Bool {
        didSet {
            app.settings.autoCaptureEnabled = autoEnabled
            if autoEnabled { engine.forgetLastCapture() }
        }
    }

    private let app: AppModel
    private let analyzer = FrameAnalyzer()
    private let wantsDistance = LockedFlag()
    private var engine: AutoCaptureEngine
    private var running = false
    private var messageTask: Task<Void, Never>?

    init(document: Manifest, layout: PageLayout? = nil, app: AppModel) {
        self.document = document
        self.app = app
        source = DevicePlatform.isSimulator ? SyntheticFrameSource() : CameraService()
        engine = AutoCaptureEngine(tuning: app.settings.sensitivity.tuning)
        autoEnabled = app.settings.autoCaptureEnabled
        // 문서에 이미 찍은 장이 있으면 마지막 장의 배치를 이어 간다. 없으면 이 종류에서 마지막에 고른 배치
        self.layout = layout ?? document.pages.last?.layout ?? app.settings.lastLayout(for: document.kind)
    }

    /// 배치 선택지. 종류를 바꾼 문서의 마지막 장 배치가 목록에 없으면 그것도 보여 준다
    var layoutChoices: [PageLayout] {
        let choices = document.kind.layouts
        return choices.contains(layout) ? choices : choices + [layout]
    }

    var cameraSession: CameraService? { source as? CameraService }

    func rawURL(_ page: Page) -> URL {
        app.store.fileURL(of: document.id, relativePath: page.raw)
    }
    var syntheticSource: SyntheticFrameSource? { source as? SyntheticFrameSource }

    func start() async {
        guard !running else { return }
        running = true
        engine.tuning = app.settings.sensitivity.tuning
        let analyzer = analyzer
        let wantsDistance = wantsDistance
        do {
            try await source.start { [weak self] frame in
                let analysis = analyzer.analyze(frame, wantsDistance: wantsDistance.value)
                Task { @MainActor in self?.handle(analysis) }
            }
            cameraError = nil
        } catch {
            cameraError = error.localizedDescription
            running = false
        }
    }

    func stop() {
        source.stop()
        running = false
    }

    /// 화면 셔터, 볼륨 버튼, 블루투스 리모컨
    func shutter() {
        guard !isSaving else { return }
        engine.manualCaptureStarted()
        Task { await capture() }
    }

    func delete(_ pageID: UUID) {
        do {
            let isLast = document.pages.last?.id == pageID
            document = try app.deletePage(pageID, from: document)
            if retakeTarget == pageID { retakeTarget = nil }
            // 마지막 장을 지웠으면 같은 장을 다시 놓고 찍을 수 있어야 한다
            if isLast {
                engine.forgetLastCapture()
                analyzer.forgetCapture()
            }
        } catch {
            show("지우지 못했습니다: \(error.localizedDescription)")
        }
    }

    func retake(_ pageID: UUID) {
        retakeTarget = pageID
        engine.forgetLastCapture()
        analyzer.forgetCapture()
        show("다시 찍을 장을 놓아 주세요")
    }

    private func handle(_ analysis: FrameAnalysis) {
        guard running else { return }
        quad = analysis.measurement.quad
        usingHint = analysis.usedHint
        guard autoEnabled else {
            status = engine.status
            return
        }
        let decision = engine.process(analysis.measurement)
        status = engine.status
        wantsDistance.value = engine.wantsDistance
        switch decision {
        case .capture:
            Task { await capture() }
        case .skippedDuplicate:
            show("방금 찍은 장과 같아 건너뜁니다")
        case .none:
            break
        }
    }

    private func capture() async {
        guard !isSaving else { return }
        isSaving = true
        defer { isSaving = false }
        let previewQuad = quad
        let trustPreview = usingHint
        do {
            let photo = try await source.capturePhoto()
            // 분석 프레임의 프린트를 기준으로 둔다. 다음 판단도 분석 프레임끼리 비교한다
            let analyzer = analyzer
            await Task.detached { analyzer.rememberCapture() }.value

            let store = app.store
            let current = document
            let layout = layout
            let target = retakeTarget.flatMap { id in current.pages.contains { $0.id == id } ? id : nil }
            let updated = try await Task.detached(priority: .userInitiated) {
                // 사진에서 다시 찾은 테두리가 더 정확하다. 못 찾으면 미리보기 테두리를 쓴다(같은 화각, 같은 방향)
                // 시뮬레이터 합성 화면은 미리보기 테두리가 정답이다
                let quad = trustPreview ? previewQuad : FrameAnalyzer.detectQuad(inPhoto: photo.data) ?? previewQuad
                return try store.addPage(
                    to: current,
                    photo: photo.data,
                    quad: quad,
                    layout: layout,
                    capturedAt: photo.capturedAt,
                    replacing: target
                )
            }.value
            document = updated
            app.replace(updated)
            retakeTarget = nil
            engine.captureFinished(succeeded: true)
            flashCount += 1
            UINotificationFeedbackGenerator().notificationOccurred(.success)
        } catch {
            engine.captureFinished(succeeded: false)
            UINotificationFeedbackGenerator().notificationOccurred(.error)
            show(error.localizedDescription)
        }
        status = engine.status
    }

    private func show(_ text: String) {
        message = text
        messageTask?.cancel()
        messageTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(2.5))
            guard !Task.isCancelled else { return }
            self?.message = nil
        }
    }
}

/// 분석 큐가 읽고 메인이 쓰는 값 하나
final class LockedFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var stored = false

    var value: Bool {
        get { lock.withLock { stored } }
        set { lock.withLock { stored = newValue } }
    }
}

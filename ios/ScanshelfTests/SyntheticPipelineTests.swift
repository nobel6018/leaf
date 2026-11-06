import Foundation
import Testing
@testable import Scanshelf

// 시뮬레이터 합성 소스로 분석과 촬영 경로를 끝까지 돌려 본다. 실기기 카메라와 Vision 경로는 확인하지 못한다
struct SyntheticPipelineTests {
    private func frame(at time: TimeInterval) throws -> AnalysisFrame {
        let scene = SyntheticFrameSource.scene(at: time)
        let image = try #require(SyntheticFrameSource.render(scene, size: SyntheticFrameSource.frameSize))
        return AnalysisFrame(
            image: .cgImage(image),
            orientation: .up,
            time: time,
            hint: SyntheticHint(quad: scene.paperVisible ? scene.quad : nil, handVisible: scene.handVisible, pageNumber: scene.pageNumber)
        )
    }

    @Test func 합성_장면은_종이를_치우고_다음_장을_놓는다() {
        let first = SyntheticFrameSource.scene(at: 1)
        let removing = SyntheticFrameSource.scene(at: SyntheticFrameSource.stillDuration + 0.7)
        let next = SyntheticFrameSource.scene(at: SyntheticFrameSource.stillDuration + SyntheticFrameSource.changeDuration + 1)

        #expect(first.pageNumber == 1 && first.paperVisible && !first.handVisible)
        #expect(!removing.paperVisible && removing.handVisible)
        #expect(next.pageNumber == 2 && next.paperVisible && !next.handVisible)
    }

    @Test func 같은_장은_거리_0_다음_장은_거리_1로_잰다() throws {
        let analyzer = FrameAnalyzer()
        _ = analyzer.analyze(try frame(at: 1), wantsDistance: false)
        analyzer.rememberCapture()

        let same = analyzer.analyze(try frame(at: 2), wantsDistance: true)
        let next = analyzer.analyze(try frame(at: 6), wantsDistance: true)

        #expect(same.measurement.distanceFromLastCapture == 0)
        #expect(next.measurement.distanceFromLastCapture == 1)
        #expect(same.measurement.quad == SyntheticFrameSource.scene(at: 2).quad)
        #expect(same.usedHint)
    }

    @Test func 합성_사진은_HEIC로_저장된다() async throws {
        let source = SyntheticFrameSource()

        let photo = try await source.capturePhoto()

        // HEIC(ISO BMFF)는 4바이트 뒤에 ftyp가 온다
        #expect(photo.data.count > 1000)
        #expect(String(data: photo.data.subdata(in: 4 ..< 8), encoding: .ascii) == "ftyp")
    }

    @Test func 합성_장면을_자동_촬영에_넣으면_장마다_한_번씩_찍는다() throws {
        let analyzer = FrameAnalyzer()
        var engine = AutoCaptureEngine(tuning: .normal)
        var captures: [Int] = []
        var time = 0.0
        // 세 장이 지나가는 동안 0.1초 간격으로 판단한다. 촬영은 즉시 성공한 것으로 본다
        let end = 3 * (SyntheticFrameSource.stillDuration + SyntheticFrameSource.changeDuration) - 0.2
        while time < end {
            let analysis = analyzer.analyze(try frame(at: time), wantsDistance: engine.wantsDistance)
            if engine.process(analysis.measurement) == .capture {
                analyzer.rememberCapture()
                engine.captureFinished(succeeded: true)
                captures.append(SyntheticFrameSource.scene(at: time).pageNumber)
            }
            time += 0.1
        }

        #expect(captures == [1, 2, 3])
    }
}

import CoreGraphics
import Foundation
import ImageIO
import UIKit
import UniformTypeIdentifiers

// 카메라가 없는 시뮬레이터용 프레임 공급자. 어두운 책상 위 흰 종이를 그리고, 몇 초마다 손이 들어와
// 다음 장으로 바꾸는 장면을 되풀이한다. 모든 그림은 코드로 만들므로 저작권 자료가 들어가지 않는다.
// 실기기에서는 쓰지 않는다(CaptureModel이 시뮬레이터일 때만 고른다).
// @see docs/ios.md

final class SyntheticFrameSource: FrameSource, @unchecked Sendable {
    /// 한 장이 놓여 있는 시간과, 손이 들어와 종이를 바꾸는 시간
    static let stillDuration: TimeInterval = 3.0
    static let changeDuration: TimeInterval = 1.5
    static let frameSize = CGSize(width: 720, height: 960)
    static let photoSize = CGSize(width: 2268, height: 3024)

    private let queue = DispatchQueue(label: "leaf.synthetic.frames")
    private var timer: DispatchSourceTimer?
    private var startedAt = Date.now
    /// 미리보기용 최근 프레임. 메인 스레드에서 읽는다
    private let latest = LatestImage()

    var latestImage: CGImage? { latest.value }

    func start(onFrame: @escaping @Sendable (AnalysisFrame) -> Void) async throws {
        queue.sync {
            guard timer == nil else { return }
            startedAt = .now
            let timer = DispatchSource.makeTimerSource(queue: queue)
            timer.schedule(deadline: .now(), repeating: 1 / FrameThrottle.analysisFPS)
            timer.setEventHandler { [weak self] in
                guard let self else { return }
                let time = Date.now.timeIntervalSince(self.startedAt)
                let scene = Self.scene(at: time)
                guard let image = Self.render(scene, size: Self.frameSize) else { return }
                self.latest.value = image
                onFrame(AnalysisFrame(
                    image: .cgImage(image),
                    orientation: .up,
                    time: time,
                    hint: SyntheticHint(
                        quad: scene.paperVisible ? scene.quad : nil,
                        handVisible: scene.handVisible,
                        pageNumber: scene.pageNumber
                    )
                ))
            }
            timer.resume()
            self.timer = timer
        }
    }

    func stop() {
        queue.sync {
            timer?.cancel()
            timer = nil
        }
    }

    func capturePhoto() async throws -> CapturedPhoto {
        let time = queue.sync { Date.now.timeIntervalSince(startedAt) }
        let scene = Self.scene(at: time)
        guard let image = Self.render(scene, size: Self.photoSize) else {
            throw FrameSourceError.captureFailed("합성 이미지를 만들지 못했습니다")
        }
        return CapturedPhoto(data: try Self.heic(image), capturedAt: .now)
    }

    struct Scene: Equatable {
        var pageNumber: Int
        var quad: Quad
        var paperVisible: Bool
        var handVisible: Bool
        /// 손 위치 0~1. 손이 들어왔다 나간다
        var handProgress: Double
    }

    static func scene(at time: TimeInterval) -> Scene {
        let cycle = stillDuration + changeDuration
        let index = Int(time / cycle)
        let phase = time - Double(index) * cycle
        // 장마다 조금씩 다르게 놓는다(실제로 손으로 놓으면 매번 어긋난다)
        let jitter = Double((index * 37) % 7 - 3) * 0.006
        let base = Quad(
            topLeft: .init(x: 0.14 + jitter, y: 0.12),
            topRight: .init(x: 0.86 + jitter, y: 0.11 - jitter),
            bottomRight: .init(x: 0.92, y: 0.88 + jitter),
            bottomLeft: .init(x: 0.08 - jitter, y: 0.89)
        )
        guard phase >= stillDuration else {
            return Scene(pageNumber: index + 1, quad: base, paperVisible: true, handVisible: false, handProgress: 0)
        }
        let progress = (phase - stillDuration) / changeDuration
        // 전반부는 손이 종이를 걷어 내고, 후반부는 다음 장을 내려놓는다
        let paperVisible = progress < 0.35 || progress > 0.6
        let page = progress > 0.6 ? index + 2 : index + 1
        return Scene(pageNumber: page, quad: base, paperVisible: paperVisible, handVisible: progress < 0.9, handProgress: progress)
    }

    static func render(_ scene: Scene, size: CGSize) -> CGImage? {
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        format.opaque = true
        let renderer = UIGraphicsImageRenderer(size: size, format: format)
        let image = renderer.image { context in
            let cg = context.cgContext
            UIColor(red: 0.22, green: 0.18, blue: 0.15, alpha: 1).setFill()
            cg.fill(CGRect(origin: .zero, size: size))

            if scene.paperVisible {
                drawPaper(scene, size: size, in: cg)
            }
            if scene.handVisible {
                drawHand(progress: scene.handProgress, size: size, in: cg)
            }
        }
        return image.cgImage
    }

    private static func drawPaper(_ scene: Scene, size: CGSize, in cg: CGContext) {
        let corners = scene.quad.cgPoints(in: size)
        let path = UIBezierPath()
        path.move(to: corners[0])
        corners.dropFirst().forEach(path.addLine(to:))
        path.close()
        cg.setShadow(offset: CGSize(width: 0, height: size.width * 0.01), blur: size.width * 0.02)
        UIColor(white: 0.97, alpha: 1).setFill()
        path.fill()
        cg.setShadow(offset: .zero, blur: 0)

        let left = max(corners[0].x, corners[3].x) + size.width * 0.05
        let right = min(corners[1].x, corners[2].x) - size.width * 0.05
        let top = max(corners[0].y, corners[1].y) + size.height * 0.04
        let width = right - left

        let title = "leaf 합성 페이지 \(scene.pageNumber)" as NSString
        title.draw(at: CGPoint(x: left, y: top), withAttributes: [
            .font: UIFont.boldSystemFont(ofSize: size.width * 0.055),
            .foregroundColor: UIColor(red: 0.1, green: 0.35, blue: 0.2, alpha: 1),
        ])

        // 본문 줄. 장마다 길이를 달리해 장끼리 구별되게 한다
        UIColor(white: 0.55, alpha: 1).setFill()
        let lineHeight = size.height * 0.018
        var y = top + size.height * 0.1
        var line = 0
        let bottom = min(corners[2].y, corners[3].y) - size.height * 0.05
        while y < bottom {
            let seed = (scene.pageNumber * 131 + line * 17) % 40
            let fraction = line % 6 == 5 ? 0.45 : 0.7 + Double(seed) / 140
            cg.fill(CGRect(x: left, y: y, width: width * fraction, height: lineHeight))
            y += lineHeight * 2.2
            line += 1
        }
    }

    private static func drawHand(progress: Double, size: CGSize, in cg: CGContext) {
        // 오른쪽 아래에서 들어왔다 나가는 단순한 손 모양
        let reach = sin(progress * .pi)
        let center = CGPoint(x: size.width * (1.05 - 0.4 * reach), y: size.height * (1.0 - 0.35 * reach))
        UIColor(red: 0.86, green: 0.67, blue: 0.55, alpha: 1).setFill()
        let palm = CGRect(x: center.x - size.width * 0.14, y: center.y - size.width * 0.1, width: size.width * 0.3, height: size.width * 0.34)
        UIBezierPath(roundedRect: palm, cornerRadius: size.width * 0.08).fill()
        for finger in 0 ..< 4 {
            let x = palm.minX + CGFloat(finger) * palm.width * 0.24 + palm.width * 0.04
            let rect = CGRect(x: x, y: palm.minY - size.width * 0.16, width: palm.width * 0.18, height: size.width * 0.2)
            UIBezierPath(roundedRect: rect, cornerRadius: palm.width * 0.09).fill()
        }
    }

    static func heic(_ image: CGImage) throws -> Data {
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(data, UTType.heic.identifier as CFString, 1, nil) else {
            throw FrameSourceError.captureFailed("이 환경은 HEIC 인코딩을 지원하지 않습니다")
        }
        CGImageDestinationAddImage(destination, image, [kCGImageDestinationLossyCompressionQuality: 0.8] as CFDictionary)
        guard CGImageDestinationFinalize(destination) else {
            throw FrameSourceError.captureFailed("HEIC 인코딩 실패")
        }
        return data as Data
    }
}

private final class LatestImage: @unchecked Sendable {
    private let lock = NSLock()
    private var image: CGImage?

    var value: CGImage? {
        get { lock.withLock { image } }
        set { lock.withLock { image = newValue } }
    }
}

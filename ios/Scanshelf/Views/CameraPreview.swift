import AVFoundation
import AVKit
import SwiftUI

// 카메라 미리보기와 하드웨어 셔터. 볼륨 버튼과 블루투스 카메라 리모컨(볼륨 키 신호를 보낸다)은
// AVCaptureEventInteraction으로 받는다. 이 상호작용이 붙어 있는 동안 볼륨 버튼은 소리를 바꾸지 않는다.
// 캡처 세션이 돌고 있어야 동작하므로 시뮬레이터 합성 소스에서는 화면 셔터만 쓴다.
// @see docs/capture.md

struct CameraPreview: UIViewRepresentable {
    let session: AVCaptureSession
    let onShutter: @MainActor () -> Void

    func makeUIView(context: Context) -> PreviewView {
        let view = PreviewView()
        view.previewLayer.session = session
        // 미리보기는 4:3 상자에 맞춰 잘라내지 않는다. 테두리 좌표를 상자 크기에 그대로 곱해 그리면 된다
        view.previewLayer.videoGravity = .resizeAspect
        if let connection = view.previewLayer.connection, connection.isVideoRotationAngleSupported(CameraService.portraitRotationAngle) {
            connection.videoRotationAngle = CameraService.portraitRotationAngle
        }
        let interaction = AVCaptureEventInteraction { event in
            // 누르는 순간이 아니라 뗄 때 찍는다. 길게 누르면 한 번만 찍힌다
            if event.phase == .ended {
                MainActor.assumeIsolated { onShutter() }
            }
        }
        view.addInteraction(interaction)
        return view
    }

    func updateUIView(_ view: PreviewView, context: Context) {
        if let connection = view.previewLayer.connection, connection.videoRotationAngle != CameraService.portraitRotationAngle,
           connection.isVideoRotationAngleSupported(CameraService.portraitRotationAngle) {
            connection.videoRotationAngle = CameraService.portraitRotationAngle
        }
    }

    final class PreviewView: UIView {
        override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }

        var previewLayer: AVCaptureVideoPreviewLayer {
            // layerClass로 정했으므로 항상 이 타입이다
            layer as! AVCaptureVideoPreviewLayer
        }
    }
}

/// 시뮬레이터 합성 소스의 최근 프레임을 보여 준다
struct SyntheticPreview: View {
    let source: SyntheticFrameSource

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1 / FrameThrottle.analysisFPS)) { _ in
            if let image = source.latestImage {
                Image(decorative: image, scale: 1)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
            } else {
                Color.black
            }
        }
    }
}

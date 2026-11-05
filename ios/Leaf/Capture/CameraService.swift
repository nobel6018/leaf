import AVFoundation
import Foundation
import ImageIO

// AVCaptureSession 하나로 미리보기, 분석 프레임, 최대 해상도 HEIC 사진을 같이 낸다.
// 세션 프리셋을 .photo로 두어 분석 프레임과 사진의 화각, 비율(4:3)이 같다. 그래서 미리보기에 그린 테두리가
// 사진에서도 같은 자리다. 앱은 세로 고정이라 회전 각도도 90도로 고정한다.
// @see docs/capture.md

final class CameraService: NSObject, FrameSource, @unchecked Sendable {
    /// 세로로 든 후면 카메라 버퍼를 표시 방향으로 돌리는 방향
    static let bufferOrientation: CGImagePropertyOrientation = .right
    /// 세로 고정 앱의 사진, 미리보기 회전 각도
    static let portraitRotationAngle: CGFloat = 90

    let session = AVCaptureSession()

    private let sessionQueue = DispatchQueue(label: "leaf.camera.session")
    private let analysisQueue = DispatchQueue(label: "leaf.camera.analysis", qos: .userInitiated)
    private let photoOutput = AVCapturePhotoOutput()
    private let videoOutput = AVCaptureVideoDataOutput()
    private var configured = false
    private var throttle = FrameThrottle()
    private var onFrame: (@Sendable (AnalysisFrame) -> Void)?
    private let pending = PendingPhotos()

    func start(onFrame: @escaping @Sendable (AnalysisFrame) -> Void) async throws {
        guard await Self.requestAccess() else { throw FrameSourceError.notAuthorized }
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            sessionQueue.async {
                do {
                    try self.configureIfNeeded()
                    self.analysisQueue.sync { self.onFrame = onFrame }
                    if !self.session.isRunning { self.session.startRunning() }
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    func stop() {
        sessionQueue.async {
            if self.session.isRunning { self.session.stopRunning() }
        }
    }

    func capturePhoto() async throws -> CapturedPhoto {
        try await withCheckedThrowingContinuation { continuation in
            sessionQueue.async {
                let settings: AVCapturePhotoSettings
                if self.photoOutput.availablePhotoCodecTypes.contains(.hevc) {
                    settings = AVCapturePhotoSettings(format: [AVVideoCodecKey: AVVideoCodecType.hevc])
                } else {
                    continuation.resume(throwing: FrameSourceError.captureFailed("이 기기는 HEIC를 지원하지 않습니다"))
                    return
                }
                settings.maxPhotoDimensions = self.photoOutput.maxPhotoDimensions
                settings.photoQualityPrioritization = .quality
                if let connection = self.photoOutput.connection(with: .video),
                   connection.isVideoRotationAngleSupported(Self.portraitRotationAngle) {
                    connection.videoRotationAngle = Self.portraitRotationAngle
                }
                self.pending.add(settings.uniqueID, continuation)
                self.photoOutput.capturePhoto(with: settings, delegate: self)
            }
        }
    }

    private static func requestAccess() async -> Bool {
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized: return true
        case .notDetermined: return await AVCaptureDevice.requestAccess(for: .video)
        default: return false
        }
    }

    private func configureIfNeeded() throws {
        guard !configured else { return }
        session.beginConfiguration()
        defer { session.commitConfiguration() }
        session.sessionPreset = .photo

        // 문서 촬영은 광각이 왜곡이 적다. 거리가 가까우면 자동 매크로 전환이 있는 기기가 낫다
        guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back) else {
            throw FrameSourceError.cameraUnavailable
        }
        let input = try AVCaptureDeviceInput(device: device)
        guard session.canAddInput(input) else { throw FrameSourceError.cameraUnavailable }
        session.addInput(input)

        guard session.canAddOutput(photoOutput) else { throw FrameSourceError.cameraUnavailable }
        session.addOutput(photoOutput)
        if let largest = device.activeFormat.supportedMaxPhotoDimensions.max(by: { $0.width * $0.height < $1.width * $1.height }) {
            photoOutput.maxPhotoDimensions = largest
        }
        photoOutput.maxPhotoQualityPrioritization = .quality

        videoOutput.alwaysDiscardsLateVideoFrames = true
        videoOutput.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
        videoOutput.setSampleBufferDelegate(self, queue: analysisQueue)
        guard session.canAddOutput(videoOutput) else { throw FrameSourceError.cameraUnavailable }
        session.addOutput(videoOutput)

        // 문서는 가까이 있고 정지해 있다. 초점은 계속 맞추고, 노출은 화면 가운데 기준
        try device.lockForConfiguration()
        if device.isFocusModeSupported(.continuousAutoFocus) { device.focusMode = .continuousAutoFocus }
        if device.isAutoFocusRangeRestrictionSupported { device.autoFocusRangeRestriction = .near }
        if device.isExposureModeSupported(.continuousAutoExposure) { device.exposureMode = .continuousAutoExposure }
        device.unlockForConfiguration()

        configured = true
    }
}

extension CameraService: AVCaptureVideoDataOutputSampleBufferDelegate {
    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        let time = CMSampleBufferGetPresentationTimeStamp(sampleBuffer).seconds
        guard throttle.shouldAnalyze(at: time),
              let onFrame,
              let buffer = CMSampleBufferGetImageBuffer(sampleBuffer)
        else { return }
        onFrame(AnalysisFrame(image: .pixelBuffer(buffer), orientation: Self.bufferOrientation, time: time, hint: nil))
    }
}

extension CameraService: AVCapturePhotoCaptureDelegate {
    func photoOutput(_ output: AVCapturePhotoOutput, didFinishProcessingPhoto photo: AVCapturePhoto, error: Error?) {
        let id = photo.resolvedSettings.uniqueID
        if let error {
            pending.finish(id, .failure(FrameSourceError.captureFailed(error.localizedDescription)))
            return
        }
        guard let data = photo.fileDataRepresentation() else {
            pending.finish(id, .failure(FrameSourceError.captureFailed("사진 데이터가 없습니다")))
            return
        }
        pending.finish(id, .success(CapturedPhoto(data: data, capturedAt: .now)))
    }
}

/// 사진 요청 id별 continuation. 셔터를 연달아 눌러도 각자 자기 사진을 받는다
private final class PendingPhotos: @unchecked Sendable {
    private let lock = NSLock()
    private var waiting: [Int64: CheckedContinuation<CapturedPhoto, Error>] = [:]

    func add(_ id: Int64, _ continuation: CheckedContinuation<CapturedPhoto, Error>) {
        lock.withLock { waiting[id] = continuation }
    }

    func finish(_ id: Int64, _ result: Result<CapturedPhoto, Error>) {
        let continuation = lock.withLock { waiting.removeValue(forKey: id) }
        continuation?.resume(with: result)
    }
}

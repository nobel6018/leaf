import SwiftUI

// 촬영 화면. 1단계에서 카메라 미리보기, 테두리 표시, 자동 촬영을 붙인다(docs/capture.md)
struct CaptureView: View {
    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "leaf")
                .font(.system(size: 48))
                .foregroundStyle(.green)
            Text("leaf")
                .font(.title)
            Text("거치대에 올리고 종이만 바꿔 놓으세요.")
                .foregroundStyle(.secondary)
        }
    }
}

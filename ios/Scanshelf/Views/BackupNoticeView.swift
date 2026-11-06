import SwiftUI
import UniformTypeIdentifiers

// 스캔이 이 iPhone에만 있을 때 문서 목록과 촬영 화면에 계속 띄우는 안내. 촬영은 막지 않는다.
// @see docs/ios.md

struct BackupNoticeView: View {
    @Environment(AppModel.self) private var app
    let notice: BackupNotice
    /// 촬영 화면은 미리보기 자리를 뺏지 않게 한 줄로 쓴다
    var compact = false

    @State private var picking = false
    @State private var failure: String?

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "exclamationmark.icloud.fill")
                .foregroundStyle(.orange)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(notice.title)
                    .font(compact ? .caption.weight(.semibold) : .subheadline.weight(.semibold))
                if !compact {
                    Text(failure ?? notice.message)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else if let failure {
                    Text(failure).font(.caption2)
                }
            }
            Spacer(minLength: 4)
            Button("폴더 고르기") { picking = true }
                .font(compact ? .caption.weight(.semibold) : .subheadline.weight(.semibold))
                .buttonStyle(.borderedProminent)
                .tint(.orange)
                .frame(minHeight: 44)
                .accessibilityIdentifier("pickICloudFolder")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, compact ? 2 : 6)
        .background(compact ? AnyShapeStyle(Color.white.opacity(0.14)) : AnyShapeStyle(.regularMaterial))
        .accessibilityIdentifier("backupNotice")
        .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
            do {
                try app.chooseICloudFolder(result.get())
                failure = nil
            } catch {
                failure = error.localizedDescription
            }
        }
    }
}

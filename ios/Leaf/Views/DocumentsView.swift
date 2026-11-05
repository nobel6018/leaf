import SwiftUI

// 첫 화면: 문서 목록과 보낸 상태, 새 문서 시작.
// @see docs/ios.md

struct DocumentsView: View {
    @Environment(AppModel.self) private var app
    @State private var showingNew = false
    @State private var capturing: CaptureRequest?
    @State private var showingSettings = false

    var body: some View {
        List {
            if let notice = app.notice {
                Text(notice).foregroundStyle(.secondary)
            }
            ForEach(sessions, id: \.self) { session in
                Section(session.isEmpty ? "세션 없음" : session) {
                    ForEach(app.documents.filter { ($0.session ?? "") == session }) { manifest in
                        NavigationLink(value: manifest.id) {
                            DocumentRow(manifest: manifest, status: app.status(of: manifest))
                        }
                    }
                    .onDelete { offsets in
                        let inSession = app.documents.filter { ($0.session ?? "") == session }
                        for index in offsets {
                            try? app.deleteDocument(inSession[index].id)
                        }
                    }
                }
            }
        }
        .overlay {
            if app.documents.isEmpty {
                ContentUnavailableView(
                    "아직 문서가 없습니다",
                    systemImage: "leaf",
                    description: Text("거치대에 iPhone을 올리고 새 문서를 시작하세요. 종이만 바꿔 놓으면 한 장씩 찍힙니다.")
                )
            }
        }
        .navigationTitle("leaf")
        .navigationDestination(for: UUID.self) { id in
            DocumentDetailView(documentID: id) { manifest in capturing = CaptureRequest(manifest: manifest) }
        }
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button("설정", systemImage: "gearshape") { showingSettings = true }
                    .accessibilityIdentifier("settings")
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button("모두 보내기", systemImage: "paperplane") {
                    Task { await app.sendPending() }
                }
                .disabled(app.settings.pairedMac == nil && !app.iCloud.isChosen)
            }
        }
        .safeAreaInset(edge: .bottom) {
            Button {
                showingNew = true
            } label: {
                Label("새 문서", systemImage: "plus")
                    .font(.headline)
                    .frame(maxWidth: .infinity, minHeight: 50)
            }
            .buttonStyle(.borderedProminent)
            .tint(.green)
            .padding()
            .accessibilityIdentifier("newDocument")
        }
        .sheet(isPresented: $showingNew) {
            NewDocumentView { manifest, layout in
                showingNew = false
                capturing = CaptureRequest(manifest: manifest, layout: layout)
            }
        }
        .sheet(isPresented: $showingSettings) {
            NavigationStack { SettingsView() }
        }
        .fullScreenCover(item: $capturing) { request in
            CaptureView(document: request.manifest, layout: request.layout, app: app)
                .onDisappear {
                    // 촬영을 마치면 바로 보내 본다. Mac이 없으면 기록만 남고 다음에 다시 시도한다
                    Task { await app.sendPending() }
                }
        }
        #if DEBUG
        .task { openDebugScreen() }
        #endif
    }

    #if DEBUG
    /// 시뮬레이터 화면 확인용. `-leafScreen capture|new|settings` 로 켜면 그 화면을 바로 연다(docs/ios.md)
    private func openDebugScreen() {
        guard let screen = UserDefaults.standard.string(forKey: "leafScreen") else { return }
        switch screen {
        case "capture":
            let manifest = app.documents.first { $0.pages.isEmpty }
                ?? (try? app.createDocument(name: "", kind: .pamphlet, session: "2026 예시박람회"))
            capturing = manifest.map { CaptureRequest(manifest: $0) }
        case "new": showingNew = true
        case "settings": showingSettings = true
        default: break
        }
    }
    #endif

    /// 최근 문서의 세션이 위로 온다
    private var sessions: [String] {
        var seen: [String] = []
        for manifest in app.documents where !seen.contains(manifest.session ?? "") {
            seen.append(manifest.session ?? "")
        }
        return seen
    }
}

struct DocumentRow: View {
    let manifest: Manifest
    let status: SendStatus

    var body: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(manifest.name).font(.headline).lineLimit(1)
                Text("\(manifest.kind.label), \(manifest.pages.count)장, \(manifest.createdAt.formatted(date: .abbreviated, time: .shortened))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            SendStatusBadge(status: status)
        }
        .padding(.vertical, 2)
    }
}

struct SendStatusBadge: View {
    let status: SendStatus

    var body: some View {
        switch status {
        case .notSent:
            Label("안 보냄", systemImage: "iphone").labelStyle(.badge).foregroundStyle(.secondary)
        case let .sending(progress):
            ProgressView(value: progress).frame(width: 60)
        case .awaitingMac:
            Label("iCloud 대기", systemImage: "icloud.and.arrow.up").labelStyle(.badge).foregroundStyle(.blue)
        case .sent:
            Label("Mac에 있음", systemImage: "checkmark.circle.fill").labelStyle(.badge).foregroundStyle(.green)
        case .changedSinceSent:
            Label("바뀜", systemImage: "arrow.triangle.2.circlepath").labelStyle(.badge).foregroundStyle(.orange)
        case .failed:
            Label("실패", systemImage: "exclamationmark.triangle.fill").labelStyle(.badge).foregroundStyle(.red)
        }
    }
}

private struct BadgeLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 4) {
            configuration.icon
            configuration.title
        }
        .font(.caption.weight(.medium))
    }
}

extension LabelStyle where Self == BadgeLabelStyle {
    fileprivate static var badge: BadgeLabelStyle { BadgeLabelStyle() }
}

extension DocumentKind {
    var label: String {
        switch self {
        case .pamphlet: "팜플렛"
        case .book: "책"
        case .document: "문서"
        }
    }
}

/// 촬영 화면을 여는 요청. 새 문서는 고른 배치로, 이어 찍기는 마지막 장의 배치로 시작한다
struct CaptureRequest: Identifiable {
    var manifest: Manifest
    var layout: PageLayout?

    var id: UUID { manifest.id }
}

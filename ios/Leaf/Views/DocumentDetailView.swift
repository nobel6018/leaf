import SwiftUI

// 문서 하나: 이름, 종류, 세션 고치기, 찍은 장 보기와 지우기, 이어서 찍기, 보내기.
// @see docs/ios.md

struct DocumentDetailView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss

    let documentID: UUID
    let onContinue: (Manifest) -> Void

    @State private var name = ""
    @State private var session = ""
    @State private var kind: DocumentKind = .pamphlet
    @State private var confirmDelete = false
    @State private var failure: String?

    var body: some View {
        if let manifest = app.document(documentID) {
            content(manifest)
        } else {
            ContentUnavailableView("문서가 없습니다", systemImage: "doc")
        }
    }

    private func content(_ manifest: Manifest) -> some View {
        Form {
            Section("정보") {
                TextField("이름", text: $name)
                    .onSubmit { save(manifest) }
                TextField("묶음", text: $session)
                    .onSubmit { save(manifest) }
                Picker("종류", selection: $kind) {
                    Text("문서").tag(DocumentKind.document)
                    Text("책").tag(DocumentKind.book)
                    Text("팜플렛").tag(DocumentKind.pamphlet)
                }
                .onChange(of: kind) { save(manifest) }
            }

            Section("보내기") {
                HStack {
                    Text("상태")
                    Spacer()
                    SendStatusBadge(status: app.status(of: manifest))
                }
                if case let .failed(reason) = app.status(of: manifest) {
                    Text(reason).font(.footnote).foregroundStyle(.red)
                }
                Button("Mac으로 보내기 (Wi-Fi)", systemImage: "wifi") {
                    Task { await app.send(manifest.id, route: .wifi) }
                }
                .disabled(app.settings.pairedMac == nil || manifest.pages.isEmpty)
                Button("iCloud 폴더로 보내기", systemImage: "icloud") {
                    Task { await app.send(manifest.id, route: .icloud) }
                }
                .disabled(!app.iCloud.isChosen || manifest.pages.isEmpty)
            }

            Section("\(manifest.pages.count)장") {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 90), spacing: 10)], spacing: 10) {
                    ForEach(Array(manifest.pages.enumerated()), id: \.element.id) { index, page in
                        PageThumbnail(url: app.store.fileURL(of: manifest.id, relativePath: page.raw), quad: page.quad)
                            .frame(height: 120)
                            .overlay(alignment: .bottomLeading) {
                                Text(page.layout == .spread ? "\(index + 1) 두 쪽" : "\(index + 1)")
                                    .font(.caption2.bold())
                                    .padding(3)
                                    .background(.thinMaterial, in: .rect(cornerRadius: 4))
                            }
                            .contextMenu {
                                Button("지우기", systemImage: "trash", role: .destructive) {
                                    do { _ = try app.deletePage(page.id, from: manifest) } catch { failure = error.localizedDescription }
                                }
                            }
                    }
                }
                .padding(.vertical, 4)
            }

            if let failure {
                Text(failure).foregroundStyle(.red)
            }

            Section {
                Button("문서 지우기", role: .destructive) { confirmDelete = true }
            }
        }
        .navigationTitle(manifest.name)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button("이어서 찍기", systemImage: "camera") { onContinue(manifest) }
            }
        }
        .onAppear {
            name = manifest.name
            session = manifest.session ?? ""
            kind = manifest.kind
        }
        .onDisappear { save(manifest) }
        .confirmationDialog("이 iPhone에서 문서와 원본을 지웁니다", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("지우기", role: .destructive) {
                try? app.deleteDocument(manifest.id)
                dismiss()
            }
        } message: {
            Text("Mac으로 보낸 문서는 Mac에 남습니다.")
        }
    }

    private func save(_ manifest: Manifest) {
        guard let current = app.document(manifest.id) else { return }
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        guard trimmed != current.name || session != (current.session ?? "") || kind != current.kind else { return }
        do {
            try app.updateMetadata(current, name: trimmed, kind: kind, session: session)
        } catch {
            failure = error.localizedDescription
        }
    }
}

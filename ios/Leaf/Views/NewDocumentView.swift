import SwiftUI

// 새 문서 시작. 세션(박람회명)은 바꾸기 전까지 기억하고, 이름은 기본으로 `세션_번호`를 넣는다.
// 팜플렛 이름은 나중에 Mac AI가 표지에서 회사명을 추천한다(docs/features.md).
// @see docs/ios.md

struct NewDocumentView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss

    let onStart: (Manifest, PageLayout) -> Void

    @State private var kind: DocumentKind = .pamphlet
    @State private var session = ""
    @State private var name = ""
    /// 이름을 직접 고쳤으면 세션을 바꿔도 덮어쓰지 않는다
    @State private var nameEdited = false
    @State private var spread = true
    @State private var failure: String?

    var body: some View {
        NavigationStack {
            Form {
                Section("종류") {
                    Picker("종류", selection: $kind) {
                        Text("팜플렛").tag(DocumentKind.pamphlet)
                        Text("책").tag(DocumentKind.book)
                        Text("문서").tag(DocumentKind.document)
                    }
                    .pickerStyle(.segmented)
                    .accessibilityIdentifier("kindPicker")
                    if kind == .book {
                        Toggle("펼친 두 페이지로 찍기", isOn: $spread)
                    }
                }
                Section {
                    TextField("예: 2026 데이터센터코리아", text: $session)
                        .accessibilityIdentifier("sessionField")
                } header: {
                    Text("세션(박람회명)")
                } footer: {
                    Text("바꾸기 전까지 다음 문서에도 붙습니다.")
                }
                Section {
                    TextField("문서 이름", text: $name)
                        .onChange(of: name) { _, value in
                            if value != app.defaultName(session: session) { nameEdited = true }
                        }
                        .accessibilityIdentifier("nameField")
                } header: {
                    Text("이름")
                } footer: {
                    Text(kind == .pamphlet ? "Mac이 표지에서 회사명을 찾아 이름을 추천합니다." : "")
                }
                if let failure {
                    Text(failure).foregroundStyle(.red)
                }
            }
            .navigationTitle("새 문서")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("취소") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("촬영 시작") { start() }
                        .bold()
                        .accessibilityIdentifier("startCapture")
                }
            }
            .onAppear {
                session = app.settings.lastSession
                name = app.defaultName(session: session)
                nameEdited = false
            }
            .onChange(of: session) { _, value in
                if !nameEdited { name = app.defaultName(session: value) }
            }
        }
    }

    private func start() {
        do {
            let manifest = try app.createDocument(name: name, kind: kind, session: session)
            onStart(manifest, kind == .book && spread ? .spread : .single)
        } catch {
            failure = error.localizedDescription
        }
    }
}

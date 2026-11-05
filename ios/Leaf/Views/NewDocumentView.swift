import SwiftUI

// 새 문서 시작. 종류, 묶음, 배치는 바꾸기 전까지 기억하고, 이름은 기본으로 `묶음_번호`(묶음이 없으면 `종류_번호`)를 넣는다.
// 팜플렛 이름은 나중에 Mac AI가 표지에서 회사명을 추천한다(docs/features.md).
// @see docs/ios.md

struct NewDocumentView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss

    let onStart: (Manifest, PageLayout) -> Void

    @State private var kind: DocumentKind = .document
    @State private var session = ""
    @State private var name = ""
    /// 이름을 직접 고쳤으면 묶음이나 종류를 바꿔도 덮어쓰지 않는다
    @State private var nameEdited = false
    /// 종류마다 고른 배치. 종류를 오가도 각자 기억한다
    @State private var layouts: [DocumentKind: PageLayout] = [:]
    @State private var failure: String?

    var body: some View {
        NavigationStack {
            Form {
                Section("종류") {
                    Picker("종류", selection: $kind) {
                        ForEach(DocumentKind.displayOrder, id: \.self) { value in
                            Text(value.label).tag(value)
                        }
                    }
                    .pickerStyle(.segmented)
                    .accessibilityIdentifier("kindPicker")
                    switch kind {
                    case .book:
                        Toggle("펼친 두 페이지로 찍기", isOn: layoutToggle(.spread))
                    case .pamphlet:
                        Toggle("펼친 3단 접지로 찍기", isOn: layoutToggle(.trifold))
                            .accessibilityIdentifier("trifoldToggle")
                    case .document:
                        EmptyView()
                    }
                }
                Section {
                    TextField("예: 2026 데이터센터코리아, 사피엔스", text: $session)
                        .accessibilityIdentifier("sessionField")
                } header: {
                    Text("묶음")
                } footer: {
                    Text("여러 문서를 함께 모아 볼 이름입니다. 박람회명이나 책 시리즈처럼 쓰고, 바꾸기 전까지 다음 문서에도 붙습니다.")
                }
                Section {
                    TextField("문서 이름", text: $name)
                        .onChange(of: name) { _, value in
                            if value != app.defaultName(session: session, kind: kind) { nameEdited = true }
                        }
                        .accessibilityIdentifier("nameField")
                } header: {
                    Text("이름")
                } footer: {
                    Text(nameFooter)
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
                kind = app.settings.lastKind
                session = app.settings.lastSession
                layouts = Dictionary(uniqueKeysWithValues: DocumentKind.displayOrder.map { ($0, app.settings.lastLayout(for: $0)) })
                name = app.defaultName(session: session, kind: kind)
                nameEdited = false
            }
            .onChange(of: session) { _, value in
                if !nameEdited { name = app.defaultName(session: value, kind: kind) }
            }
            .onChange(of: kind) { _, value in
                if !nameEdited { name = app.defaultName(session: session, kind: value) }
            }
        }
    }

    private var nameFooter: String {
        kind == .pamphlet ? "Mac이 표지에서 회사명을 찾아 이름을 추천합니다." : ""
    }

    private var layout: PageLayout {
        layouts[kind] ?? kind.defaultLayout
    }

    /// 켜면 그 배치, 끄면 한 장
    private func layoutToggle(_ wide: PageLayout) -> Binding<Bool> {
        Binding(
            get: { layout == wide },
            set: { layouts[kind] = $0 ? wide : .single }
        )
    }

    private func start() {
        do {
            app.settings.lastKind = kind
            app.settings.setLastLayout(layout, for: kind)
            let manifest = try app.createDocument(name: name, kind: kind, session: session)
            onStart(manifest, layout)
        } catch {
            failure = error.localizedDescription
        }
    }
}

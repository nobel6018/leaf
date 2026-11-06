import SwiftUI
import UniformTypeIdentifiers

// 설정: 짝지은 Mac, iCloud 폴더, 원본 자동 삭제 기간, 자동 촬영 민감도.
// @see docs/ios.md

struct SettingsView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    @State private var pickingFolder = false
    @State private var confirmUnpair = false
    @State private var failure: String?

    var body: some View {
        @Bindable var settings = app.settings
        Form {
            Section {
                if let mac = app.settings.pairedMac {
                    LabeledContent("Mac", value: mac.macName)
                    LabeledContent("짝지은 날", value: mac.pairedAt.formatted(date: .abbreviated, time: .shortened))
                    if let address = mac.lastAddress {
                        LabeledContent("마지막 주소", value: "\(address.host):\(address.port)")
                    }
                    Button("짝짓기 해제", role: .destructive) { confirmUnpair = true }
                } else {
                    NavigationLink("Mac 연결") { PairingView() }
                        .accessibilityIdentifier("pairMac")
                }
            } header: {
                Text("Wi-Fi로 보내기")
            } footer: {
                Text("Mac의 scanshelf 설정에서 \"iPhone 연결\"을 누르면 6자리 코드가 나옵니다. 같은 Wi-Fi에서만 보냅니다.")
            }

            Section {
                if let root = app.iCloud.scanshelfRoot {
                    LabeledContent("폴더", value: root.lastPathComponent)
                    Button("폴더 잊기", role: .destructive) { app.iCloud.forget() }
                }
                Button(app.iCloud.isChosen ? "다른 폴더 고르기" : "iCloud Drive/scanshelf 고르기") { pickingFolder = true }
            } header: {
                Text("iCloud 폴더로 보내기")
            } footer: {
                Text("Mac이 같은 Wi-Fi에 없을 때 씁니다. 파일 앱에서 iCloud Drive의 scanshelf 폴더(없으면 iCloud Drive)를 고르세요.")
            }

            Section {
                Picker("원본 지우기", selection: $settings.retentionDays) {
                    ForEach(AppSettings.retentionChoices, id: \.self) { days in
                        Text(days == 0 ? "지우지 않음" : "보낸 뒤 \(days)일").tag(days)
                    }
                }
            } header: {
                Text("저장 공간")
            } footer: {
                Text("Mac이 받은 것이 확인되고 그 뒤 바뀌지 않은 문서만 지웁니다. Mac에는 원본이 남습니다.")
            }

            Section {
                Picker("자동 촬영 민감도", selection: $settings.sensitivity) {
                    ForEach(AutoCaptureSensitivity.allCases, id: \.self) { value in
                        Text(value.label).tag(value)
                    }
                }
                Toggle("자동 촬영", isOn: $settings.autoCaptureEnabled)
            } header: {
                Text("촬영")
            } footer: {
                Text("민감하게: 멈춘 지 \(String(format: "%.1f", AutoCaptureTuning.high.stableDuration))초 뒤, 보통: \(String(format: "%.1f", AutoCaptureTuning.normal.stableDuration))초 뒤, 둔하게: \(String(format: "%.1f", AutoCaptureTuning.low.stableDuration))초 뒤에 찍습니다.")
            }

            if let failure {
                Text(failure).foregroundStyle(.red)
            }

            Section("이 iPhone") {
                LabeledContent("기기 id", value: String(app.settings.deviceID.uuidString.lowercased().prefix(8)))
                LabeledContent("버전", value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "")
            }
        }
        .navigationTitle("설정")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button("닫기") { dismiss() }
            }
        }
        .fileImporter(isPresented: $pickingFolder, allowedContentTypes: [.folder]) { result in
            do {
                try app.iCloud.choose(result.get())
                failure = nil
            } catch {
                failure = error.localizedDescription
            }
        }
        .confirmationDialog("짝짓기를 해제합니다", isPresented: $confirmUnpair, titleVisibility: .visible) {
            Button("해제", role: .destructive) { app.unpair() }
        } message: {
            Text("다시 보내려면 Mac에서 새 코드를 받아 짝지어야 합니다.")
        }
    }
}

struct PairingView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    @State private var selected: DiscoveredMac?
    @State private var code = ""
    @State private var pairing = false
    @State private var failure: String?
    @FocusState private var codeFocused: Bool

    /// Mac이 보여 주는 짝짓기 코드 길이
    static let codeLength = 6

    var body: some View {
        Form {
            Section {
                if app.browser.macs.isEmpty {
                    HStack {
                        ProgressView()
                        Text("같은 Wi-Fi의 Mac을 찾는 중")
                            .foregroundStyle(.secondary)
                    }
                }
                ForEach(app.browser.macs) { mac in
                    Button {
                        selected = mac
                        codeFocused = true
                    } label: {
                        HStack {
                            Label(mac.name, systemImage: "desktopcomputer")
                            Spacer()
                            if selected == mac { Image(systemName: "checkmark") }
                        }
                    }
                    .frame(minHeight: 44)
                }
                if let failure = app.browser.failure {
                    Text(failure).font(.footnote).foregroundStyle(.red)
                }
            } header: {
                Text("Mac 고르기")
            } footer: {
                Text("Mac에서 scanshelf를 열어 두어야 보입니다.")
            }

            if selected != nil {
                Section("Mac에 보이는 6자리 코드") {
                    TextField("000000", text: $code)
                        .keyboardType(.numberPad)
                        .textContentType(.oneTimeCode)
                        .font(.system(size: 28, weight: .semibold, design: .monospaced))
                        .focused($codeFocused)
                        .onChange(of: code) { _, value in
                            let digits = String(value.filter(\.isNumber).prefix(Self.codeLength))
                            if digits != value { code = digits }
                        }
                    Button {
                        Task { await pair() }
                    } label: {
                        if pairing { ProgressView() } else { Text("연결") }
                    }
                    .disabled(code.count != Self.codeLength || pairing)
                }
            }

            if let failure {
                Text(failure).foregroundStyle(.red)
            }
        }
        .navigationTitle("Mac 연결")
        .onAppear { app.browser.start() }
    }

    private func pair() async {
        guard let selected else { return }
        pairing = true
        defer { pairing = false }
        do {
            try await app.pair(with: selected, code: code)
            dismiss()
        } catch {
            failure = "연결하지 못했습니다: \(error.localizedDescription)"
            code = ""
        }
    }
}

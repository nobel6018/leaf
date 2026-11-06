import Foundation
import UIKit

// 앱 전체 상태: 문서 목록, 보낸 기록, 보내기(Wi-Fi 먼저, 안 되면 iCloud 폴더), 원본 자동 삭제.
// @see docs/ios.md, docs/transfer.md

@MainActor
@Observable
final class AppModel {
    /// Bonjour로 페어링한 Mac을 기다리는 시간. 넘으면 마지막 주소로 시도한다
    static let discoveryTimeout: TimeInterval = 4

    let store: DocumentStore
    let settings: AppSettings
    let browser = MacBrowser()
    let iCloud = ICloudFolder()

    private(set) var documents: [Manifest] = []
    private(set) var records: [UUID: SendRecord] = [:]
    private(set) var progress: [UUID: Double] = [:]
    private(set) var failures: [UUID: String] = [:]
    /// 화면 위에 잠깐 띄우는 안내
    var notice: String?

    private let recordStore: SendRecordStore
    private let urlSession: URLSession

    init(
        store: DocumentStore = .appDefault(),
        recordStore: SendRecordStore = .appDefault(),
        settings: AppSettings = AppSettings()
    ) {
        self.store = store
        self.recordStore = recordStore
        self.settings = settings
        let configuration = URLSessionConfiguration.default
        configuration.timeoutIntervalForRequest = 30
        configuration.timeoutIntervalForResource = 600
        configuration.waitsForConnectivity = false
        urlSession = URLSession(configuration: configuration)
        try? FileManager.default.createDirectory(at: store.root, withIntermediateDirectories: true)
        reload()
    }

    func reload() {
        documents = store.list()
        records = recordStore.load()
    }

    func document(_ id: UUID) -> Manifest? {
        documents.first { $0.id == id }
    }

    func status(of manifest: Manifest) -> SendStatus {
        if let value = progress[manifest.id] { return .sending(progress: value) }
        if let failure = failures[manifest.id] { return .failed(failure) }
        return SendStatus.of(record: records[manifest.id], currentFingerprint: try? manifest.fingerprint())
    }

    // MARK: 문서

    func defaultName(session: String, kind: DocumentKind) -> String {
        DocumentNaming.defaultName(session: session, kind: kind, existingNames: documents.map(\.name))
    }

    func createDocument(name: String, kind: DocumentKind, session: String) throws -> Manifest {
        let trimmedSession = session.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        settings.lastSession = trimmedSession
        let manifest = try store.create(
            name: trimmedName.isEmpty ? defaultName(session: trimmedSession, kind: kind) : trimmedName,
            kind: kind,
            session: trimmedSession.isEmpty ? nil : trimmedSession
        )
        documents.insert(manifest, at: 0)
        return manifest
    }

    func replace(_ manifest: Manifest) {
        if let index = documents.firstIndex(where: { $0.id == manifest.id }) {
            documents[index] = manifest
        } else {
            documents.insert(manifest, at: 0)
        }
        failures[manifest.id] = nil
    }

    func updateMetadata(_ manifest: Manifest, name: String, kind: DocumentKind, session: String) throws {
        let trimmed = session.trimmingCharacters(in: .whitespacesAndNewlines)
        replace(try store.updateMetadata(of: manifest, name: name, kind: kind, session: trimmed.isEmpty ? nil : trimmed))
    }

    func deletePage(_ pageID: UUID, from manifest: Manifest) throws -> Manifest {
        let updated = try store.deletePage(pageID, from: manifest)
        replace(updated)
        return updated
    }

    func deleteDocument(_ id: UUID) throws {
        try store.deleteDocument(id)
        documents.removeAll { $0.id == id }
        records[id] = nil
        try? recordStore.save(records)
    }

    // MARK: 보내기

    enum Route {
        case wifi
        case icloud
        /// Wi-Fi로 Mac을 찾고, 못 찾으면 iCloud 폴더
        case automatic
    }

    /// 아직 안 보냈거나 보낸 뒤 바뀐 문서를 보낸다. 앱이 앞으로 올 때와 촬영을 마칠 때 부른다
    func sendPending() async {
        await confirmICloudImports()
        let pending = documents.filter { manifest in
            guard !manifest.pages.isEmpty, progress[manifest.id] == nil else { return false }
            switch status(of: manifest) {
            case .notSent, .changedSinceSent, .failed: return true
            default: return false
            }
        }
        guard !pending.isEmpty, settings.pairedMac != nil || iCloud.isChosen else { return }
        for manifest in pending {
            await send(manifest.id, route: .automatic)
        }
        applyRetention()
    }

    func send(_ documentID: UUID, route: Route) async {
        guard progress[documentID] == nil else { return }
        progress[documentID] = 0
        failures[documentID] = nil
        // 화면을 끄거나 앱을 내려도 보내던 파일 하나는 마칠 수 있게 시간을 얻는다(약 30초). 그 뒤는 앞으로 오면 이어 보낸다
        let task = UIApplication.shared.beginBackgroundTask(withName: "scanshelf.send") {}
        defer {
            UIApplication.shared.endBackgroundTask(task)
            progress[documentID] = nil
        }
        do {
            switch route {
            case .wifi:
                try await sendOverWiFi(documentID)
            case .icloud:
                try await sendToICloud(documentID)
            case .automatic:
                do {
                    try await sendOverWiFi(documentID)
                } catch let error as TransferError where error == .unauthorized {
                    throw error
                } catch {
                    guard iCloud.isChosen else { throw error }
                    try await sendToICloud(documentID)
                }
            }
        } catch {
            failures[documentID] = error.localizedDescription
        }
    }

    private func sendOverWiFi(_ documentID: UUID) async throws {
        guard let paired = settings.pairedMac else { throw TransferError.notPaired }
        guard let tokenData = try Keychain.macToken.read(account: Self.tokenAccount),
              let tokenHex = String(data: tokenData, encoding: .utf8),
              let token = Data(hexString: tokenHex)
        else { throw TransferError.notPaired }

        let address = try await locate(paired)
        guard let baseURL = address.baseURL else { throw URLError(.badURL) }
        let client = TransferClient(baseURL: baseURL, session: urlSession)
        let signer = RequestSigner(deviceID: settings.deviceID, token: token)
        let store = store
        let sent = try await client.upload(documentID: documentID, store: store, signer: signer) { value in
            await MainActor.run { [weak self] in self?.progress[documentID] = value }
        }
        try record(SendRecord(method: .wifi, sentAt: .now, manifestFingerprint: sent.fingerprint(), confirmed: true), for: documentID)
    }

    private func sendToICloud(_ documentID: UUID) async throws {
        let root = try iCloud.resolve()
        let store = store
        let sent = try await Task.detached(priority: .userInitiated) {
            try ICloudExporter.export(documentID: documentID, store: store, scanshelfRoot: root)
        }.value
        try record(SendRecord(method: .icloud, sentAt: .now, manifestFingerprint: sent.fingerprint(), confirmed: false), for: documentID)
    }

    private func locate(_ paired: PairedMac) async throws -> MacAddress {
        if let found = await browser.waitFor(name: paired.serviceName, timeout: Self.discoveryTimeout),
           let address = try? await MacBrowser.resolve(found.endpoint) {
            if settings.pairedMac?.lastAddress != address {
                settings.pairedMac?.lastAddress = address
            }
            return address
        }
        guard let last = paired.lastAddress else { throw URLError(.cannotFindHost) }
        return last
    }

    private func record(_ record: SendRecord, for documentID: UUID) throws {
        records[documentID] = record
        try recordStore.save(records)
    }

    /// iCloud로 보낸 문서 중 Mac이 inbox에서 가져간 것을 확인됨으로 바꾼다
    func confirmICloudImports() async {
        guard let root = try? iCloud.resolve() else { return }
        var changed = false
        for (id, record) in records where record.method == .icloud && !record.confirmed {
            if ICloudExporter.isImportedByMac(documentID: id, scanshelfRoot: root) {
                records[id]?.confirmed = true
                changed = true
            }
        }
        if changed { try? recordStore.save(records) }
    }

    /// Mac이 받은 것이 확인되고 그 뒤 바뀌지 않은 문서를 기간이 지나면 지운다. Mac은 원본을 지우지 않는다
    func applyRetention(now: Date = .now) {
        let fingerprints = Dictionary(uniqueKeysWithValues: documents.compactMap { manifest in
            (try? manifest.fingerprint()).map { (manifest.id, $0) }
        })
        let expired = RetentionPolicy.documentsToDelete(
            records: records,
            fingerprints: fingerprints,
            days: settings.retentionPolicyDays,
            now: now
        )
        for id in expired {
            try? deleteDocument(id)
        }
    }

    // MARK: 업데이트 알림

    /// 목록 위에 띄울 새 버전. 닫으면 그 버전은 다시 알리지 않는다
    var updateBanner: AvailableUpdate? {
        UpdateCheck.banner(
            cached: settings.availableUpdate,
            current: UpdateCheck.installedVersion,
            dismissed: settings.dismissedUpdateVersion
        )
    }

    /// 하루에 한 번 GitHub에 묻는다. 묻지 못하면 기록을 남기지 않아 다음에 앞으로 올 때 다시 묻는다
    func checkForUpdate(now: Date = .now) async {
        // 단위 테스트가 앱을 띄울 때는 GitHub에 묻지 않는다
        guard ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] == nil else { return }
        guard UpdateCheck.shouldCheck(lastCheckedAt: settings.lastUpdateCheckAt, now: now) else { return }
        guard let release = try? await UpdateCheck.fetchLatest(session: urlSession) else { return }
        settings.lastUpdateCheckAt = now
        settings.availableUpdate = UpdateCheck.available(in: release, current: UpdateCheck.installedVersion)
    }

    func dismissUpdate() {
        settings.dismissedUpdateVersion = settings.availableUpdate?.version
    }

    // MARK: 페어링

    static let tokenAccount = "paired-mac"

    func pair(with mac: DiscoveredMac, code: String) async throws {
        let address = try await MacBrowser.resolve(mac.endpoint)
        guard let baseURL = address.baseURL else { throw URLError(.badURL) }
        let client = TransferClient(baseURL: baseURL, session: urlSession)
        let response = try await client.pair(code: code, deviceID: settings.deviceID, deviceName: UIDevice.current.name)
        guard Data(hexString: response.token) != nil else { throw TransferError.invalidResponse }
        try Keychain.macToken.write(Data(response.token.utf8), account: Self.tokenAccount)
        settings.pairedMac = PairedMac(serviceName: mac.name, macName: response.macName, lastAddress: address, pairedAt: .now)
        failures = failures.filter { $0.value != TransferError.unauthorized.localizedDescription }
    }

    func unpair() {
        try? Keychain.macToken.delete(account: Self.tokenAccount)
        settings.pairedMac = nil
    }
}

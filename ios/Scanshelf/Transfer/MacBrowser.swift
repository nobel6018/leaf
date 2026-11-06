import Foundation
import Network

// 같은 Wi-Fi의 Mac을 Bonjour `_scanshelf._tcp`로 찾는다. 처음 찾을 때 iOS가 로컬 네트워크 권한을 묻는다
// (Info.plist NSLocalNetworkUsageDescription, NSBonjourServices).
// @see docs/transfer.md

struct DiscoveredMac: Identifiable, Hashable, Sendable {
    /// Bonjour 서비스 이름. Mac 앱이 정한다(보통 Mac 이름)
    let name: String
    let endpoint: NWEndpoint

    var id: String { name }
}

/// 짝지은 Mac에 HTTP로 붙을 주소
struct MacAddress: Codable, Equatable, Sendable {
    var host: String
    var port: Int

    var baseURL: URL? {
        var components = URLComponents()
        components.scheme = "http"
        components.host = host
        components.port = port
        return components.url
    }
}

@MainActor
@Observable
final class MacBrowser {
    /// Mac이 Bonjour TXT `api`로 알리는 전송 API 버전
    nonisolated static let supportedAPI = "1"

    private(set) var macs: [DiscoveredMac] = []
    private(set) var isBrowsing = false
    private(set) var failure: String?

    private var browser: NWBrowser?

    func start() {
        guard browser == nil else { return }
        let parameters = NWParameters()
        parameters.includePeerToPeer = false
        let browser = NWBrowser(for: .bonjourWithTXTRecord(type: "_scanshelf._tcp", domain: nil), using: parameters)
        browser.stateUpdateHandler = { [weak self] state in
            Task { @MainActor in self?.handle(state) }
        }
        browser.browseResultsChangedHandler = { [weak self] results, _ in
            let macs = results.compactMap { result -> DiscoveredMac? in
                guard case let .service(name, _, _, _) = result.endpoint else { return nil }
                // TXT api가 이 앱이 아는 버전과 다르면 API가 바뀐 Mac이라 고르지 않는다. TXT가 없으면 받아 준다
                if case let .bonjour(txt) = result.metadata, let api = txt["api"], api != MacBrowser.supportedAPI {
                    return nil
                }
                return DiscoveredMac(name: name, endpoint: result.endpoint)
            }
            .sorted { $0.name < $1.name }
            Task { @MainActor in self?.macs = macs }
        }
        browser.start(queue: .main)
        self.browser = browser
        isBrowsing = true
    }

    func stop() {
        browser?.cancel()
        browser = nil
        isBrowsing = false
    }

    /// 찾은 Mac 중 이름이 같은 것을 최대 `timeout`초 기다린다
    func waitFor(name: String, timeout: TimeInterval) async -> DiscoveredMac? {
        start()
        let deadline = Date.now.addingTimeInterval(timeout)
        while Date.now < deadline {
            if let mac = macs.first(where: { $0.name == name }) { return mac }
            try? await Task.sleep(for: .milliseconds(200))
        }
        return macs.first { $0.name == name }
    }

    private func handle(_ state: NWBrowser.State) {
        switch state {
        case let .failed(error):
            failure = "Mac을 찾지 못했습니다: \(error.localizedDescription)"
            stop()
        case let .waiting(error):
            // 로컬 네트워크 권한을 거절하면 여기로 온다
            failure = "로컬 네트워크 권한이 필요합니다(설정 > scanshelf): \(error.localizedDescription)"
        case .ready:
            failure = nil
        default:
            break
        }
    }

    /// Bonjour 서비스 주소를 HTTP용 IPv4 호스트와 포트로 푼다. IPv6 링크 로컬 주소는 URL에 넣기 까다로워 IPv4로 묶는다
    nonisolated static func resolve(_ endpoint: NWEndpoint, timeout: TimeInterval = 5) async throws -> MacAddress {
        let parameters = NWParameters.tcp
        if let ip = parameters.defaultProtocolStack.internetProtocol as? NWProtocolIP.Options {
            ip.version = .v4
        }
        let connection = NWConnection(to: endpoint, using: parameters)
        let resolver = ResolveOnce()
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                resolver.set(continuation)
                connection.stateUpdateHandler = { state in
                    switch state {
                    case .ready:
                        if case let .hostPort(host, port) = connection.currentPath?.remoteEndpoint {
                            resolver.finish(.success(MacAddress(host: Self.hostString(host), port: Int(port.rawValue))))
                        } else {
                            resolver.finish(.failure(URLError(.cannotFindHost)))
                        }
                        connection.cancel()
                    case let .failed(error):
                        resolver.finish(.failure(error))
                        connection.cancel()
                    case let .waiting(error):
                        resolver.finish(.failure(error))
                        connection.cancel()
                    default:
                        break
                    }
                }
                connection.start(queue: .global(qos: .userInitiated))
                DispatchQueue.global().asyncAfter(deadline: .now() + timeout) {
                    resolver.finish(.failure(URLError(.timedOut)))
                    connection.cancel()
                }
            }
        } onCancel: {
            connection.cancel()
        }
    }

    nonisolated private static func hostString(_ host: NWEndpoint.Host) -> String {
        let text: String
        switch host {
        case let .ipv4(address): text = "\(address)"
        case let .ipv6(address): text = "\(address)"
        case let .name(name, _): text = name
        @unknown default: text = "\(host)"
        }
        // "192.168.0.3%en0"처럼 인터페이스가 붙어 오면 뗀다
        return text.split(separator: "%").first.map(String.init) ?? text
    }
}

/// 연결 상태 콜백은 여러 번 오지만 continuation은 한 번만 끝내야 한다
private final class ResolveOnce: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<MacAddress, Error>?

    func set(_ continuation: CheckedContinuation<MacAddress, Error>) {
        lock.withLock { self.continuation = continuation }
    }

    func finish(_ result: Result<MacAddress, Error>) {
        let pending = lock.withLock { () -> CheckedContinuation<MacAddress, Error>? in
            defer { continuation = nil }
            return continuation
        }
        pending?.resume(with: result)
    }
}

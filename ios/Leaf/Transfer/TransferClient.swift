import Foundation

// Mac 전송 서버 API 호출. 원본을 먼저, manifest를 마지막에 보낸다. 중간에 끊겨도 다시 부르면
// status로 Mac이 이미 받은 파일(해시가 같은 것)은 건너뛰므로 이어 보내기가 된다.
// @see docs/transfer.md

enum TransferError: Error, LocalizedError, Equatable {
    /// 서명이 맞지 않는다. Mac에서 짝짓기를 지웠거나 토큰이 바뀌었다. 다시 짝지어야 한다
    case unauthorized
    case http(status: Int, message: String)
    case invalidResponse
    case notPaired

    var errorDescription: String? {
        switch self {
        case .unauthorized: "Mac이 이 iPhone을 알아보지 못합니다. 설정에서 다시 짝지어 주세요"
        case let .http(status, message): "Mac 응답 \(status): \(message)"
        case .invalidResponse: "Mac 응답을 읽지 못했습니다"
        case .notPaired: "짝지은 Mac이 없습니다"
        }
    }
}

struct MacInfo: Codable, Equatable, Sendable {
    var name: String
    var version: String
}

struct PairRequest: Codable, Equatable, Sendable {
    var code: String
    var deviceId: String
    var deviceName: String
}

struct PairResponse: Codable, Equatable, Sendable {
    var token: String
    var macName: String
}

struct DocumentStatusResponse: Codable, Equatable, Sendable {
    /// 문서 폴더 기준 상대 경로 → sha256 hex
    var files: [String: String]
}

/// 보낼 파일 하나. Mac status와 비교해 이미 같은 해시가 있으면 건너뛴다
struct UploadItem: Equatable, Sendable {
    var relativePath: String
    var fileURL: URL
    var sha256: String
}

enum UploadPlan {
    static func missing(_ items: [UploadItem], remote: [String: String]) -> [UploadItem] {
        items.filter { remote[$0.relativePath] != $0.sha256 }
    }
}

struct TransferClient: Sendable {
    let baseURL: URL
    let session: URLSession

    init(baseURL: URL, session: URLSession = .shared) {
        self.baseURL = baseURL
        self.session = session
    }

    func info() async throws -> MacInfo {
        let (data, response) = try await session.data(for: request("GET", "/v1/info"))
        try check(response, data)
        return try JSONDecoder().decode(MacInfo.self, from: data)
    }

    /// 짝짓기는 아직 토큰이 없어 서명하지 않는다
    func pair(code: String, deviceID: UUID, deviceName: String) async throws -> PairResponse {
        var request = request("POST", "/v1/pair")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONEncoder().encode(
            PairRequest(code: code, deviceId: deviceID.uuidString.lowercased(), deviceName: deviceName)
        )
        let (data, response) = try await session.data(for: request)
        try check(response, data)
        return try JSONDecoder().decode(PairResponse.self, from: data)
    }

    func status(of documentID: UUID, signer: RequestSigner) async throws -> [String: String] {
        let path = "/v1/documents/\(documentID.uuidString.lowercased())/status"
        var request = request("GET", path)
        signer.sign(&request, path: path, bodySha256: RequestSigner.sha256Hex(Data()))
        let (data, response) = try await session.data(for: request)
        // 처음 보내는 문서는 Mac이 모를 수 있다
        if (response as? HTTPURLResponse)?.statusCode == 404 { return [:] }
        try check(response, data)
        return try JSONDecoder().decode(DocumentStatusResponse.self, from: data).files
    }

    func putFile(_ item: UploadItem, documentID: UUID, signer: RequestSigner) async throws {
        let path = "/v1/documents/\(documentID.uuidString.lowercased())/files/\(item.relativePath)"
        var request = request("PUT", path)
        request.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
        signer.sign(&request, path: path, bodySha256: item.sha256)
        let (data, response) = try await session.upload(for: request, fromFile: item.fileURL)
        try check(response, data)
    }

    func putManifest(_ manifest: Data, documentID: UUID, signer: RequestSigner) async throws {
        let path = "/v1/documents/\(documentID.uuidString.lowercased())/manifest"
        var request = request("PUT", path)
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        signer.sign(&request, path: path, bodySha256: RequestSigner.sha256Hex(manifest))
        let (data, response) = try await session.upload(for: request, from: manifest)
        try check(response, data)
    }

    /// 문서 하나를 보낸다. progress는 0~1(파일 개수 기준, manifest 포함)
    func upload(
        documentID: UUID,
        store: DocumentStore,
        signer: RequestSigner,
        progress: @Sendable (Double) async -> Void
    ) async throws -> Manifest {
        // 보내는 동안 페이지가 더해질 수 있다. 읽은 순간의 manifest가 가리키는 원본만 보내고 그 manifest를 보낸다
        let manifestURL = store.manifestURL(of: documentID)
        let manifestData = try Data(contentsOf: manifestURL)
        let manifest = try Manifest.decoder().decode(Manifest.self, from: manifestData)
        try manifest.validate()

        let items = try manifest.pages.map { page in
            let url = store.fileURL(of: documentID, relativePath: page.raw)
            return UploadItem(relativePath: page.raw, fileURL: url, sha256: try RequestSigner.sha256Hex(fileAt: url))
        }
        let remote = try await status(of: documentID, signer: signer)
        let missing = UploadPlan.missing(items, remote: remote)
        let total = Double(missing.count + 1)
        await progress(0)
        for (index, item) in missing.enumerated() {
            try Task.checkCancellation()
            try await putFile(item, documentID: documentID, signer: signer)
            await progress(Double(index + 1) / total)
        }
        try await putManifest(manifestData, documentID: documentID, signer: signer)
        await progress(1)
        return manifest
    }

    private func request(_ method: String, _ path: String) -> URLRequest {
        var request = URLRequest(url: baseURL.appending(path: path))
        request.httpMethod = method
        request.timeoutInterval = 30
        return request
    }

    private func check(_ response: URLResponse, _ data: Data) throws {
        guard let http = response as? HTTPURLResponse else { throw TransferError.invalidResponse }
        if http.statusCode == 401 { throw TransferError.unauthorized }
        guard (200 ..< 300).contains(http.statusCode) else {
            throw TransferError.http(status: http.statusCode, message: String(data: data.prefix(300), encoding: .utf8) ?? "")
        }
    }
}

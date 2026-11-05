import Foundation

// Mac 전송 서버 API 호출. 원본을 먼저, manifest를 마지막에 보낸다. 중간에 끊겨도 다시 부르면
// status로 Mac이 이미 받은 파일(해시가 같은 것)은 건너뛰므로 이어 보내기가 된다.
// @see docs/transfer.md

enum TransferError: Error, LocalizedError, Equatable {
    /// 서명이 맞지 않거나 Mac이 이 기기를 모른다(Mac에서 짝을 끊었다). 다시 짝지어야 한다
    case unauthorized
    /// 짝짓기 코드가 틀렸다. 창은 아직 열려 있다(5번까지)
    case wrongCode
    /// 짝짓기 창이 닫혔다(안 열림, 2분 지남, 5번 틀림, 이미 씀). Mac에서 새 코드를 받아야 한다
    case pairingClosed
    /// manifest를 보냈는데 Mac에 원본이 덜 왔다. 빠진 상대 경로
    case missingFiles([String])
    case tooLarge
    /// Mac 앱이 이 iPhone 앱보다 오래돼 manifest 버전을 모른다
    case unsupportedVersion
    case http(status: Int, code: String, message: String)
    case invalidResponse
    case notPaired

    var errorDescription: String? {
        switch self {
        case .unauthorized: "Mac이 이 iPhone을 알아보지 못합니다. 설정에서 다시 짝지어 주세요"
        case .wrongCode: "코드가 맞지 않습니다. Mac에 보이는 6자리를 다시 넣어 주세요"
        case .pairingClosed: "Mac의 짝짓기 창이 닫혔습니다. Mac 설정에서 \"iPhone 연결\"을 다시 눌러 새 코드를 받아 주세요"
        case let .missingFiles(paths): "Mac에 원본 \(paths.count)장이 덜 왔습니다"
        case .tooLarge: "파일이 너무 커서 Mac이 받지 않습니다(원본 한 장 200MB까지)"
        case .unsupportedVersion: "Mac의 leaf가 오래돼 이 문서를 읽지 못합니다. Mac 앱을 업데이트해 주세요"
        case let .http(status, code, message): "Mac 응답 \(status) \(code): \(message)"
        case .invalidResponse: "Mac 응답을 읽지 못했습니다"
        case .notPaired: "짝지은 Mac이 없습니다"
        }
    }

    /// Mac 오류 본문 `{ error, message, missing? }`을 읽는다
    static func from(status: Int, body: Data) -> TransferError {
        struct Body: Decodable {
            var error: String
            var message: String?
            var missing: [String]?
        }
        guard let parsed = try? JSONDecoder().decode(Body.self, from: body) else {
            if status == 401 { return .unauthorized }
            return .http(status: status, code: "", message: String(data: body.prefix(300), encoding: .utf8) ?? "")
        }
        switch (status, parsed.error) {
        case (401, "wrong_code"): return .wrongCode
        case (401, _): return .unauthorized
        case (403, "pairing_closed"): return .pairingClosed
        case (409, "missing_files"): return .missingFiles(parsed.missing ?? [])
        case (413, _): return .tooLarge
        case (422, _): return .unsupportedVersion
        default: return .http(status: status, code: parsed.error, message: parsed.message ?? "")
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
    /// manifest 409(원본 덜 옴) 뒤 빠진 원본을 다시 보내는 횟수
    static let missingFileRetries = 2

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
        sign(&request, signer: signer, bodySha256: RequestSigner.sha256Hex(Data()))
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
        sign(&request, signer: signer, bodySha256: item.sha256)
        let (data, response) = try await session.upload(for: request, fromFile: item.fileURL)
        try check(response, data)
    }

    func putManifest(_ manifest: Data, documentID: UUID, signer: RequestSigner) async throws {
        let path = "/v1/documents/\(documentID.uuidString.lowercased())/manifest"
        var request = request("PUT", path)
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        sign(&request, signer: signer, bodySha256: RequestSigner.sha256Hex(manifest))
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
        // Mac이 원본이 덜 왔다고 하면(409) 빠진 것만 다시 보내고 manifest를 다시 보낸다
        var attempts = 0
        while true {
            do {
                try await putManifest(manifestData, documentID: documentID, signer: signer)
                break
            } catch let TransferError.missingFiles(paths) where attempts < Self.missingFileRetries {
                attempts += 1
                let resend = items.filter { paths.contains($0.relativePath) }
                guard !resend.isEmpty else { throw TransferError.missingFiles(paths) }
                for item in resend {
                    try Task.checkCancellation()
                    try await putFile(item, documentID: documentID, signer: signer)
                }
            }
        }
        await progress(1)
        return manifest
    }

    func request(_ method: String, _ path: String) -> URLRequest {
        var request = URLRequest(url: baseURL.appending(path: path))
        request.httpMethod = method
        request.timeoutInterval = 30
        return request
    }

    private func check(_ response: URLResponse, _ data: Data) throws {
        guard let http = response as? HTTPURLResponse else { throw TransferError.invalidResponse }
        guard (200 ..< 300).contains(http.statusCode) else {
            throw TransferError.from(status: http.statusCode, body: data)
        }
    }

    /// Mac은 요청 줄의 경로를 퍼센트 인코딩을 풀지 않고 그대로 서명 대상으로 쓴다. 보낼 URL에서 그 문자열을 꺼내 서명한다
    private func sign(_ request: inout URLRequest, signer: RequestSigner, bodySha256: String) {
        let path = request.url?.path(percentEncoded: true) ?? ""
        signer.sign(&request, path: path, bodySha256: bodySha256)
    }
}

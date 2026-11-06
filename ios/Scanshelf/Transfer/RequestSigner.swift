import CryptoKit
import Foundation

// Mac(crates/scanshelf-core/src/transfer.rs)과 같은 값을 만들어야 한다. fixtures/transfer-signature.json을
// 두 쪽 테스트가 같이 확인한다. 토큰은 짝짓기 때 한 번만 오가고 이후에는 서명만 보낸다.
// @see docs/transfer.md

struct RequestSigner: Sendable {
    static let deviceHeader = "X-Scanshelf-Device"
    static let timestampHeader = "X-Scanshelf-Timestamp"
    static let bodyHashHeader = "X-Scanshelf-Body-Sha256"
    static let signatureHeader = "X-Scanshelf-Signature"

    let deviceID: UUID
    let token: Data

    static func sha256Hex(_ data: Data) -> String {
        SHA256.hash(data: data).hexString
    }

    /// 사진은 수 MB라 한 번에 읽지 않고 1MB씩 해시한다
    static func sha256Hex(fileAt url: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return hasher.finalize().hexString
    }

    /// 서명할 문자열: `METHOD\nPATH\nTIMESTAMP\nBODY_SHA256_HEX`. PATH는 쿼리 없는 경로
    static func canonicalRequest(method: String, path: String, timestamp: Int64, bodySha256: String) -> String {
        "\(method)\n\(path)\n\(timestamp)\n\(bodySha256)"
    }

    static func signature(token: Data, method: String, path: String, timestamp: Int64, bodySha256: String) -> String {
        let message = Data(canonicalRequest(method: method, path: path, timestamp: timestamp, bodySha256: bodySha256).utf8)
        let code = HMAC<SHA256>.authenticationCode(for: message, using: SymmetricKey(data: token))
        return Data(code).hexString
    }

    func headers(method: String, path: String, bodySha256: String, now: Date = .now) -> [String: String] {
        let timestamp = Int64(now.timeIntervalSince1970)
        return [
            Self.deviceHeader: deviceID.uuidString.lowercased(),
            Self.timestampHeader: String(timestamp),
            Self.bodyHashHeader: bodySha256,
            Self.signatureHeader: Self.signature(
                token: token,
                method: method,
                path: path,
                timestamp: timestamp,
                bodySha256: bodySha256
            ),
        ]
    }

    func sign(_ request: inout URLRequest, path: String, bodySha256: String, now: Date = .now) {
        for (name, value) in headers(method: request.httpMethod ?? "GET", path: path, bodySha256: bodySha256, now: now) {
            request.setValue(value, forHTTPHeaderField: name)
        }
    }
}

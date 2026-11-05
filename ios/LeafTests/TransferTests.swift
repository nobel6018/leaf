import Foundation
import Testing
@testable import Leaf

struct RequestSignerTests {
    private struct Vector: Decodable {
        var tokenHex: String
        var method: String
        var path: String
        var timestamp: Int64
        var body: String
        var bodySha256: String
        var signature: String
    }

    // 저장소의 fixtures/transfer-signature.json. Rust 테스트(crates/leaf-core/tests/transfer.rs)와 같은 파일
    private func vector() throws -> Vector {
        let url = URL(filePath: #filePath)
            .deletingLastPathComponent()
            .appending(path: "../../fixtures/transfer-signature.json")
            .standardized
        return try JSONDecoder().decode(Vector.self, from: Data(contentsOf: url))
    }

    @Test func 공용_예시와_같은_서명을_만든다() throws {
        let v = try vector()
        let token = try #require(Data(hexString: v.tokenHex))

        #expect(RequestSigner.sha256Hex(Data(v.body.utf8)) == v.bodySha256)
        #expect(
            RequestSigner.signature(token: token, method: v.method, path: v.path, timestamp: v.timestamp, bodySha256: v.bodySha256)
                == v.signature
        )
    }

    @Test func 헤더에_기기_시각_해시_서명을_넣는다() throws {
        let v = try vector()
        let token = try #require(Data(hexString: v.tokenHex))
        let deviceID = try #require(UUID(uuidString: "3F2A0B1C-4D5E-4F60-8172-93A4B5C6D7E8"))
        let signer = RequestSigner(deviceID: deviceID, token: token)

        let headers = signer.headers(
            method: v.method,
            path: v.path,
            bodySha256: v.bodySha256,
            now: Date(timeIntervalSince1970: TimeInterval(v.timestamp))
        )

        #expect(headers["X-Leaf-Device"] == "3f2a0b1c-4d5e-4f60-8172-93a4b5c6d7e8")
        #expect(headers["X-Leaf-Timestamp"] == String(v.timestamp))
        #expect(headers["X-Leaf-Body-Sha256"] == v.bodySha256)
        #expect(headers["X-Leaf-Signature"] == v.signature)
    }

    @Test func 빈_본문은_빈_바이트의_해시를_쓴다() {
        #expect(RequestSigner.sha256Hex(Data()) == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    }

    @Test func 파일_해시는_한_번에_읽은_해시와_같다() throws {
        let url = FileManager.default.temporaryDirectory.appending(path: "leaf-hash-\(UUID().uuidString)")
        let data = Data((0 ..< 3_000_000).map { UInt8($0 % 251) })
        try data.write(to: url)
        defer { try? FileManager.default.removeItem(at: url) }

        #expect(try RequestSigner.sha256Hex(fileAt: url) == RequestSigner.sha256Hex(data))
    }

    @Test func 잘못된_hex는_읽지_않는다() {
        #expect(Data(hexString: "abc") == nil)
        #expect(Data(hexString: "zz") == nil)
        #expect(Data(hexString: "0aff") == Data([0x0a, 0xff]))
    }
}

struct UploadPlanTests {
    @Test func Mac에_같은_해시로_있는_파일은_건너뛴다() {
        let items = [
            UploadItem(relativePath: "raw/a.heic", fileURL: URL(filePath: "/tmp/a"), sha256: "aa"),
            UploadItem(relativePath: "raw/b.heic", fileURL: URL(filePath: "/tmp/b"), sha256: "bb"),
            UploadItem(relativePath: "raw/c.heic", fileURL: URL(filePath: "/tmp/c"), sha256: "cc"),
        ]

        let missing = UploadPlan.missing(items, remote: ["raw/a.heic": "aa", "raw/b.heic": "old"])

        #expect(missing.map(\.relativePath) == ["raw/b.heic", "raw/c.heic"])
    }
}

struct ICloudFolderTests {
    private let drive = URL(filePath: "/private/var/mobile/Library/Mobile Documents/com~apple~CloudDocs", directoryHint: .isDirectory)

    @Test func leaf_폴더를_고르면_그대로_쓴다() {
        let picked = drive.appending(path: "leaf", directoryHint: .isDirectory)

        #expect(ICloudFolderLayout.leafRoot(forPicked: picked)?.path == picked.path)
    }

    @Test func leaf_안의_폴더를_고르면_leaf로_올라간다() {
        let picked = drive.appending(path: "leaf/inbox/abc", directoryHint: .isDirectory)

        #expect(ICloudFolderLayout.leafRoot(forPicked: picked)?.lastPathComponent == "leaf")
        #expect(ICloudFolderLayout.leafRoot(forPicked: picked)?.deletingLastPathComponent().lastPathComponent == "com~apple~CloudDocs")
    }

    @Test func iCloud_Drive를_고르면_안의_leaf를_쓴다() {
        #expect(ICloudFolderLayout.leafRoot(forPicked: drive)?.path == drive.appending(path: "leaf").path)
    }

    @Test func iCloud_밖이나_다른_폴더는_거절한다() {
        #expect(ICloudFolderLayout.leafRoot(forPicked: drive.appending(path: "Documents")) == nil)
        #expect(ICloudFolderLayout.leafRoot(forPicked: URL(filePath: "/private/var/mobile/Containers/Shared/leaf")) == nil)
    }

    @Test func 원본을_복사하고_manifest를_마지막에_쓴다() throws {
        let base = FileManager.default.temporaryDirectory.appending(path: "leaf-icloud-\(UUID().uuidString)")
        let store = DocumentStore(root: base.appending(path: "library"))
        let leafRoot = base.appending(path: "com~apple~CloudDocs/leaf")
        var manifest = try store.create(name: "예시_1", kind: .pamphlet, session: "예시")
        manifest = try store.addPage(to: manifest, photo: Data([9, 8, 7]), quad: nil, layout: .single, capturedAt: .now)
        defer { try? FileManager.default.removeItem(at: base) }

        let exported = try ICloudExporter.export(documentID: manifest.id, store: store, leafRoot: leafRoot)

        let inbox = ICloudFolderLayout.inbox(in: leafRoot, documentID: manifest.id)
        #expect(exported == manifest)
        #expect(try Data(contentsOf: inbox.appending(path: manifest.pages[0].raw)) == Data([9, 8, 7]))
        let copied = try Manifest.decoder().decode(Manifest.self, from: Data(contentsOf: inbox.appending(path: "manifest.json")))
        #expect(copied == manifest)
        #expect(!ICloudExporter.isImportedByMac(documentID: manifest.id, leafRoot: leafRoot))

        try FileManager.default.removeItem(at: inbox)
        #expect(ICloudExporter.isImportedByMac(documentID: manifest.id, leafRoot: leafRoot))
    }

    @Test func inbox가_안_보이면_받았다고_보지_않는다() {
        let missingRoot = FileManager.default.temporaryDirectory.appending(path: "leaf-none-\(UUID().uuidString)")

        #expect(!ICloudExporter.isImportedByMac(documentID: UUID(), leafRoot: missingRoot))
    }
}

// Mac 서버(src-tauri/src/transfer/server.rs)의 응답 모양과 맞춘다
struct TransferClientTests {
    private let client = TransferClient(baseURL: URL(string: "http://192.168.0.3:52100")!)

    @Test func 서명하는_경로는_요청_줄에_실리는_경로와_같다() {
        let path = "/v1/documents/7d1c2a4e-0f3b-4a8e-9c61-2b5d8e3f1a90/files/raw/1e9f4b7a-3c2d-4e5f-8a6b-7c8d9e0f1a2b.heic"

        let request = client.request("PUT", path)

        #expect(request.url?.path(percentEncoded: true) == path)
        #expect(request.url?.absoluteString == "http://192.168.0.3:52100" + path)
    }

    @Test func 짝짓기_코드_틀림과_창_닫힘을_구별한다() {
        let wrong = Data(#"{"error":"wrong_code","message":"코드가 맞지 않는다"}"#.utf8)
        let closed = Data(#"{"error":"pairing_closed","message":"짝짓기 창이 열려 있지 않다"}"#.utf8)

        #expect(TransferError.from(status: 401, body: wrong) == .wrongCode)
        #expect(TransferError.from(status: 403, body: closed) == .pairingClosed)
        #expect(TransferError.wrongCode.localizedDescription != TransferError.pairingClosed.localizedDescription)
    }

    @Test func 원본이_덜_왔다는_409에서_빠진_경로를_읽는다() {
        let body = Data(#"{"error":"missing_files","message":"원본이 덜 왔다","missing":["raw/a.heic","raw/b.heic"]}"#.utf8)

        #expect(TransferError.from(status: 409, body: body) == .missingFiles(["raw/a.heic", "raw/b.heic"]))
    }

    @Test func 그_밖의_오류_코드를_읽는다() {
        let unauthorized = Data(#"{"error":"unauthorized","message":"서명이 맞지 않는다"}"#.utf8)
        let large = Data(#"{"error":"too_large","message":"본문이 너무 크다"}"#.utf8)
        let version = Data(#"{"error":"unsupported_version","message":"x"}"#.utf8)
        let bad = Data(#"{"error":"bad_request","message":"raw/ 밖 경로"}"#.utf8)

        #expect(TransferError.from(status: 401, body: unauthorized) == .unauthorized)
        #expect(TransferError.from(status: 413, body: large) == .tooLarge)
        #expect(TransferError.from(status: 422, body: version) == .unsupportedVersion)
        #expect(TransferError.from(status: 400, body: bad) == .http(status: 400, code: "bad_request", message: "raw/ 밖 경로"))
        #expect(TransferError.from(status: 401, body: Data("not json".utf8)) == .unauthorized)
    }
}

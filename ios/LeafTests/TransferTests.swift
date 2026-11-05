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

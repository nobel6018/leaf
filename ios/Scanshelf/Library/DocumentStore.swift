import CryptoKit
import Darwin
import Foundation

// 앱 Documents/library/<문서 id>/ 아래에 manifest.json과 raw/<페이지 id>.heic를 쌓는다.
// 폴더 구조와 규칙은 Mac(crates/scanshelf-core)과 같다.
// @see docs/document-format.md

enum DocumentStoreError: Error, LocalizedError {
    case unsupportedVersion(Int)
    case invalidPage(UUID, String)
    case pageNotFound(UUID)

    var errorDescription: String? {
        switch self {
        case let .unsupportedVersion(version):
            "지원하지 않는 schemaVersion \(version) (이 앱은 \(ManifestSchema.version)까지)"
        case let .invalidPage(id, reason):
            "페이지 \(id): \(reason)"
        case let .pageNotFound(id):
            "페이지 \(id)를 찾지 못했습니다"
        }
    }
}

extension Manifest {
    /// Rust `Manifest::validate`와 같은 규칙. 저장 전에 확인해 Mac이 거부할 문서를 만들지 않는다
    func validate() throws {
        guard schemaVersion <= ManifestSchema.version else {
            throw DocumentStoreError.unsupportedVersion(schemaVersion)
        }
        for page in pages {
            let parts = page.raw.split(separator: "/", omittingEmptySubsequences: false)
            let escapes = page.raw.hasPrefix("/") || parts.contains { $0.isEmpty || $0 == "." || $0 == ".." }
            if page.raw.isEmpty || escapes {
                throw DocumentStoreError.invalidPage(page.id, "raw 경로는 문서 폴더 안의 상대 경로여야 한다")
            }
            if let quad = page.quad, !quad.points.allSatisfy({ (0 ... 1).contains($0.x) && (0 ... 1).contains($0.y) }) {
                throw DocumentStoreError.invalidPage(page.id, "꼭짓점은 0~1 정규화 좌표여야 한다")
            }
        }
    }

    /// 보낸 뒤 페이지를 더 찍었는지 알아보는 지문. 저장되는 바이트 그대로의 해시다
    func fingerprint() throws -> String {
        let data = try Manifest.encoder().encode(self)
        return SHA256.hash(data: data).hexString
    }

    static func rawPath(for pageID: UUID) -> String {
        "raw/\(pageID.uuidString.lowercased()).heic"
    }
}

struct DocumentStore: Sendable {
    static let rawDirectory = "raw"

    let root: URL

    /// 앱 Documents/library. 파일 앱에서 보이지 않게 Info.plist에 공유 설정을 켜지 않는다
    static func appDefault() -> DocumentStore {
        let documents = URL.documentsDirectory
        return DocumentStore(root: documents.appending(path: "library", directoryHint: .isDirectory))
    }

    func directory(of documentID: UUID) -> URL {
        root.appending(path: documentID.uuidString.lowercased(), directoryHint: .isDirectory)
    }

    func manifestURL(of documentID: UUID) -> URL {
        directory(of: documentID).appending(path: ManifestSchema.fileName)
    }

    func fileURL(of documentID: UUID, relativePath: String) -> URL {
        directory(of: documentID).appending(path: relativePath)
    }

    /// 읽지 못하는 문서(새 버전 앱이 만든 것, 깨진 것)는 건너뛴다. 최근 문서가 앞에 온다
    func list() -> [Manifest] {
        let entries = (try? FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)) ?? []
        return entries
            .compactMap { UUID(uuidString: $0.lastPathComponent) }
            .compactMap { try? load($0) }
            .sorted { $0.createdAt > $1.createdAt }
    }

    func load(_ documentID: UUID) throws -> Manifest {
        let data = try Data(contentsOf: manifestURL(of: documentID))
        let manifest = try Manifest.decoder().decode(Manifest.self, from: data)
        try manifest.validate()
        return manifest
    }

    /// 임시 파일에 쓴 뒤 rename(2)로 바꾼다. 전송이나 iCloud 복사 중에 반쯤 쓴 manifest를 읽지 않게 한다
    func save(_ manifest: Manifest) throws {
        try manifest.validate()
        let directory = directory(of: manifest.id)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try Self.writeAtomically(
            try Manifest.encoder().encode(manifest),
            to: directory.appending(path: ManifestSchema.fileName)
        )
    }

    func create(name: String, kind: DocumentKind, session: String?, now: Date = .now) throws -> Manifest {
        let manifest = Manifest(
            schemaVersion: ManifestSchema.version,
            id: UUID(),
            name: name,
            kind: kind,
            session: session,
            createdAt: now.millisecondPrecision,
            pages: []
        )
        try FileManager.default.createDirectory(
            at: directory(of: manifest.id).appending(path: Self.rawDirectory),
            withIntermediateDirectories: true
        )
        try save(manifest)
        return manifest
    }

    /// 원본을 먼저 쓰고 manifest를 나중에 쓴다. 중간에 앱이 죽어도 manifest가 없는 사진을 가리키지 않는다.
    /// `replacing`을 주면 그 자리에 다시 찍은 것으로 넣고 예전 원본을 지운다
    func addPage(
        to manifest: Manifest,
        photo: Data,
        quad: Quad?,
        layout: PageLayout,
        capturedAt: Date,
        replacing oldPageID: UUID? = nil
    ) throws -> Manifest {
        var updated = manifest
        let pageID = UUID()
        let newPage = Page(id: pageID, raw: Manifest.rawPath(for: pageID), capturedAt: capturedAt.millisecondPrecision, layout: layout, quad: quad)

        let rawURL = fileURL(of: manifest.id, relativePath: newPage.raw)
        try FileManager.default.createDirectory(at: rawURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Self.writeAtomically(photo, to: rawURL)

        if let oldPageID, let index = updated.pages.firstIndex(where: { $0.id == oldPageID }) {
            let old = updated.pages[index]
            updated.pages[index] = newPage
            try save(updated)
            try? FileManager.default.removeItem(at: fileURL(of: manifest.id, relativePath: old.raw))
        } else {
            updated.pages.append(newPage)
            try save(updated)
        }
        return updated
    }

    func deletePage(_ pageID: UUID, from manifest: Manifest) throws -> Manifest {
        guard let index = manifest.pages.firstIndex(where: { $0.id == pageID }) else {
            throw DocumentStoreError.pageNotFound(pageID)
        }
        var updated = manifest
        let removed = updated.pages.remove(at: index)
        try save(updated)
        try? FileManager.default.removeItem(at: fileURL(of: manifest.id, relativePath: removed.raw))
        return updated
    }

    func updateMetadata(of manifest: Manifest, name: String, kind: DocumentKind, session: String?) throws -> Manifest {
        var updated = manifest
        updated.name = name
        updated.kind = kind
        updated.session = session
        try save(updated)
        return updated
    }

    func deleteDocument(_ documentID: UUID) throws {
        let directory = directory(of: documentID)
        if FileManager.default.fileExists(atPath: directory.path) {
            try FileManager.default.removeItem(at: directory)
        }
    }

    /// 같은 폴더의 `.<이름>.tmp`에 쓰고 rename(2)한다. 같은 볼륨 안 rename은 원자적이다
    static func writeAtomically(_ data: Data, to url: URL) throws {
        let temp = url.deletingLastPathComponent().appending(path: ".\(url.lastPathComponent).tmp")
        try data.write(to: temp)
        guard rename(temp.path, url.path) == 0 else {
            let code = errno
            try? FileManager.default.removeItem(at: temp)
            throw CocoaError(.fileWriteUnknown, userInfo: [NSFilePathErrorKey: url.path, NSUnderlyingErrorKey: POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO)])
        }
    }
}

extension Date {
    /// manifest는 밀리초까지 쓴다. 메모리 값도 맞춰 두어야 저장했다 읽은 값과 같다
    var millisecondPrecision: Date {
        Date(timeIntervalSince1970: (timeIntervalSince1970 * 1000).rounded() / 1000)
    }
}

extension Digest {
    var hexString: String {
        map { String(format: "%02x", $0) }.joined()
    }
}

extension Data {
    var hexString: String {
        map { String(format: "%02x", $0) }.joined()
    }

    init?(hexString: String) {
        guard hexString.count.isMultiple(of: 2) else { return nil }
        var bytes: [UInt8] = []
        bytes.reserveCapacity(hexString.count / 2)
        var index = hexString.startIndex
        while index < hexString.endIndex {
            let next = hexString.index(index, offsetBy: 2)
            guard let byte = UInt8(hexString[index ..< next], radix: 16) else { return nil }
            bytes.append(byte)
            index = next
        }
        self.init(bytes)
    }
}

import Foundation

// Mac이 같은 Wi-Fi에 없을 때 쓰는 보조 경로. 무료 Apple ID 서명이라 CloudKit과 앱 전용 iCloud 폴더를 못 쓴다.
// 파일 앱 폴더 선택기로 iCloud Drive/scanshelf를 한 번 고르고 보안 범위 북마크로 기억한다.
// scanshelf/inbox/<문서 id>/에 원본을 먼저, manifest를 마지막에 복사한다. Mac은 가져간 뒤 그 폴더를 지운다.
// @see docs/transfer.md

enum ICloudFolderError: Error, LocalizedError {
    case notICloudFolder
    case accessDenied
    case notChosen

    var errorDescription: String? {
        switch self {
        case .notICloudFolder: "iCloud Drive의 scanshelf 폴더(또는 iCloud Drive 자체)를 골라 주세요"
        case .accessDenied: "iCloud 폴더에 접근하지 못했습니다. 설정에서 폴더를 다시 골라 주세요"
        case .notChosen: "iCloud 폴더를 아직 고르지 않았습니다"
        }
    }
}

enum ICloudFolderLayout {
    static let rootName = "scanshelf"
    static let inboxName = "inbox"
    private static let iCloudDriveName = "com~apple~CloudDocs"

    /// 고른 폴더에서 scanshelf 루트를 찾는다. scanshelf 안의 inbox나 문서 폴더를 고르는 실수를 받아 주려고 위로 올라가며 찾는다.
    /// iCloud Drive 자체를 고르면 그 안의 scanshelf를 쓴다. 그 밖(이 iPhone 안 등)은 Mac이 볼 수 없으니 거절한다
    static func scanshelfRoot(forPicked picked: URL) -> URL? {
        let standardized = picked.standardizedFileURL
        var current = standardized
        while current.pathComponents.count > 1 {
            if current.lastPathComponent == iCloudDriveName { break }
            if current.lastPathComponent == rootName, isInsideICloudDrive(current) { return current }
            current = current.deletingLastPathComponent()
        }
        if standardized.lastPathComponent == iCloudDriveName {
            return standardized.appending(path: rootName, directoryHint: .isDirectory)
        }
        return nil
    }

    static func inbox(in root: URL, documentID: UUID) -> URL {
        root.appending(path: inboxName, directoryHint: .isDirectory)
            .appending(path: documentID.uuidString.lowercased(), directoryHint: .isDirectory)
    }

    private static func isInsideICloudDrive(_ url: URL) -> Bool {
        url.pathComponents.contains(iCloudDriveName)
    }
}

@MainActor
@Observable
final class ICloudFolder {
    private static let bookmarkKey = "scanshelf.icloudFolderBookmark"

    /// 고른 폴더(보안 범위가 걸린 URL). 앱이 켜져 있는 동안 접근을 열어 둔다
    private(set) var pickedFolder: URL?
    private(set) var failure: String?

    var scanshelfRoot: URL? {
        pickedFolder.flatMap(ICloudFolderLayout.scanshelfRoot(forPicked:))
    }

    var isChosen: Bool {
        UserDefaults.standard.data(forKey: Self.bookmarkKey) != nil
    }

    init() {
        _ = try? resolve()
    }

    /// `.fileImporter`가 돌려준 폴더를 기억한다
    func choose(_ url: URL) throws {
        guard ICloudFolderLayout.scanshelfRoot(forPicked: url) != nil else { throw ICloudFolderError.notICloudFolder }
        guard url.startAccessingSecurityScopedResource() else { throw ICloudFolderError.accessDenied }
        do {
            let bookmark = try url.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil)
            UserDefaults.standard.set(bookmark, forKey: Self.bookmarkKey)
            pickedFolder?.stopAccessingSecurityScopedResource()
            pickedFolder = url
            failure = nil
        } catch {
            url.stopAccessingSecurityScopedResource()
            throw error
        }
    }

    func forget() {
        pickedFolder?.stopAccessingSecurityScopedResource()
        pickedFolder = nil
        UserDefaults.standard.removeObject(forKey: Self.bookmarkKey)
    }

    @discardableResult
    func resolve() throws -> URL {
        if let pickedFolder, let root = ICloudFolderLayout.scanshelfRoot(forPicked: pickedFolder) { return root }
        guard let bookmark = UserDefaults.standard.data(forKey: Self.bookmarkKey) else { throw ICloudFolderError.notChosen }
        var stale = false
        let url = try URL(resolvingBookmarkData: bookmark, options: [], relativeTo: nil, bookmarkDataIsStale: &stale)
        guard url.startAccessingSecurityScopedResource() else {
            failure = ICloudFolderError.accessDenied.localizedDescription
            throw ICloudFolderError.accessDenied
        }
        // 폴더를 옮기거나 이름을 바꾸면 오래된 북마크가 된다. 새로 만들어 둔다
        if stale, let fresh = try? url.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil) {
            UserDefaults.standard.set(fresh, forKey: Self.bookmarkKey)
        }
        pickedFolder = url
        guard let root = ICloudFolderLayout.scanshelfRoot(forPicked: url) else { throw ICloudFolderError.notICloudFolder }
        return root
    }
}

/// 파일 복사는 무거우니 메인 스레드 밖에서 부른다
enum ICloudExporter {
    /// 원본을 먼저, manifest를 마지막에 복사한다. 이미 같은 크기로 있는 원본은 건너뛰어 이어 복사가 된다
    static func export(documentID: UUID, store: DocumentStore, scanshelfRoot: URL) throws -> Manifest {
        let manifestData = try Data(contentsOf: store.manifestURL(of: documentID))
        let manifest = try Manifest.decoder().decode(Manifest.self, from: manifestData)
        try manifest.validate()

        let target = ICloudFolderLayout.inbox(in: scanshelfRoot, documentID: documentID)
        try FileManager.default.createDirectory(at: target, withIntermediateDirectories: true)

        for page in manifest.pages {
            let source = store.fileURL(of: documentID, relativePath: page.raw)
            let destination = target.appending(path: page.raw)
            if sameSize(source, destination) { continue }
            try FileManager.default.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
            try coordinatedWrite(to: destination) { url in
                try DocumentStore.writeAtomically(try Data(contentsOf: source), to: url)
            }
        }
        try coordinatedWrite(to: target.appending(path: ManifestSchema.fileName)) { url in
            try DocumentStore.writeAtomically(manifestData, to: url)
        }
        return manifest
    }

    /// Mac이 가져가면 inbox/<문서 id>를 지운다. 그 삭제가 이 iPhone에 내려오면 받은 것으로 확인한다
    /// inbox 자체가 안 보이면 폴더 접근이 끊긴 것이라 확인하지 않는다(지운 것으로 오해해 원본을 지우면 안 된다)
    static func isImportedByMac(documentID: UUID, scanshelfRoot: URL) -> Bool {
        let inbox = scanshelfRoot.appending(path: ICloudFolderLayout.inboxName, directoryHint: .isDirectory)
        guard FileManager.default.fileExists(atPath: inbox.path) else { return false }
        return !FileManager.default.fileExists(atPath: ICloudFolderLayout.inbox(in: scanshelfRoot, documentID: documentID).path)
    }

    private static func sameSize(_ a: URL, _ b: URL) -> Bool {
        guard let sizeA = try? a.resourceValues(forKeys: [.fileSizeKey]).fileSize,
              let sizeB = try? b.resourceValues(forKeys: [.fileSizeKey]).fileSize
        else { return false }
        return sizeA == sizeB
    }

    private static func coordinatedWrite(to url: URL, _ write: (URL) throws -> Void) throws {
        var coordinationError: NSError?
        var writeError: Error?
        NSFileCoordinator(filePresenter: nil).coordinate(writingItemAt: url, options: .forReplacing, error: &coordinationError) { target in
            do { try write(target) } catch { writeError = error }
        }
        if let coordinationError { throw coordinationError }
        if let writeError { throw writeError }
    }
}

import Foundation
import Testing
@testable import Leaf

struct DocumentStoreTests {
    private let store: DocumentStore

    init() throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "leaf-tests-\(UUID().uuidString)")
        store = DocumentStore(root: root)
    }

    private let quad = Quad(
        topLeft: .init(x: 0.08, y: 0.06),
        topRight: .init(x: 0.93, y: 0.05),
        bottomRight: .init(x: 0.95, y: 0.94),
        bottomLeft: .init(x: 0.06, y: 0.95)
    )

    @Test func 문서를_만들면_폴더와_manifest가_생긴다() throws {
        let manifest = try store.create(name: "예시_1", kind: .pamphlet, session: "예시")

        let folder = store.directory(of: manifest.id)
        #expect(folder.lastPathComponent == manifest.id.uuidString.lowercased())
        #expect(FileManager.default.fileExists(atPath: folder.appending(path: "manifest.json").path))
        #expect(FileManager.default.fileExists(atPath: folder.appending(path: "raw").path))
        #expect(try store.load(manifest.id) == manifest)
    }

    @Test func 페이지를_더하면_원본이_raw_아래에_생기고_임시_파일은_남지_않는다() throws {
        let manifest = try store.create(name: "예시_1", kind: .book, session: nil)

        let updated = try store.addPage(to: manifest, photo: Data([1, 2, 3]), quad: quad, layout: .spread, capturedAt: .now)

        let page = try #require(updated.pages.first)
        #expect(page.raw == "raw/\(page.id.uuidString.lowercased()).heic")
        #expect(try Data(contentsOf: store.fileURL(of: manifest.id, relativePath: page.raw)) == Data([1, 2, 3]))
        let names = try FileManager.default.contentsOfDirectory(atPath: store.directory(of: manifest.id).path)
        #expect(!names.contains { $0.hasSuffix(".tmp") })
        #expect(try store.load(manifest.id) == updated)
    }

    @Test func 다시_찍으면_같은_자리에_들어가고_예전_원본은_지운다() throws {
        var manifest = try store.create(name: "예시_1", kind: .document, session: nil)
        manifest = try store.addPage(to: manifest, photo: Data([1]), quad: nil, layout: .single, capturedAt: .now)
        manifest = try store.addPage(to: manifest, photo: Data([2]), quad: nil, layout: .single, capturedAt: .now)
        let first = manifest.pages[0]

        manifest = try store.addPage(to: manifest, photo: Data([3]), quad: quad, layout: .single, capturedAt: .now, replacing: first.id)

        #expect(manifest.pages.count == 2)
        #expect(manifest.pages[0].id != first.id)
        #expect(manifest.pages[0].quad == quad)
        #expect(!FileManager.default.fileExists(atPath: store.fileURL(of: manifest.id, relativePath: first.raw).path))
    }

    @Test func 페이지를_지우면_원본도_지운다() throws {
        var manifest = try store.create(name: "예시_1", kind: .document, session: nil)
        manifest = try store.addPage(to: manifest, photo: Data([1]), quad: nil, layout: .single, capturedAt: .now)
        let page = manifest.pages[0]

        manifest = try store.deletePage(page.id, from: manifest)

        #expect(manifest.pages.isEmpty)
        #expect(!FileManager.default.fileExists(atPath: store.fileURL(of: manifest.id, relativePath: page.raw).path))
    }

    @Test func 목록은_최근_문서가_앞이고_읽지_못하는_문서는_건너뛴다() throws {
        let older = try store.create(name: "예시_1", kind: .pamphlet, session: nil, now: Date(timeIntervalSince1970: 1_000))
        let newer = try store.create(name: "예시_2", kind: .pamphlet, session: nil, now: Date(timeIntervalSince1970: 2_000))
        let broken = store.directory(of: UUID())
        try FileManager.default.createDirectory(at: broken, withIntermediateDirectories: true)
        try Data("{}".utf8).write(to: broken.appending(path: "manifest.json"))

        let ids = store.list().map(\.id)

        #expect(ids == [newer.id, older.id])
    }

    @Test func 문서_폴더_밖을_가리키는_raw는_저장하지_않는다() throws {
        var manifest = try store.create(name: "예시_1", kind: .pamphlet, session: nil)
        manifest.pages.append(Page(id: UUID(), raw: "../../etc/x.heic", capturedAt: .now, layout: .single, quad: nil))

        #expect(throws: DocumentStoreError.self) { try store.save(manifest) }
    }

    @Test func 지문은_페이지가_바뀌면_달라진다() throws {
        let manifest = try store.create(name: "예시_1", kind: .pamphlet, session: nil)
        let before = try manifest.fingerprint()

        let updated = try store.addPage(to: manifest, photo: Data([1]), quad: nil, layout: .single, capturedAt: .now)

        #expect(try manifest.fingerprint() == before)
        #expect(try updated.fingerprint() != before)
    }
}

struct DocumentNamingTests {
    @Test func 세션_이름_뒤에_다음_번호를_붙인다() {
        let name = DocumentNaming.defaultName(
            session: "2026 예시박람회",
            existingNames: ["2026 예시박람회_1", "2026 예시박람회_3", "2026 예시박람회_예시로보틱스", "다른박람회_9"]
        )

        #expect(name == "2026 예시박람회_4")
    }

    @Test func 세션이_없으면_문서_번호로_짓는다() {
        #expect(DocumentNaming.defaultName(session: "  ", existingNames: []) == "문서_1")
    }
}

struct RetentionPolicyTests {
    private let now = Date(timeIntervalSince1970: 1_800_000_000)
    private let day: TimeInterval = 24 * 60 * 60

    @Test func 확인됐고_바뀌지_않았고_기한이_지난_문서만_지운다() {
        let old = UUID()
        let recent = UUID()
        let changed = UUID()
        let unconfirmed = UUID()
        let records: [UUID: SendRecord] = [
            old: .init(method: .wifi, sentAt: now - 8 * day, manifestFingerprint: "a", confirmed: true),
            recent: .init(method: .wifi, sentAt: now - 2 * day, manifestFingerprint: "b", confirmed: true),
            changed: .init(method: .wifi, sentAt: now - 30 * day, manifestFingerprint: "c", confirmed: true),
            unconfirmed: .init(method: .icloud, sentAt: now - 30 * day, manifestFingerprint: "d", confirmed: false),
        ]
        let fingerprints = [old: "a", recent: "b", changed: "c2", unconfirmed: "d"]

        let ids = RetentionPolicy.documentsToDelete(records: records, fingerprints: fingerprints, days: 7, now: now)

        #expect(ids == [old])
    }

    @Test func 기간이_없으면_지우지_않는다() {
        let id = UUID()
        let records = [id: SendRecord(method: .wifi, sentAt: now - 365 * day, manifestFingerprint: "a", confirmed: true)]

        #expect(RetentionPolicy.documentsToDelete(records: records, fingerprints: [id: "a"], days: nil, now: now).isEmpty)
    }

    @Test func 보낸_뒤_바뀌면_상태가_바뀜으로_보인다() {
        let record = SendRecord(method: .icloud, sentAt: now, manifestFingerprint: "a", confirmed: false)

        #expect(SendStatus.of(record: record, currentFingerprint: "a") == .awaitingMac(.icloud))
        #expect(SendStatus.of(record: record, currentFingerprint: "b") == .changedSinceSent)
        #expect(SendStatus.of(record: nil, currentFingerprint: "a") == .notSent)
    }
}

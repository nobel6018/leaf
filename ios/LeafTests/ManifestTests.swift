import Foundation
import Testing
@testable import Leaf

struct ManifestTests {
    // 저장소의 fixtures/manifest-v1.json. Rust 테스트(crates/leaf-core/tests/manifest.rs)와 같은 파일
    private func fixture() throws -> Data {
        let url = URL(filePath: #filePath)
            .deletingLastPathComponent()
            .appending(path: "../../fixtures/manifest-v1.json")
            .standardized
        return try Data(contentsOf: url)
    }

    @Test func 공용_예시_manifest를_읽는다() throws {
        let manifest = try Manifest.decoder().decode(Manifest.self, from: fixture())

        #expect(manifest.kind == .pamphlet)
        #expect(manifest.pages.count == 2)
        #expect(manifest.pages[1].layout == .spread)
        #expect(manifest.pages[1].quad == nil)
    }

    @Test func 저장했다_다시_읽으면_같다() throws {
        let manifest = try Manifest.decoder().decode(Manifest.self, from: fixture())

        let encoded = try Manifest.encoder().encode(manifest)

        #expect(try Manifest.decoder().decode(Manifest.self, from: encoded) == manifest)
    }
}

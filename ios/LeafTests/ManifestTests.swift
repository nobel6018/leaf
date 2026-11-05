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

    // 날짜 형식만 다르고(Swift는 항상 .000Z까지 쓴다) 나머지 키와 값은 예시와 똑같아야 Mac이 그대로 읽는다
    @Test func 인코더_출력은_공용_예시와_같은_JSON이다() throws {
        let original = try fixture()
        let manifest = try Manifest.decoder().decode(Manifest.self, from: original)

        let encoded = try Manifest.encoder().encode(manifest)

        let expected = try normalized(JSONSerialization.jsonObject(with: original))
        let actual = try normalized(JSONSerialization.jsonObject(with: encoded))
        #expect(actual as? NSDictionary == expected as? NSDictionary)
    }

    @Test func 날짜는_밀리초와_Z로_쓴다() throws {
        let manifest = try Manifest.decoder().decode(Manifest.self, from: fixture())

        let text = try #require(String(data: Manifest.encoder().encode(manifest), encoding: .utf8))

        #expect(text.contains("\"createdAt\" : \"2026-11-05T01:20:00.000Z\""))
        #expect(text.contains("\"capturedAt\" : \"2026-11-05T01:20:03.412Z\""))
        #expect(text.contains("\"capturedAt\" : \"2026-11-05T01:20:09.020Z\""))
    }

    @Test func 저장을_여러_번_해도_밀리초가_밀리지_않는다() throws {
        var manifest = try Manifest.decoder().decode(Manifest.self, from: fixture())

        for _ in 0 ..< 5 {
            manifest = try Manifest.decoder().decode(Manifest.self, from: Manifest.encoder().encode(manifest))
        }

        let text = try #require(String(data: Manifest.encoder().encode(manifest), encoding: .utf8))
        #expect(text.contains("2026-11-05T01:20:03.412Z"))
    }

    @Test func id는_소문자로_쓰고_없는_값은_키를_빼고_쓴다() throws {
        var manifest = try Manifest.decoder().decode(Manifest.self, from: fixture())
        manifest.session = nil

        let text = try #require(String(data: Manifest.encoder().encode(manifest), encoding: .utf8))

        #expect(text.contains("7d1c2a4e-0f3b-4a8e-9c61-2b5d8e3f1a90"))
        #expect(!text.contains("session"))
        #expect(!text.contains("null"))
    }

    /// 날짜 문자열을 밀리초 정수로 바꿔 비교한다
    private func normalized(_ value: Any) throws -> Any {
        if let dictionary = value as? [String: Any] {
            var result: [String: Any] = [:]
            for (key, child) in dictionary {
                if key == "createdAt" || key == "capturedAt", let text = child as? String {
                    let style = Date.ISO8601FormatStyle(includingFractionalSeconds: text.contains("."))
                    result[key] = Int64((try style.parse(text).timeIntervalSince1970 * 1000).rounded())
                } else {
                    result[key] = try normalized(child)
                }
            }
            return result
        }
        if let array = value as? [Any] {
            return try array.map(normalized)
        }
        return value
    }

    @Test func 세_면_접지_예시를_읽는다() throws {
        let url = URL(filePath: #filePath)
            .deletingLastPathComponent()
            .appending(path: "../../fixtures/manifest-v1-trifold.json")
            .standardized
        let manifest = try Manifest.decoder().decode(Manifest.self, from: Data(contentsOf: url))

        #expect(manifest.pages[0].layout == .trifold)
    }
}

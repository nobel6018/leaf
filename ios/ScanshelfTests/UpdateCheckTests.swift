import Foundation
import Testing
@testable import Scanshelf

struct UpdateCheckTests {
    @Test func 자리마다_숫자로_비교한다() {
        #expect(UpdateCheck.isNewer("0.10.0", than: "0.9.1"))
        #expect(UpdateCheck.isNewer("1.0.0", than: "0.99.99"))
        #expect(UpdateCheck.isNewer("0.1.1", than: "0.1.0"))
        #expect(!UpdateCheck.isNewer("0.9.1", than: "0.10.0"))
    }

    @Test func 같은_버전은_새_버전이_아니다() {
        #expect(!UpdateCheck.isNewer("0.2.0", than: "0.2.0"))
        #expect(!UpdateCheck.isNewer("0.2", than: "0.2.0"))
        #expect(!UpdateCheck.isNewer("0.2.0", than: "0.2"))
    }

    @Test func 앞의_v와_숫자_뒤_글자는_무시한다() {
        #expect(UpdateCheck.isNewer("v0.3.0", than: "0.2.9"))
        #expect(!UpdateCheck.isNewer("v0.2.0", than: "0.2.0"))
        #expect(!UpdateCheck.isNewer("0.2.0-beta.1", than: "0.2.0"))
        #expect(UpdateCheck.isNewer("0.2.1", than: "0.2.0-beta"))
    }

    @Test func 짧은_버전은_빈_자리를_0으로_본다() {
        #expect(UpdateCheck.isNewer("0.2.0.1", than: "0.2"))
        #expect(!UpdateCheck.isNewer("0.2", than: "0.2.0.1"))
    }

    private func release(_ tag: String, assets: [String]) throws -> GitHubRelease {
        let json = """
        {
          "tag_name": "\(tag)",
          "html_url": "https://github.com/nobel6018/scanshelf/releases/tag/\(tag)",
          "draft": false,
          "assets": [\(assets.map { "{\"name\": \"\($0)\", \"size\": 1}" }.joined(separator: ","))]
        }
        """
        return try JSONDecoder().decode(GitHubRelease.self, from: Data(json.utf8))
    }

    @Test func iPhone_파일이_붙은_새_버전만_알린다() throws {
        let ready = try release("v0.2.0", assets: ["scanshelf_0.2.0_aarch64.dmg", "scanshelf-ios.ipa", "apps.json"])
        let macOnly = try release("v0.2.0", assets: ["scanshelf_0.2.0_aarch64.dmg", "latest.json"])
        let same = try release("v0.1.0", assets: ["scanshelf-ios.ipa"])

        let update = UpdateCheck.available(in: ready, current: "0.1.0")

        #expect(update?.version == "0.2.0")
        #expect(update?.releaseURL.absoluteString == "https://github.com/nobel6018/scanshelf/releases/tag/v0.2.0")
        #expect(UpdateCheck.available(in: macOnly, current: "0.1.0") == nil)
        #expect(UpdateCheck.available(in: same, current: "0.1.0") == nil)
    }

    @Test func 하루에_한_번만_묻는다() {
        let checked = Date(timeIntervalSince1970: 1_000_000)

        #expect(UpdateCheck.shouldCheck(lastCheckedAt: nil, now: checked))
        #expect(!UpdateCheck.shouldCheck(lastCheckedAt: checked, now: checked.addingTimeInterval(23 * 3600)))
        #expect(UpdateCheck.shouldCheck(lastCheckedAt: checked, now: checked.addingTimeInterval(24 * 3600)))
        // 시계가 뒤로 갔으면 다시 묻는다
        #expect(UpdateCheck.shouldCheck(lastCheckedAt: checked, now: checked.addingTimeInterval(-60)))
    }

    @Test func 닫은_버전과_이미_깔린_버전은_알리지_않는다() {
        let cached = AvailableUpdate(version: "0.2.0", releaseURL: URL(string: "https://github.com/nobel6018/scanshelf/releases/tag/v0.2.0")!)

        #expect(UpdateCheck.banner(cached: cached, current: "0.1.0", dismissed: nil) == cached)
        #expect(UpdateCheck.banner(cached: cached, current: "0.1.0", dismissed: "0.1.5") == cached)
        #expect(UpdateCheck.banner(cached: cached, current: "0.1.0", dismissed: "0.2.0") == nil)
        #expect(UpdateCheck.banner(cached: cached, current: "0.2.0", dismissed: nil) == nil)
        #expect(UpdateCheck.banner(cached: nil, current: "0.1.0", dismissed: nil) == nil)
    }

    @Test func AltStore_주소는_소스_주소를_인코딩해_넘긴다() throws {
        let url = UpdateCheck.altStoreURL
        let components = try #require(URLComponents(url: url, resolvingAgainstBaseURL: false))

        #expect(url.scheme == "altstore")
        #expect(url.host() == "source")
        #expect(components.queryItems?.first { $0.name == "url" }?.value == UpdateCheck.sourceURL)
    }
}

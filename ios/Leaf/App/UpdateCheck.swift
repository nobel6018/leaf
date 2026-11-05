import Foundation

// 새 버전 알림. 앱이 앞으로 올 때 하루에 한 번 GitHub 최신 Release를 보고, leaf-ios.ipa가 붙은 더 새 버전이면 목록 위에 알린다.
// 설치는 AltStore가 한다(무료 Apple ID 사이드로딩이라 앱이 스스로 설치하지 못한다).
// @see docs/ios.md, docs/release.md

/// 알릴 새 버전
struct AvailableUpdate: Codable, Equatable, Sendable {
    var version: String
    /// GitHub Release 페이지. AltStore가 없을 때 연다
    var releaseURL: URL
}

/// GitHub `releases/latest` 응답에서 쓰는 부분
struct GitHubRelease: Decodable, Sendable {
    struct Asset: Decodable, Sendable {
        var name: String
    }

    var tagName: String
    var htmlURL: URL
    var assets: [Asset]

    private enum CodingKeys: String, CodingKey {
        case tagName = "tag_name"
        case htmlURL = "html_url"
        case assets
    }
}

enum UpdateCheck {
    static let latestReleaseURL = URL(string: "https://api.github.com/repos/nobel6018/leaf/releases/latest")!
    /// Release는 플랫폼마다 빌드가 끝나는 대로 파일이 붙는다. iPhone 파일이 없으면 알리지 않는다
    static let assetName = "leaf-ios.ipa"
    /// AltStore 소스. 늘 최신 Release의 apps.json을 가리킨다(docs/release.md)
    static let sourceURL = "https://github.com/nobel6018/leaf/releases/latest/download/apps.json"
    static let interval: TimeInterval = 24 * 60 * 60

    /// AltStore에서 leaf 소스를 연다. 소스를 이미 넣었으면 AltStore가 그 소스를 보여 주고 My Apps에 Update 버튼이 뜬다
    static var altStoreURL: URL {
        var components = URLComponents()
        components.scheme = "altstore"
        components.host = "source"
        components.queryItems = [URLQueryItem(name: "url", value: sourceURL)]
        return components.url!
    }

    /// 점으로 나눈 숫자를 앞에서부터 비교한다(0.10.0 > 0.9.1). 앞의 `v`와 뒤표(`-beta.1`)는 무시한다
    static func isNewer(_ latest: String, than current: String) -> Bool {
        let a = numbers(latest)
        let b = numbers(current)
        for index in 0 ..< max(a.count, b.count) {
            let left = index < a.count ? a[index] : 0
            let right = index < b.count ? b[index] : 0
            if left != right { return left > right }
        }
        return false
    }

    /// 이 Release가 지금 버전보다 새롭고 iPhone 파일이 붙어 있으면 알릴 정보, 아니면 nil
    static func available(in release: GitHubRelease, current: String) -> AvailableUpdate? {
        let version = stripPrefix(release.tagName)
        guard isNewer(version, than: current) else { return nil }
        guard release.assets.contains(where: { $0.name == assetName }) else { return nil }
        return AvailableUpdate(version: version, releaseURL: release.htmlURL)
    }

    /// 하루에 한 번만 묻는다. 시계가 뒤로 갔으면 다시 묻는다
    static func shouldCheck(lastCheckedAt: Date?, now: Date) -> Bool {
        guard let lastCheckedAt else { return true }
        let elapsed = now.timeIntervalSince(lastCheckedAt)
        return elapsed < 0 || elapsed >= interval
    }

    /// 알림을 보여 줄지. 이미 그 버전 이상이 깔렸거나 사용자가 그 버전 알림을 닫았으면 숨긴다
    static func banner(cached: AvailableUpdate?, current: String, dismissed: String?) -> AvailableUpdate? {
        guard let cached, isNewer(cached.version, than: current), cached.version != dismissed else { return nil }
        return cached
    }

    static func fetchLatest(session: URLSession) async throws -> GitHubRelease {
        var request = URLRequest(url: latestReleaseURL)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.timeoutInterval = 15
        let (data, response) = try await session.data(for: request)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw URLError(.badServerResponse) }
        return try JSONDecoder().decode(GitHubRelease.self, from: data)
    }

    static var installedVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
    }

    private static func stripPrefix(_ tag: String) -> String {
        tag.hasPrefix("v") || tag.hasPrefix("V") ? String(tag.dropFirst()) : tag
    }

    private static func numbers(_ version: String) -> [Int] {
        // semver 뒤표(`-beta.1`, `+build`)는 비교하지 않는다. latest는 사전 배포판을 주지 않는다
        let core = stripPrefix(version.trimmingCharacters(in: .whitespaces)).prefix { $0 != "-" && $0 != "+" }
        return core
            .split(separator: ".", omittingEmptySubsequences: false)
            .map { part in Int(part.prefix { $0.isASCII && $0.isNumber }) ?? 0 }
    }
}

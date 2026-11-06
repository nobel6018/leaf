import Foundation
import Testing
@testable import Scanshelf

// 인텐트 테스트가 공용 ScanLauncher를 쓰므로 차례로 돌린다
@MainActor
@Suite(.serialized)
struct ScanStartTests {
    private func makeSettings() -> AppSettings {
        let name = "scanshelf-tests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defaults.removePersistentDomain(forName: name)
        return AppSettings(defaults: defaults)
    }

    @Test func 처음에는_문서_한_장으로_묶음_없이_시작한다() {
        let settings = makeSettings()

        let plan = ScanStartPlan.make(requested: nil, settings: settings)

        #expect(plan == ScanStartPlan(kind: .document, session: "", layout: .single))
    }

    @Test func 종류를_비우면_마지막에_고른_종류와_묶음을_쓴다() {
        let settings = makeSettings()
        settings.lastKind = .book
        settings.lastSession = "사피엔스"

        let plan = ScanStartPlan.make(requested: nil, settings: settings)

        // 책은 따로 고른 적이 없으면 펼친 두 페이지
        #expect(plan == ScanStartPlan(kind: .book, session: "사피엔스", layout: .spread))
    }

    @Test func 종류를_주면_그_종류의_마지막_배치를_쓴다() {
        let settings = makeSettings()
        settings.lastKind = .book
        settings.lastSession = "2026 예시박람회"
        settings.setLastLayout(.trifold, for: .pamphlet)

        let plan = ScanStartPlan.make(requested: .pamphlet, settings: settings)

        #expect(plan == ScanStartPlan(kind: .pamphlet, session: "2026 예시박람회", layout: .trifold))
    }

    @Test func 그_종류에_없는_배치는_기억하지_않는다() {
        let settings = makeSettings()

        settings.setLastLayout(.trifold, for: .book)

        #expect(settings.lastLayout(for: .book) == .spread)
    }

    @Test func 기억한_값은_다시_열어도_남는다() {
        let name = "scanshelf-tests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defer { defaults.removePersistentDomain(forName: name) }
        let first = AppSettings(defaults: defaults)
        first.lastKind = .pamphlet
        first.setLastLayout(.trifold, for: .pamphlet)

        let reopened = AppSettings(defaults: defaults)

        #expect(reopened.lastKind == .pamphlet)
        #expect(reopened.lastLayout(for: .pamphlet) == .trifold)
        // 예전 화면의 @AppStorage 키를 그대로 쓴다
        #expect(defaults.string(forKey: "lastDocumentKind") == "pamphlet")
    }

    @Test func 스캔_시작_인텐트는_고른_종류로_요청을_남긴다() async throws {
        _ = ScanLauncher.shared.take()

        _ = try await StartScanIntent(kind: .book).perform()
        let request = ScanLauncher.shared.take()

        #expect(request?.kind == .book)
        #expect(ScanLauncher.shared.pending == nil)
    }

    @Test func 종류를_비운_인텐트는_종류_없이_요청한다() async throws {
        _ = ScanLauncher.shared.take()

        _ = try await StartScanIntent().perform()
        let request = try #require(ScanLauncher.shared.take())

        #expect(request.kind == nil)
    }

    @Test func 단축어_종류는_manifest_종류와_값이_같다() {
        for option in [ScanKindOption.document, .book, .pamphlet] {
            #expect(DocumentKind(rawValue: option.rawValue) != nil)
        }
    }
}

struct KindLayoutTests {
    @Test func 팜플렛만_3단_접지를_고른다() {
        #expect(DocumentKind.pamphlet.layouts.contains(.trifold))
        #expect(!DocumentKind.book.layouts.contains(.trifold))
        #expect(!DocumentKind.document.layouts.contains(.trifold))
    }

    @Test func 묶음이_없으면_종류_이름으로_짓는다() {
        #expect(DocumentNaming.defaultName(session: nil, kind: .book, existingNames: ["책_2"]) == "책_3")
        #expect(DocumentNaming.defaultName(session: "", kind: .pamphlet, existingNames: ["문서_4"]) == "팜플렛_1")
    }
}

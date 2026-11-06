import Foundation

// 문서 manifest.json의 Swift 정의. 원본 정의는 crates/scanshelf-core/src/lib.rs이고,
// 두 쪽 모두 fixtures/manifest-v1.json을 읽는 테스트로 형식이 어긋나지 않는지 확인한다.
// @see docs/document-format.md

enum ManifestSchema {
    static let version = 1
    static let fileName = "manifest.json"
}

enum DocumentKind: String, Codable, Sendable {
    case pamphlet
    case book
    case document
}

enum PageLayout: String, Codable, Sendable {
    case single
    case spread
    /// 펼친 3단 접지 팜플렛. Mac이 세 면으로 나눈다
    case trifold
}

struct NormalizedPoint: Codable, Equatable, Sendable {
    var x: Double
    var y: Double
}

struct Quad: Codable, Equatable, Sendable {
    var topLeft: NormalizedPoint
    var topRight: NormalizedPoint
    var bottomRight: NormalizedPoint
    var bottomLeft: NormalizedPoint
}

struct Page: Codable, Equatable, Sendable, Identifiable {
    var id: UUID
    var raw: String
    var capturedAt: Date
    var layout: PageLayout
    var quad: Quad?

    private enum CodingKeys: String, CodingKey {
        case id, raw, capturedAt, layout, quad
    }

    init(id: UUID, raw: String, capturedAt: Date, layout: PageLayout, quad: Quad?) {
        self.id = id
        self.raw = raw
        self.capturedAt = capturedAt
        self.layout = layout
        self.quad = quad
    }

    // Swift UUID는 대문자로 쓴다. 폴더 이름, raw 경로, Rust uuid 출력이 모두 소문자라 맞춘다
    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(id.uuidString.lowercased(), forKey: .id)
        try container.encode(raw, forKey: .raw)
        try container.encode(capturedAt, forKey: .capturedAt)
        try container.encode(layout, forKey: .layout)
        try container.encodeIfPresent(quad, forKey: .quad)
    }
}

struct Manifest: Codable, Equatable, Sendable, Identifiable {
    var schemaVersion: Int
    var id: UUID
    var name: String
    var kind: DocumentKind
    var session: String?
    var createdAt: Date
    var pages: [Page]
    /// 저장할 때마다 1씩 오르는 번호(`DocumentStore.save`). Wi-Fi와 iCloud로 순서가 뒤바뀌어 도착해도 Mac이 옛 판으로 되돌아가지 않게 한다.
    /// 옛 앱이 만든 문서에는 없다
    var revision: Int?

    private enum CodingKeys: String, CodingKey {
        case schemaVersion, id, name, kind, session, createdAt, pages, revision
    }

    init(schemaVersion: Int, id: UUID, name: String, kind: DocumentKind, session: String?, createdAt: Date, pages: [Page], revision: Int? = nil) {
        self.schemaVersion = schemaVersion
        self.id = id
        self.name = name
        self.kind = kind
        self.session = session
        self.createdAt = createdAt
        self.pages = pages
        self.revision = revision
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(schemaVersion, forKey: .schemaVersion)
        try container.encode(id.uuidString.lowercased(), forKey: .id)
        try container.encode(name, forKey: .name)
        try container.encode(kind, forKey: .kind)
        try container.encodeIfPresent(session, forKey: .session)
        try container.encode(createdAt, forKey: .createdAt)
        try container.encode(pages, forKey: .pages)
        try container.encodeIfPresent(revision, forKey: .revision)
    }

    static func decoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .custom { decoder in
            let text = try decoder.singleValueContainer().decode(String.self)
            if let date = Manifest.parseDate(text) {
                return date
            }
            throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "RFC 3339 날짜가 아님: \(text)"))
        }
        return decoder
    }

    static func encoder() -> JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        encoder.dateEncodingStrategy = .custom { date, encoder in
            var container = encoder.singleValueContainer()
            try container.encode(Manifest.formatDate(date))
        }
        return encoder
    }

    // Rust chrono는 소수 초가 있을 때만 붙여 쓴다. 두 형식을 모두 읽는다
    private static func parseDate(_ text: String) -> Date? {
        let withFraction = Date.ISO8601FormatStyle(includingFractionalSeconds: true)
        let withoutFraction = Date.ISO8601FormatStyle()
        guard let date = (try? withFraction.parse(text)) ?? (try? withoutFraction.parse(text)) else {
            return nil
        }
        return Date(timeIntervalSince1970: (date.timeIntervalSince1970 * 1000).rounded() / 1000)
    }

    // Date는 Double이라 .412가 .41199…로 들어간다. 기본 형식화는 이걸 잘라 .411로 써서
    // 저장할 때마다 1ms씩 밀린다. 밀리초 정수로 바꿔 직접 쓴다. 시간대 Z가 있어야 Rust chrono가 읽는다
    private static func formatDate(_ date: Date) -> String {
        let milliseconds = Int64((date.timeIntervalSince1970 * 1000).rounded())
        let (seconds, fraction) = milliseconds.quotientAndRemainder(dividingBy: 1000)
        let whole = Date(timeIntervalSince1970: TimeInterval(seconds)).formatted(.iso8601)
        return whole.replacingOccurrences(of: "Z", with: String(format: ".%03dZ", fraction))
    }
}

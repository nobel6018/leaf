import Foundation

// 문서를 어디로 언제 보냈는지 기기 안에만 기록한다. manifest는 Mac과 나누는 형식이라 넣지 않는다.
// 원본 자동 삭제는 "보낸 뒤 바뀌지 않았고 Mac이 받았다고 확인된" 문서만 대상이다.
// @see docs/transfer.md, docs/ios.md

enum SendMethod: String, Codable, Sendable {
    case wifi
    case icloud
}

struct SendRecord: Codable, Equatable, Sendable {
    var method: SendMethod
    var sentAt: Date
    /// 보낼 때의 manifest 지문. 지금 지문과 다르면 그 뒤에 페이지를 더 찍은 것이다
    var manifestFingerprint: String
    /// Mac이 받았다고 확인됐는가. Wi-Fi는 manifest PUT 200, iCloud는 Mac이 inbox에서 가져가 폴더가 사라졌을 때
    var confirmed: Bool
}

enum SendStatus: Equatable, Sendable {
    case notSent
    case sending(progress: Double)
    /// iCloud에 복사했고 Mac이 가져가기를 기다린다
    case awaitingMac(SendMethod)
    case sent(SendMethod, Date)
    /// 보낸 뒤 페이지가 바뀌었다
    case changedSinceSent
    case failed(String)

    static func of(record: SendRecord?, currentFingerprint: String?) -> SendStatus {
        guard let record else { return .notSent }
        guard record.manifestFingerprint == currentFingerprint else { return .changedSinceSent }
        return record.confirmed ? .sent(record.method, record.sentAt) : .awaitingMac(record.method)
    }
}

/// Application Support/send-records.json. 문서 폴더 밖에 두어 iCloud나 Mac으로 섞여 가지 않는다
struct SendRecordStore: Sendable {
    let url: URL

    static func appDefault() -> SendRecordStore {
        SendRecordStore(url: URL.applicationSupportDirectory.appending(path: "send-records.json"))
    }

    func load() -> [UUID: SendRecord] {
        guard let data = try? Data(contentsOf: url),
              let records = try? JSONDecoder.leaf.decode([String: SendRecord].self, from: data)
        else { return [:] }
        return Dictionary(uniqueKeysWithValues: records.compactMap { key, value in
            UUID(uuidString: key).map { ($0, value) }
        })
    }

    func save(_ records: [UUID: SendRecord]) throws {
        let keyed = Dictionary(uniqueKeysWithValues: records.map { ($0.key.uuidString.lowercased(), $0.value) })
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try DocumentStore.writeAtomically(try JSONEncoder.leaf.encode(keyed), to: url)
    }
}

enum RetentionPolicy {
    /// 원본을 지울 문서. Mac이 받았다고 확인됐고, 보낸 뒤 바뀌지 않았고, 보낸 지 `days`일이 지난 것.
    /// days가 nil이면 지우지 않는다
    static func documentsToDelete(
        records: [UUID: SendRecord],
        fingerprints: [UUID: String],
        days: Int?,
        now: Date
    ) -> [UUID] {
        guard let days else { return [] }
        let limit = TimeInterval(days) * 24 * 60 * 60
        return records.compactMap { id, record in
            guard record.confirmed,
                  fingerprints[id] == record.manifestFingerprint,
                  now.timeIntervalSince(record.sentAt) >= limit
            else { return nil }
            return id
        }
        .sorted { $0.uuidString < $1.uuidString }
    }
}

enum DocumentNaming {
    /// 기본 이름은 `묶음_번호`. 같은 묶음에서 쓴 가장 큰 번호 + 1이다. 묶음이 없으면 종류 이름을 쓴다(`문서_1`, `책_1`, `팜플렛_1`).
    /// 팜플렛은 나중에 Mac AI가 표지 OCR로 회사명을 추천한다
    static func defaultName(session: String?, kind: DocumentKind = .document, existingNames: [String]) -> String {
        let trimmed = session?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let prefix = (trimmed.isEmpty ? kind.label : trimmed) + "_"
        let used = existingNames.compactMap { name -> Int? in
            guard name.hasPrefix(prefix) else { return nil }
            return Int(name.dropFirst(prefix.count))
        }
        return prefix + String((used.max() ?? 0) + 1)
    }
}

extension JSONEncoder {
    static var leaf: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.dateEncodingStrategy = .millisecondsSince1970
        return encoder
    }
}

extension JSONDecoder {
    static var leaf: JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .millisecondsSince1970
        return decoder
    }
}

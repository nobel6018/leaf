import Foundation
import Security

// 짝지은 Mac의 토큰은 키체인에만 둔다. 무료 Apple ID 서명이라 접근 그룹을 따로 쓰지 않고 앱 기본 그룹에 둔다.
// 이 기기에서만 열리게 하고(백업으로 다른 기기에 넘어가지 않게) 잠금 해제 뒤에만 읽는다.
// @see docs/transfer.md

enum KeychainError: Error, LocalizedError {
    case status(OSStatus)

    var errorDescription: String? {
        switch self {
        case let .status(code):
            let message = SecCopyErrorMessageString(code, nil) as String? ?? "알 수 없는 오류"
            return "키체인 오류 \(code): \(message)"
        }
    }
}

struct Keychain: Sendable {
    let service: String

    static let macToken = Keychain(service: "com.nobel6018.leaf.mac-token")

    func read(account: String) throws -> Data? {
        var query = baseQuery(account: account)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess else { throw KeychainError.status(status) }
        return item as? Data
    }

    func write(_ data: Data, account: String) throws {
        let attributes: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        let status = SecItemUpdate(baseQuery(account: account) as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            var query = baseQuery(account: account)
            query.merge(attributes) { _, new in new }
            let added = SecItemAdd(query as CFDictionary, nil)
            guard added == errSecSuccess else { throw KeychainError.status(added) }
            return
        }
        guard status == errSecSuccess else { throw KeychainError.status(status) }
    }

    func delete(account: String) throws {
        let status = SecItemDelete(baseQuery(account: account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw KeychainError.status(status) }
    }

    private func baseQuery(account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
    }
}

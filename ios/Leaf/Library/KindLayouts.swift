import Foundation

// 문서 종류마다 고를 수 있는 배치와 화면 이름. 배치는 페이지마다 manifest `layout`에 들어간다.
// @see docs/document-format.md, docs/ios.md

extension DocumentKind {
    /// 화면과 단축어에 보이는 순서(문서, 책, 팜플렛)
    static let displayOrder: [DocumentKind] = [.document, .book, .pamphlet]

    var label: String {
        switch self {
        case .pamphlet: "팜플렛"
        case .book: "책"
        case .document: "문서"
        }
    }

    /// 촬영 화면에서 고를 수 있는 배치. 3단 접지는 팜플렛만, 펼친 두 쪽은 모든 종류(책, 두 쪽짜리 문서, 반 접은 리플릿)
    var layouts: [PageLayout] {
        switch self {
        case .pamphlet: [.single, .spread, .trifold]
        case .book, .document: [.single, .spread]
        }
    }

    /// 처음 고르는 배치. 책은 펼쳐서 찍는 경우가 많아 두 쪽이 기본이다
    var defaultLayout: PageLayout {
        self == .book ? .spread : .single
    }
}

extension PageLayout {
    /// 촬영 화면의 짧은 이름
    var shortLabel: String {
        switch self {
        case .single: "한 장"
        case .spread: "두 쪽"
        case .trifold: "3단"
        }
    }

    /// 페이지 띠, 문서 화면의 장 번호 옆 표시. 한 장은 붙이지 않는다
    var badge: String? {
        switch self {
        case .single: nil
        case .spread: "두 쪽"
        case .trifold: "3단"
        }
    }
}

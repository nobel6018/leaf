import AppIntents

// "스캔 시작" 동작. 앱 타깃과 제어 센터 확장(ScanshelfControls)이 함께 컴파일한다.
// 제어 센터 버튼은 확장이 그리지만, 앱을 여는 인텐트는 앱 프로세스에서 실행되므로 두 타깃에 같은 타입이 있어야 한다.
// 확장 쪽에서는 SCANSHELF_CONTROLS 조건으로 앱 코드 호출을 뺀다.
// 인텐트는 앱 타깃에 두어야 Xcode가 단축어 앱이 읽는 메타데이터(Metadata.appintents)를 뽑는다.
// @see docs/ios.md "스캔 시작 동작"

/// 단축어에서 고르는 종류. 값은 manifest `kind`와 같다
enum ScanKindOption: String, AppEnum {
    case document
    case book
    case pamphlet

    static let typeDisplayRepresentation: TypeDisplayRepresentation = "문서 종류"
    static let caseDisplayRepresentations: [ScanKindOption: DisplayRepresentation] = [
        .document: "문서",
        .book: "책",
        .pamphlet: "팜플렛",
    ]
}

struct StartScanIntent: AppIntent {
    static let title: LocalizedStringResource = "스캔 시작"
    static let description = IntentDescription("scanshelf를 열고 바로 새 문서 촬영을 시작합니다. 종류를 비우면 마지막에 고른 종류, 묶음은 마지막에 쓴 묶음입니다")
    /// 촬영은 카메라 화면이 있어야 하므로 앱을 앞으로 띄운다
    static let openAppWhenRun = true

    @Parameter(title: "종류", description: "비우면 마지막에 고른 종류")
    var kind: ScanKindOption?

    init() {}

    init(kind: ScanKindOption?) {
        self.kind = kind
    }

    static var parameterSummary: some ParameterSummary {
        Summary("\(\.$kind) 스캔 시작")
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        #if !SCANSHELF_CONTROLS
        ScanLauncher.shared.request(kind: kind.flatMap { DocumentKind(rawValue: $0.rawValue) })
        #endif
        return .result()
    }
}

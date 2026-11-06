import CoreGraphics
import ImageIO

// manifest의 quad는 "원본 사진에 EXIF 회전을 적용한 뒤" 기준 0~1 좌표이고 원점이 왼쪽 위다.
// Vision 결과는 원점이 왼쪽 아래이고, 카메라 버퍼는 센서 방향(가로)이라 세로 촬영과 축이 다르다.
// 좌표계를 오가는 계산을 여기 모아 테스트로 고정한다.
// @see docs/capture.md, docs/document-format.md

enum QuadGeometry {
    /// Vision 정규화 좌표(원점 왼쪽 아래)를 manifest 좌표(원점 왼쪽 위)로 바꾼다.
    /// Vision에 넘긴 orientation이 표시 방향과 같다면 결과는 이미 표시 방향 기준이라 y만 뒤집으면 된다
    static func fromVision(_ point: CGPoint) -> NormalizedPoint {
        NormalizedPoint(x: clamp(Double(point.x)), y: clamp(1 - Double(point.y)))
    }

    static func fromVision(topLeft: CGPoint, topRight: CGPoint, bottomRight: CGPoint, bottomLeft: CGPoint) -> Quad {
        ordered([fromVision(topLeft), fromVision(topRight), fromVision(bottomRight), fromVision(bottomLeft)])
    }

    /// 버퍼에 저장된 방향(원점 왼쪽 위, 0~1)의 점을 EXIF orientation을 적용한 표시 방향 점으로 바꾼다.
    /// 세로로 든 iPhone 후면 카메라 버퍼는 `.right`다(표시하려면 시계 방향 90도 회전)
    static func applying(_ orientation: CGImagePropertyOrientation, to point: NormalizedPoint) -> NormalizedPoint {
        let x = point.x
        let y = point.y
        switch orientation {
        case .up: return NormalizedPoint(x: x, y: y)
        case .upMirrored: return NormalizedPoint(x: 1 - x, y: y)
        case .down: return NormalizedPoint(x: 1 - x, y: 1 - y)
        case .downMirrored: return NormalizedPoint(x: x, y: 1 - y)
        case .right: return NormalizedPoint(x: 1 - y, y: x)
        case .rightMirrored: return NormalizedPoint(x: 1 - y, y: 1 - x)
        case .left: return NormalizedPoint(x: y, y: 1 - x)
        case .leftMirrored: return NormalizedPoint(x: y, y: x)
        }
    }

    static func applying(_ orientation: CGImagePropertyOrientation, to quad: Quad) -> Quad {
        ordered(quad.points.map { applying(orientation, to: $0) })
    }

    /// 회전하고 나면 "왼쪽 위"였던 점이 다른 자리로 간다. 이름은 위치로 다시 붙인다.
    /// 위쪽 두 점과 아래쪽 두 점을 나눈 뒤 각각 x로 좌우를 정한다(거치대 촬영은 크게 기울지 않는다)
    static func ordered(_ points: [NormalizedPoint]) -> Quad {
        precondition(points.count == 4, "꼭짓점은 4개")
        let byY = points.sorted { $0.y < $1.y }
        let top = byY[0 ..< 2].sorted { $0.x < $1.x }
        let bottom = byY[2 ..< 4].sorted { $0.x < $1.x }
        return Quad(topLeft: top[0], topRight: top[1], bottomRight: bottom[1], bottomLeft: bottom[0])
    }

    private static func clamp(_ value: Double) -> Double {
        min(1, max(0, value))
    }
}

extension Quad {
    var points: [NormalizedPoint] {
        [topLeft, topRight, bottomRight, bottomLeft]
    }

    /// 화면 그리기용. 정규화 좌표를 주어진 크기의 점으로 바꾼다
    func cgPoints(in size: CGSize) -> [CGPoint] {
        points.map { CGPoint(x: $0.x * size.width, y: $0.y * size.height) }
    }

    /// 신발끈 공식 넓이(0~1). 너무 작은 감지는 문서로 보지 않는다
    var area: Double {
        let p = points
        var sum = 0.0
        for i in 0 ..< 4 {
            let a = p[i]
            let b = p[(i + 1) % 4]
            sum += a.x * b.y - b.x * a.y
        }
        return abs(sum) / 2
    }
}

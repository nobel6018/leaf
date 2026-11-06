import ImageIO
import SwiftUI

// 페이지 띠와 문서 화면의 작은 그림. HEIC 원본을 통째로 풀지 않고 ImageIO 축소본만 만든다.

final class ThumbnailCache: @unchecked Sendable {
    static let shared = ThumbnailCache()

    private let cache = NSCache<NSURL, UIImage>()

    func image(for url: URL, maxPixel: Int) async -> UIImage? {
        if let cached = cache.object(forKey: url as NSURL) { return cached }
        let image = await Task.detached(priority: .utility) { () -> UIImage? in
            guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
            let options: [CFString: Any] = [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: maxPixel,
            ]
            guard let cgImage = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
            return UIImage(cgImage: cgImage)
        }.value
        if let image { cache.setObject(image, forKey: url as NSURL) }
        return image
    }
}

struct PageThumbnail: View {
    let url: URL
    var quad: Quad?
    var maxPixel = 360

    @State private var image: UIImage?

    var body: some View {
        ZStack {
            if let image {
                Image(uiImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .overlay {
                        if let quad {
                            GeometryReader { proxy in
                                QuadShape(quad: quad)
                                    .stroke(Color.green, lineWidth: 1.5)
                                    .frame(width: proxy.size.width, height: proxy.size.height)
                            }
                        }
                    }
            } else {
                Rectangle().fill(Color.gray.opacity(0.25))
            }
        }
        .task(id: url) {
            image = await ThumbnailCache.shared.image(for: url, maxPixel: maxPixel)
        }
    }
}

/// 정규화 테두리를 주어진 크기에 그린다
struct QuadShape: Shape {
    let quad: Quad

    func path(in rect: CGRect) -> Path {
        let points = quad.cgPoints(in: rect.size).map { CGPoint(x: $0.x + rect.minX, y: $0.y + rect.minY) }
        var path = Path()
        path.addLines(points)
        path.closeSubpath()
        return path
    }
}

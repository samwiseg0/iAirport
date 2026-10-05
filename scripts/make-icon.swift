// Renders the iairport app icon and writes Resources/AppIcon.icns.
// Usage: swift scripts/make-icon.swift [output.icns]
// Needs only CoreGraphics, ImageIO and the system `iconutil`.
//
// The icon is a glossy CRT screen in a chrome bezel, with a radial glow,
// scanlines and a glass reflection. iairport uses an orange tint and a single
// glowing Wi-Fi symbol.
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let canvas: CGFloat = 1024

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255, blue: CGFloat(hex & 0xff) / 255, alpha: alpha)
}

func degrees(_ value: CGFloat) -> CGFloat { value * .pi / 180 }

func gradient(_ colors: [CGColor], _ locations: [CGFloat]) -> CGGradient {
    CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB)!, colors: colors as CFArray, locations: locations)!
}

func roundedRect(_ rect: CGRect, _ radius: CGFloat) -> CGPath {
    CGPath(roundedRect: rect, cornerWidth: radius, cornerHeight: radius, transform: nil)
}

func wifiPath(center: CGPoint, outer: CGFloat, stroke: CGFloat) -> CGPath {
    let path = CGMutablePath()
    for fraction in [CGFloat(0.36), 0.68, 1.0] {
        let arc = CGMutablePath()
        arc.addArc(center: center, radius: outer * fraction, startAngle: degrees(45), endAngle: degrees(135), clockwise: false)
        path.addPath(arc.copy(strokingWithWidth: stroke, lineCap: .round, lineJoin: .round, miterLimit: 10))
    }
    let dot = stroke * 0.68
    path.addEllipse(in: CGRect(x: center.x - dot, y: center.y - dot, width: dot * 2, height: dot * 2))
    return path
}

func drawIcon(in ctx: CGContext) {
    // macOS icon grid: 824 pt body centred on the 1024 canvas.
    let body = CGRect(x: 100, y: 100, width: 824, height: 824)
    let bodyPath = roundedRect(body, 185)

    // Drop shadow under the body.
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: color(0x000000, 0.4))
    ctx.addPath(bodyPath)
    ctx.setFillColor(color(0x8A8A90))
    ctx.fillPath()
    ctx.restoreGState()

    // Chrome bezel: bright at the top, darker toward the bottom.
    ctx.saveGState()
    ctx.addPath(bodyPath)
    ctx.clip()
    ctx.drawLinearGradient(gradient([color(0xF4F4F6), color(0xC4C4CA), color(0x8C8C93), color(0x5E5E64)], [0, 0.35, 0.75, 1]),
                           start: CGPoint(x: 512, y: body.maxY), end: CGPoint(x: 512, y: body.minY), options: [])
    ctx.restoreGState()
    // Thin highlight along the outer edge.
    ctx.addPath(roundedRect(body.insetBy(dx: 3, dy: 3), 182))
    ctx.setStrokeColor(color(0xFFFFFF, 0.55))
    ctx.setLineWidth(4)
    ctx.strokePath()

    // Dark lip between bezel and screen.
    let lip = body.insetBy(dx: 44, dy: 44)
    let lipPath = roundedRect(lip, 145)
    ctx.addPath(lipPath)
    ctx.setFillColor(color(0x141416))
    ctx.fillPath()

    // Screen: orange CRT glow, brightest in the middle.
    let screen = lip.insetBy(dx: 14, dy: 14)
    let screenPath = roundedRect(screen, 132)
    let center = CGPoint(x: screen.midX, y: screen.midY)
    ctx.saveGState()
    ctx.addPath(screenPath)
    ctx.clip()
    ctx.setFillColor(color(0x2A0C02))
    ctx.fill(screen)
    ctx.drawRadialGradient(gradient([color(0xFFA040), color(0xE85F10), color(0x8A2E05), color(0x2A0B02)], [0, 0.3, 0.66, 1]),
                           startCenter: center, startRadius: 0, endCenter: center, endRadius: screen.width * 0.72, options: [.drawsAfterEndLocation])

    // Scanlines.
    ctx.setFillColor(color(0x000000, 0.13))
    var y = screen.minY
    while y < screen.maxY {
        ctx.fill(CGRect(x: screen.minX, y: y, width: screen.width, height: 4))
        y += 9
    }

    // Vignette toward the corners.
    ctx.drawRadialGradient(gradient([color(0x000000, 0), color(0x000000, 0.45)], [0.55, 1]),
                           startCenter: center, startRadius: 0, endCenter: center, endRadius: screen.width * 0.74, options: [.drawsAfterEndLocation])

    // Wi-Fi symbol with a bloom, centred on its bounding box.
    let outer: CGFloat = 268
    let stroke: CGFloat = 58
    let symbolCenter = CGPoint(x: center.x, y: center.y - (outer + stroke / 2 - stroke * 0.68) / 2)
    let symbol = wifiPath(center: symbolCenter, outer: outer, stroke: stroke)
    ctx.saveGState()
    ctx.setShadow(offset: .zero, blur: 60, color: color(0xFFD9A0, 0.95))
    ctx.addPath(symbol)
    ctx.setFillColor(color(0xFFF6EA))
    ctx.fillPath()
    ctx.restoreGState()
    ctx.saveGState()
    ctx.setShadow(offset: .zero, blur: 16, color: color(0xFFFFFF, 0.9))
    ctx.addPath(symbol)
    ctx.setFillColor(color(0xFFFBF5))
    ctx.fillPath()
    ctx.restoreGState()

    // Glass reflection: a soft band across the top of the screen.
    let glare = CGMutablePath()
    glare.move(to: CGPoint(x: screen.minX, y: screen.maxY))
    glare.addLine(to: CGPoint(x: screen.maxX, y: screen.maxY))
    glare.addLine(to: CGPoint(x: screen.maxX, y: screen.maxY - screen.height * 0.30))
    glare.addQuadCurve(to: CGPoint(x: screen.minX, y: screen.maxY - screen.height * 0.42),
                       control: CGPoint(x: screen.midX, y: screen.maxY - screen.height * 0.30))
    glare.closeSubpath()
    ctx.addPath(glare)
    ctx.clip()
    ctx.drawLinearGradient(gradient([color(0xFFFFFF, 0.22), color(0xFFFFFF, 0.04)], [0, 1]),
                           start: CGPoint(x: 512, y: screen.maxY), end: CGPoint(x: 512, y: screen.maxY - screen.height * 0.42), options: [])
    ctx.restoreGState()

    // Inner shadow along the screen edge, so it sits behind the lip.
    ctx.saveGState()
    ctx.addPath(screenPath)
    ctx.clip()
    let ring = CGMutablePath()
    ring.addRect(screen.insetBy(dx: -60, dy: -60))
    ring.addPath(screenPath)
    ctx.addPath(ring)
    ctx.setShadow(offset: CGSize(width: 0, height: -6), blur: 22, color: color(0x000000, 0.8))
    ctx.setFillColor(color(0x000000))
    ctx.fillPath(using: .evenOdd)
    ctx.restoreGState()
}

func render(size: Int) -> CGImage {
    let space = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0, space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .high
    ctx.setShouldAntialias(true)
    ctx.scaleBy(x: CGFloat(size) / canvas, y: CGFloat(size) / canvas)
    drawIcon(in: ctx)
    return ctx.makeImage()!
}

func writePNG(_ image: CGImage, to url: URL) {
    let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, image, nil)
    guard CGImageDestinationFinalize(dest) else { fatalError("could not write \(url.path)") }
}

let output = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "Resources/AppIcon.icns"
let iconset = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("iairport-\(getpid()).iconset")
try? FileManager.default.removeItem(at: iconset)
try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
for base in [16, 32, 128, 256, 512] {
    writePNG(render(size: base), to: iconset.appendingPathComponent("icon_\(base)x\(base).png"))
    writePNG(render(size: base * 2), to: iconset.appendingPathComponent("icon_\(base)x\(base)@2x.png"))
}
let preview = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("iairport-icon-preview-\(getpid()).png")
writePNG(render(size: 1024), to: preview)

let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["-c", "icns", iconset.path, "-o", output]
try iconutil.run()
iconutil.waitUntilExit()
try? FileManager.default.removeItem(at: iconset)
guard iconutil.terminationStatus == 0 else { fatalError("iconutil failed") }
print("wrote \(output)")
print("preview \(preview.path)")

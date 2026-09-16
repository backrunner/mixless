#!/usr/bin/env swift
// Render the DMG install window background: background.png (660x400) and
// background@2x.png (1320x800) next to each other so dmgbuild compiles the
// HiDPI pair. Usage: swift scripts/generate-dmg-background.swift <outdir>
import AppKit
import ImageIO
import UniformTypeIdentifiers

let W: CGFloat = 660
let H: CGFloat = 400
// Icon view layout, mirrored in scripts/dmg-settings.py (top-left origin).
let appIcon = CGPoint(x: 180, y: 168)
let appsIcon = CGPoint(x: 480, y: 168)

// Brand palette (apps/desktop/src/theme.rs)
let bgTop = NSColor(calibratedRed: 0x1b / 255, green: 0x1b / 255, blue: 0x1e / 255, alpha: 1)
let bgBottom = NSColor(calibratedRed: 0x0a / 255, green: 0x0a / 255, blue: 0x0b / 255, alpha: 1)
let accent = NSColor(calibratedRed: 0xff / 255, green: 0xb2 / 255, blue: 0x24 / 255, alpha: 1)
let text = NSColor(calibratedRed: 0xf0 / 255, green: 0xed / 255, blue: 0xe6 / 255, alpha: 1)
let muted = NSColor(calibratedRed: 0x8b / 255, green: 0x87 / 255, blue: 0x7e / 255, alpha: 1)

func draw(_ ctx: CGContext) {
    // CG origin is bottom-left; layout values are top-origin, so flip y.
    func y(_ top: CGFloat) -> CGFloat { H - top }

    let gradient = CGGradient(
        colorsSpace: CGColorSpace(name: CGColorSpace.sRGB)!,
        colors: [bgTop.cgColor, bgBottom.cgColor] as CFArray,
        locations: [0, 1]
    )!
    ctx.drawLinearGradient(
        gradient,
        start: CGPoint(x: W / 2, y: H),
        end: CGPoint(x: W / 2, y: 0),
        options: []
    )

    // Faint amber ring marks the double-click target behind the app icon.
    let ring = CGRect(
        x: appIcon.x - 76, y: y(appIcon.y + 76),
        width: 152, height: 152
    )
    ctx.setStrokeColor(accent.withAlphaComponent(0.35).cgColor)
    ctx.setLineWidth(1.5)
    ctx.addPath(CGPath(roundedRect: ring, cornerWidth: 28, cornerHeight: 28, transform: nil))
    ctx.strokePath()

    // Arrow from the app icon to the Applications alias.
    let arrowY = y(appIcon.y)
    let arrow = CGMutablePath()
    arrow.move(to: CGPoint(x: appIcon.x + 88, y: arrowY))
    arrow.addLine(to: CGPoint(x: appsIcon.x - 104, y: arrowY))
    ctx.setStrokeColor(accent.cgColor)
    ctx.setLineWidth(4)
    ctx.setLineCap(.round)
    ctx.addPath(arrow)
    ctx.strokePath()
    let head = CGMutablePath()
    head.move(to: CGPoint(x: appsIcon.x - 104, y: arrowY))
    head.addLine(to: CGPoint(x: appsIcon.x - 122, y: arrowY + 10))
    head.move(to: CGPoint(x: appsIcon.x - 104, y: arrowY))
    head.addLine(to: CGPoint(x: appsIcon.x - 122, y: arrowY - 10))
    ctx.addPath(head)
    ctx.strokePath()

    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: false)
    func centered(_ string: String, _ top: CGFloat, _ font: NSFont, _ color: NSColor) {
        let label = NSAttributedString(string: string, attributes: [
            .font: font,
            .foregroundColor: color,
        ])
        let size = label.size()
        label.draw(at: CGPoint(x: (W - size.width) / 2, y: y(top) - size.height))
    }
    centered(
        "Double-click Mixless to install",
        328,
        NSFont.systemFont(ofSize: 15, weight: .semibold),
        text
    )
    centered(
        "or drag it to Applications",
        354,
        NSFont.systemFont(ofSize: 12, weight: .regular),
        muted
    )
    NSGraphicsContext.restoreGraphicsState()
}

func writePNG(_ image: CGImage, to url: URL) {
    guard let destination = CGImageDestinationCreateWithURL(
        url as CFURL, UTType.png.identifier as CFString, 1, nil)
    else { fatalError("Cannot create \(url.path)") }
    CGImageDestinationAddImage(destination, image, nil)
    guard CGImageDestinationFinalize(destination) else {
        fatalError("Cannot write \(url.path)")
    }
}

let outdir = URL(
    fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ".",
    isDirectory: true
)
try FileManager.default.createDirectory(at: outdir, withIntermediateDirectories: true)
let space = CGColorSpace(name: CGColorSpace.sRGB)!
// Render once at 4x, then downsample each representation for crisp edges.
let master = CGContext(
    data: nil, width: Int(W * 4), height: Int(H * 4), bitsPerComponent: 8,
    bytesPerRow: Int(W * 4) * 4, space: space,
    bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
)!
master.scaleBy(x: 4, y: 4)
draw(master)
let image = master.makeImage()!
for scale in [1, 2] {
    let ctx = CGContext(
        data: nil, width: Int(W) * scale, height: Int(H) * scale, bitsPerComponent: 8,
        bytesPerRow: Int(W) * scale * 4, space: space,
        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
    )!
    ctx.interpolationQuality = .high
    ctx.draw(image, in: CGRect(x: 0, y: 0, width: W * CGFloat(scale), height: H * CGFloat(scale)))
    let name = scale == 2 ? "background@2x.png" : "background.png"
    writePNG(ctx.makeImage()!, to: outdir.appendingPathComponent(name))
}
print("DMG background written to \(outdir.path)")

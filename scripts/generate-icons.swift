#!/usr/bin/env swift
// Derive all app assets from the saved Replicate image; no API call is needed.
// Run from any directory: swift scripts/generate-icons.swift
import AppKit
import ImageIO
import UniformTypeIdentifiers

let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let brand = root.appendingPathComponent("assets/branding")
let resources = root.appendingPathComponent("apps/desktop/resources")
let catalog = resources.appendingPathComponent("Assets.xcassets/AppIcon.appiconset")
let iconset = resources.appendingPathComponent("Mixless.iconset")
let sourceURL = brand.appendingPathComponent("m-source.png")
guard let source = CGImageSourceCreateWithURL(sourceURL as CFURL, nil),
      let master = CGImageSourceCreateImageAtIndex(source, 0, nil),
      master.width == 1024, master.height == 1024 else {
    fatalError("Expected a 1024 x 1024 image at \(sourceURL.path)")
}
for url in [catalog, iconset] {
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
}
let colorSpace = CGColorSpace(name: CGColorSpace.sRGB)!
func context(_ width: Int, _ height: Int, transparent: Bool = false) -> CGContext {
    let output = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
        bytesPerRow: width * 4, space: colorSpace,
        bitmapInfo: (transparent ? CGImageAlphaInfo.premultipliedLast : CGImageAlphaInfo.noneSkipLast).rawValue)!
    output.interpolationQuality = .high
    return output
}
func writePNG(_ image: CGImage, to url: URL) {
    guard let destination = CGImageDestinationCreateWithURL(
        url as CFURL, UTType.png.identifier as CFString, 1, nil) else {
        fatalError("Cannot create \(url.path)")
    }
    CGImageDestinationAddImage(destination, image, nil)
    guard CGImageDestinationFinalize(destination) else {
        fatalError("Cannot write \(url.path)")
    }
}
func renderImage(_ size: Int, desktop: Bool = false) -> CGImage {
    let output = context(size, size, transparent: desktop)
    let inset = desktop ? CGFloat(size) * 0.08 : 0
    let bounds = CGRect(x: inset, y: inset, width: CGFloat(size) - inset * 2,
        height: CGFloat(size) - inset * 2)
    if desktop {
        // macOS ICNS images carry their own silhouette and transparent margins.
        let radius = bounds.width * 0.22
        output.addPath(CGPath(roundedRect: bounds, cornerWidth: radius,
            cornerHeight: radius, transform: nil))
        output.clip()
    }
    output.setFillColor(CGColor(colorSpace: colorSpace, components: [20.0/255, 20.0/255, 22.0/255, 1])!)
    output.fill(bounds)
    output.draw(master, in: bounds)
    return output.makeImage()!
}
writePNG(renderImage(1024), to: brand.appendingPathComponent("app-icon-1024.png"))
var images: [[String: String]] = []
for size in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let name = "icon_\(size)x\(size)\(scale == 2 ? "@2x" : "").png"
        let output = iconset.appendingPathComponent(name)
        writePNG(renderImage(size * scale, desktop: true), to: output)
        try Data(contentsOf: output).write(to: catalog.appendingPathComponent(name))
        images.append(["idiom": "mac", "size": "\(size)x\(size)", "scale": "\(scale)x", "filename": name])
    }
}
let contents: [String: Any] = ["images": images, "info": ["author": "xcode", "version": 1]]
try JSONSerialization.data(withJSONObject: contents, options: [.prettyPrinted, .sortedKeys])
    .write(to: catalog.appendingPathComponent("Contents.json"))
try Data("{\"info\":{\"author\":\"xcode\",\"version\":1}}\n".utf8)
    .write(to: resources.appendingPathComponent("Assets.xcassets/Contents.json"))
let process = Process()
process.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
process.arguments = ["-c", "icns", iconset.path, "-o", resources.appendingPathComponent("Mixless.icns").path]
try process.run()
process.waitUntilExit()
guard process.terminationStatus == 0 else { fatalError("iconutil failed") }

// Show the actual desktop exports at native size; the store master stays square.
let preview = context(720, 360)
preview.setFillColor(CGColor(colorSpace: colorSpace, components: [0.14, 0.14, 0.15, 1])!)
preview.fill(CGRect(x: 0, y: 0, width: 720, height: 360))
let sizes = [256, 128, 64, 32, 16]
let positions = [24, 316, 480, 584, 664]
for (size, x) in zip(sizes, positions) {
    let bounds = CGRect(x: x, y: 54 + (256 - size) / 2, width: size, height: size)
    preview.draw(renderImage(size, desktop: true), in: bounds)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(cgContext: preview, flipped: false)
    let label = NSAttributedString(string: "\(size) px", attributes: [
        .font: NSFont.systemFont(ofSize: 10),
        .foregroundColor: NSColor(white: 0.9, alpha: 1),
    ])
    label.draw(at: CGPoint(x: min(x, 650), y: 18))
    NSGraphicsContext.restoreGraphicsState()
}
writePNG(preview.makeImage()!, to: brand.appendingPathComponent("icon-preview.png"))
print("Generated refined M icon: opaque sRGB master, rounded macOS assets, ICNS and size preview")

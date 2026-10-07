// Renders the menu bar panel to PNG (light and dark) from daemon status and
// config JSON, using the app's real SwiftUI views. Needs no screen-recording
// permission. The README images come from the shared fixtures:
//
//   swift run RenderScreenshots ../fixtures/status_response.json ../fixtures/config_response.json ../docs/images

import AppKit
import DimKit
import DimUI
import SwiftUI

let args = CommandLine.arguments
guard args.count == 4 else {
    print("usage: RenderScreenshots <status.json> <config.json> <output-dir>")
    exit(2)
}
let statusData = try Data(contentsOf: URL(fileURLWithPath: args[1]))
// Accept both a bare Status (`dimctl status --json`) and a {"type":"status",...} response.
let status: Status
if case let .status(s)? = try? JSONDecoder.daemon.decode(Response.self, from: statusData) {
    status = s
} else {
    status = try JSONDecoder.daemon.decode(Status.self, from: statusData)
}
let configData = try Data(contentsOf: URL(fileURLWithPath: args[2]))
let config: Config
if case let .config(c)? = try? JSONDecoder.daemon.decode(Response.self, from: configData) {
    config = c
} else {
    config = try JSONDecoder.daemon.decode(Config.self, from: configData)
}
let outDir = URL(fileURLWithPath: args[3], isDirectory: true)
try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

/// The panel as it appears under the menu bar: rounded, on the window background.
struct Panel: View {
    let model: AppModel

    var body: some View {
        MenuContent(model: model)
            .background(Color(nsColor: .windowBackgroundColor))
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(.separator, lineWidth: 0.5))
    }
}

@MainActor
func render(dark: Bool, to url: URL) throws {
    let appearance = NSAppearance(named: dark ? .darkAqua : .aqua)!
    let host = NSHostingView(rootView: Panel(model: AppModel(previewStatus: status, config: config)))
    host.appearance = appearance
    // Lay out once and let onAppear state settle (settings load there), then measure.
    host.frame = NSRect(origin: .zero, size: host.fittingSize)
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
    let size = host.fittingSize
    host.frame = NSRect(origin: .zero, size: size)
    let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
    window.appearance = appearance
    window.isOpaque = false
    window.backgroundColor = .clear
    window.contentView = host
    host.layoutSubtreeIfNeeded()
    host.display()

    let scale = 2
    guard
        let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: Int(size.width) * scale, pixelsHigh: Int(size.height) * scale,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
    else { throw CocoaError(.featureUnsupported) }
    rep.size = size
    NSAppearance.current = appearance
    host.cacheDisplay(in: host.bounds, to: rep)
    guard let png = rep.representation(using: .png, properties: [:]) else { throw CocoaError(.fileWriteUnknown) }
    try png.write(to: url)
    print("wrote \(url.path) (\(Int(size.width))×\(Int(size.height)) pt)")
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
MainActor.assumeIsolated {
    do {
        try render(dark: false, to: outDir.appendingPathComponent("panel-light.png"))
        try render(dark: true, to: outDir.appendingPathComponent("panel-dark.png"))
    } catch {
        print("render failed: \(error)")
        exit(1)
    }
}

// Software dimming: a click-through black window over every screen.
//
// Normal compositing of black at opacity a gives out = in × (1 − a), an exact
// brightness multiply for every app and colour space, unlike tinting overlays
// which wash out contrast. The colour temperature itself comes from Night
// Shift (see crates/dimd/src/nightshift.rs).

import AppKit

@MainActor
final class DimOverlay {
    private var windows: [NSWindow] = []
    private var alpha: CGFloat = 0
    private var observer: NSObjectProtocol?

    /// Hard ceiling so a bad value can never black out the screen.
    static let maxAlpha: CGFloat = 0.9

    init() {
        observer = NotificationCenter.default.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.rebuild() }
        }
    }

    func set(dimPct: Double) {
        let a = min(max(CGFloat(dimPct / 100), 0), Self.maxAlpha)
        guard abs(a - alpha) > 0.001 else { return }
        alpha = a
        if a > 0, windows.isEmpty {
            rebuild()
            windows.forEach { $0.alphaValue = 0 }  // fade in from undimmed
        }
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 1.5
            for w in windows { w.animator().alphaValue = a }
        } completionHandler: {
            MainActor.assumeIsolated {
                if self.alpha == 0 { self.tearDown() }
            }
        }
    }

    private func rebuild() {
        tearDown()
        guard alpha > 0 else { return }
        windows = NSScreen.screens.map { screen in
            let w = OverlayWindow(screen: screen)
            w.alphaValue = alpha
            w.orderFrontRegardless()
            return w
        }
    }

    private func tearDown() {
        windows.forEach { $0.orderOut(nil) }
        windows = []
    }
}

private final class OverlayWindow: NSWindow {
    init(screen: NSScreen) {
        super.init(contentRect: screen.frame, styleMask: .borderless, backing: .buffered, defer: false)
        setFrame(screen.frame, display: false)
        isOpaque = false
        backgroundColor = .black
        hasShadow = false
        ignoresMouseEvents = true
        isReleasedWhenClosed = false
        // Above everything, menu bar and Dock included, so the whole screen dims evenly.
        level = NSWindow.Level(rawValue: Int(CGShieldingWindowLevel()))
        collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle, .fullScreenAuxiliary]
        // Keep screenshots and screen sharing at normal brightness.
        sharingType = .none
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

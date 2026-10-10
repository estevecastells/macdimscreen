import AppKit
import DimKit
import Foundation
import Observation
import ServiceManagement

public enum Connection: Equatable {
    case connecting
    case connected
    /// Daemon not installed or not running.
    case missing
    case failed(String)
}

@MainActor
@Observable
public final class AppModel {
    public private(set) var status: Status?
    public private(set) var config: Config?
    public private(set) var connection: Connection = .connecting
    /// The menu bar icon, kept apart from `status` so the status item only
    /// redraws when the icon itself changes, not on every poll.
    public private(set) var menuBarSymbol = MenuBarLabel.symbol(status: nil, connection: .connecting)
    private(set) var actionError: String?
    private(set) var installing = false
    private(set) var openAtLogin = false
    private(set) var loginItemNeedsApproval = false

    public let updates = Updates()
    private let client = DaemonClient()
    /// The bundled background service is checked against the running one once per launch.
    private var serviceChecked = false
    private let overlay = DimOverlay()
    private var pollTask: Task<Void, Never>?
    private var panelOpen = false
    /// README screenshots: never talk to the daemon.
    private var isPreview = false
    private var windowObservers: [NSObjectProtocol] = []
    /// The mode to go back to after a live preview from a slider.
    private var previewRestore: Mode?

    /// A status call is a single local socket round-trip. While the panel is
    /// open it's polled often so it feels live; while closed only the menu bar
    /// icon and the dimming overlay need it, and the daemon only moves on every
    /// 15 s tick (or a command), so a slower poll saves wakeups.
    static let openPollInterval: Duration = .seconds(2)
    static let closedPollInterval: Duration = .seconds(5)

    var pollInterval: Duration { panelOpen ? Self.openPollInterval : Self.closedPollInterval }

    public init() {
        configureLoginItemOnFirstLaunch()
        observePanelWindow()
        start()
        updates.start()
    }

    /// A static model for README screenshots: no daemon, no polling, no overlay.
    public init(previewStatus: Status, config: Config) {
        status = previewStatus
        self.config = config
        connection = .connected
        menuBarSymbol = MenuBarLabel.symbol(status: previewStatus, connection: .connected)
        openAtLogin = true
        isPreview = true
    }

    // MARK: Open at login

    private static let loginItemConfiguredKey = "loginItemConfigured"

    /// Open at login by default the first time the app runs; after that the
    /// user's choice (from the menu or System Settings) wins.
    private func configureLoginItemOnFirstLaunch() {
        let defaults = UserDefaults.standard
        if !defaults.bool(forKey: Self.loginItemConfiguredKey) {
            defaults.set(true, forKey: Self.loginItemConfiguredKey)
            setOpenAtLogin(true)
        }
        refreshLoginItem()
    }

    func refreshLoginItem() {
        let state = SMAppService.mainApp.status
        openAtLogin = state == .enabled || state == .requiresApproval
        loginItemNeedsApproval = state == .requiresApproval
    }

    func setOpenAtLogin(_ enabled: Bool) {
        do {
            if enabled {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
            actionError = nil
        } catch {
            actionError = "Couldn't change the login item: \(error.localizedDescription)"
        }
        refreshLoginItem()
    }

    func openLoginItemSettings() {
        SMAppService.openSystemSettingsLoginItems()
    }

    // MARK: Daemon

    public func start() {
        pollTask?.cancel()
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refresh()
                guard let interval = self?.pollInterval else { return }
                // The tolerance lets macOS coalesce this wakeup with others.
                try? await Task.sleep(for: interval, tolerance: interval / 5)
            }
        }
    }

    /// The panel is the app's only window that can become key (the dimming
    /// overlay can't), so key status tracks whether it's open. This backs up
    /// the panel's onAppear/onDisappear, which SwiftUI doesn't call on every
    /// open and close of a menu bar window on every macOS version.
    private func observePanelWindow() {
        let center = NotificationCenter.default
        for (name, open) in [(NSWindow.didBecomeKeyNotification, true), (NSWindow.didResignKeyNotification, false)] {
            windowObservers.append(center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.panelVisibilityChanged(open) }
            })
        }
    }

    /// The panel opened or closed. Opening refreshes at once and polls faster.
    public func panelVisibilityChanged(_ open: Bool) {
        guard open != panelOpen, !isPreview else { return }
        panelOpen = open
        if open { start() }
    }

    func refresh() async {
        do {
            let s = try await client.status()
            if status.map({ !$0.showsSame(as: s) }) ?? true { status = s }
            if config == nil { config = try await client.config() }
            assign(\.connection, .connected)
            overlay.set(dimPct: s.target.dimPct)
            await updateServiceIfOutdated()
        } catch DaemonError.notRunning {
            if status != nil { status = nil }
            if config != nil { config = nil }
            assign(\.connection, .missing)
            overlay.set(dimPct: 0)
        } catch {
            assign(\.connection, .failed(error.localizedDescription))
        }
        assign(\.menuBarSymbol, MenuBarLabel.symbol(status: status, connection: connection))
    }

    /// Observation notifies on every assignment, even of an equal value, and each
    /// notification re-renders the menu bar item. Assign only real changes.
    private func assign<T: Equatable>(_ keyPath: ReferenceWritableKeyPath<AppModel, T>, _ value: T) {
        if self[keyPath: keyPath] != value { self[keyPath: keyPath] = value }
    }

    func setMode(_ mode: Mode) {
        perform(.setMode(mode))
    }

    func pause(minutes: Int) {
        setMode(.paused(until: Date().addingTimeInterval(TimeInterval(minutes * 60))))
    }

    /// Change settings. The daemon validates and persists them.
    func update(_ change: (inout Config) -> Void) {
        guard var c = config else { return }
        change(&c)
        if let restore = previewRestore {
            c.mode = restore
            previewRestore = nil
        }
        config = c
        perform(.setConfig(c))
    }

    /// Show the night look right now while a slider is being dragged, even in
    /// daylight; `update` afterwards saves the value and ends the preview.
    func preview(kelvin: Double, dimPct: Double, tintPct: Double) {
        guard let c = config else { return }
        if previewRestore == nil { previewRestore = c.mode }
        perform(.setMode(.manual(kelvin: kelvin, dimPct: dimPct, tintPct: tintPct)), refreshConfig: false)
    }

    private func perform(_ request: Request, refreshConfig: Bool = true) {
        Task {
            do {
                if case let .config(c) = try await client.send(request), refreshConfig { config = c }
                actionError = nil
            } catch {
                actionError = error.localizedDescription
                config = try? await client.config()
            }
            await refresh()
        }
    }

    /// After the app updates itself, the service it bundles is newer than the one
    /// running: reinstall it, silently (it's a per-user agent, no password).
    private func updateServiceIfOutdated() async {
        guard !serviceChecked, !installing else { return }
        serviceChecked = true
        guard case let .pong(running, _)? = try? await client.send(.ping), let runningVersion = Version(running),
            runningVersion < Updates.currentVersion
        else { return }
        installDaemon()
    }

    /// Install (or reinstall) the background service from the app bundle. It's a
    /// per-user launchd agent, so no administrator password is needed.
    func installDaemon() {
        guard let resources = Bundle.main.resourceURL,
            FileManager.default.fileExists(atPath: resources.appendingPathComponent("install.sh").path)
        else {
            actionError = "Installer not found in the app bundle. Run `make install` from the repository instead."
            return
        }
        installing = true
        let script = resources.appendingPathComponent("install.sh").path
        Task.detached {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/bin/bash")
            p.arguments = [script, "--bin-dir", resources.path]
            let pipe = Pipe()
            p.standardOutput = pipe
            p.standardError = pipe
            var message: String?
            do {
                try p.run()
                p.waitUntilExit()
                if p.terminationStatus != 0 {
                    let out = String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
                    message = "Install failed: \(out.split(separator: "\n").last ?? "unknown error")"
                }
            } catch {
                message = "Install failed: \(error.localizedDescription)"
            }
            let failure = message
            await MainActor.run {
                self.installing = false
                self.actionError = failure
            }
            await self.refresh()
        }
    }

    func openLog() {
        NSWorkspace.shared.open(URL(fileURLWithPath: NSHomeDirectory() + "/Library/Logs/MacDimScreen/dimd.log"))
    }
}

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
    private(set) var actionError: String?
    private(set) var installing = false
    private(set) var openAtLogin = false
    private(set) var loginItemNeedsApproval = false

    private let client = DaemonClient()
    private let overlay = DimOverlay()
    private var pollTask: Task<Void, Never>?
    /// The mode to go back to after a live preview from a slider.
    private var previewRestore: Mode?

    /// A status call is a single local socket round-trip.
    static let pollInterval: Duration = .seconds(2)

    public init() {
        configureLoginItemOnFirstLaunch()
        start()
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
                try? await Task.sleep(for: AppModel.pollInterval)
            }
        }
    }

    func refresh() async {
        do {
            let s = try await client.status()
            status = s
            if config == nil { config = try await client.config() }
            connection = .connected
            overlay.set(dimPct: s.target.dimPct)
        } catch DaemonError.notRunning {
            (status, config, connection) = (nil, nil, .missing)
            overlay.set(dimPct: 0)
        } catch {
            connection = .failed(error.localizedDescription)
        }
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

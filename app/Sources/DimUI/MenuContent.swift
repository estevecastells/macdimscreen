import DimKit
import SwiftUI

public struct MenuContent: View {
    let model: AppModel

    public init(model: AppModel) {
        self.model = model
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            switch model.connection {
            case .connected:
                if let status = model.status {
                    StatusView(model: model, status: status)
                }
            case .connecting:
                ProgressView("Connecting…").frame(maxWidth: .infinity)
            case .missing:
                SetupView(model: model)
            case let .failed(message):
                Label(message, systemImage: "exclamationmark.triangle").foregroundStyle(.red)
            }

            if let error = model.actionError {
                Text(error).font(.caption).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true)
            }

            Divider()
            HStack {
                Toggle("Open at login", isOn: Binding(get: { model.openAtLogin }, set: { model.setOpenAtLogin($0) }))
                    .toggleStyle(.checkbox)
                    .font(.callout)
                if model.loginItemNeedsApproval {
                    Button("Approve…") { model.openLoginItemSettings() }
                        .buttonStyle(.link)
                        .font(.caption)
                        .help("macOS needs you to allow MacDimScreen in Login Items")
                }
            }
            HStack {
                Button("Log") { model.openLog() }
                Spacer()
                Button("Quit") { NSApplication.shared.terminate(nil) }
                    .help("Night Shift keeps following the schedule; only the dimming overlay stops")
            }
            .buttonStyle(.borderless)
            .font(.callout)
        }
        .padding(14)
        .frame(width: 320)
    }
}

@MainActor
private struct SetupView: View {
    let model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("Background service not installed", systemImage: "moon.zzz").font(.headline)
            Text("MacDimScreen uses a small background service to warm your screen on schedule, even when this menu isn't open. It runs as you; no password needed.")
                .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            Button(model.installing ? "Installing…" : "Install Background Service") { model.installDaemon() }
                .disabled(model.installing)
                .controlSize(.large)
        }
    }
}

@MainActor
private struct StatusView: View {
    let model: AppModel
    let status: Status

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            header
            warnings
            ModePicker(model: model, status: status)
            if let config = model.config {
                Divider()
                SettingsView(model: model, config: config)
            }
        }
    }

    private var header: some View {
        HStack(alignment: .center, spacing: 10) {
            Image(systemName: phaseSymbol(status.target.phase))
                .font(.system(size: 26))
                .foregroundStyle(warmth(status.target.kelvin))
                .frame(width: 34)
            VStack(alignment: .leading, spacing: 2) {
                Text(headline).font(.system(size: 20, weight: .semibold, design: .rounded)).monospacedDigit()
                Text(subline).font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private var headline: String {
        let t = status.target
        var s = "\(t.phase.title) · \(status.appliedKelvin.map(Format.kelvin) ?? "6500K")"
        if status.appliedTintPct != nil { s += "+" }
        if t.dimPct >= 1 { s += " · \(Format.percent(t.dimPct)) dim" }
        return s
    }

    private var subline: String {
        let t = status.target
        switch status.mode {
        case let .paused(until): return "Paused until \(Format.time(until))"
        case .off: return "Turned off · screen colours unchanged"
        case .manual: return "Holding a fixed colour"
        case .auto: break
        }
        guard let next = t.nextChange?.date else { return "Following the sun" }
        let what: String = switch t.nextPhase {
        case .sunset: "Sunset transition"
        case .night: "Full night colour"
        case .sunrise: "Morning transition"
        case .day: "Daylight"
        default: "Next change"
        }
        return "\(what) at \(Format.time(next))"
    }

    @ViewBuilder private var warnings: some View {
        if !status.nightShiftAvailable {
            Label(status.lastError ?? "Night Shift isn't available on this Mac.", systemImage: "xmark.octagon")
                .font(.caption).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true)
        } else if let e = status.lastError {
            Label(e, systemImage: "exclamationmark.triangle").font(.caption).foregroundStyle(.orange)
                .fixedSize(horizontal: false, vertical: true)
        }
        if status.locationEstimated {
            Label("Location estimated from your time zone. Set it below for accurate sunset times.", systemImage: "location.slash")
                .font(.caption).foregroundStyle(.orange).fixedSize(horizontal: false, vertical: true)
        }
        if status.clamped {
            Label("Night Shift can't go below 2700K; using that.", systemImage: "info.circle")
                .font(.caption).foregroundStyle(.secondary)
        }
    }
}

@MainActor
private struct ModePicker: View {
    let model: AppModel
    let status: Status

    private enum Choice: Hashable { case auto, pause, off }

    private var choice: Choice {
        switch status.mode {
        case .auto, .manual: .auto
        case .paused: .pause
        case .off: .off
        }
    }

    var body: some View {
        Picker("", selection: Binding(get: { choice }, set: { select($0) })) {
            Text("Schedule").tag(Choice.auto)
            Text("Pause 1 h").tag(Choice.pause)
            Text("Off").tag(Choice.off)
        }
        .pickerStyle(.segmented)
        .labelsHidden()
    }

    private func select(_ c: Choice) {
        switch c {
        case .auto: model.setMode(.auto)
        case .pause: model.pause(minutes: 60)
        case .off: model.setMode(.off)
        }
    }
}

@MainActor
private struct SettingsView: View {
    let model: AppModel
    let config: Config

    @State private var nightKelvin: Double = 3400
    @State private var dimPct: Double = 0
    @State private var tintPct: Double = 40
    @State private var tintOn = false
    @State private var latitude = ""
    @State private var longitude = ""
    @State private var followSunrise = true
    /// Only a slider being dragged previews; programmatic updates must not.
    @State private var dragging = false
    @State private var wake = Calendar.current.date(bySettingHour: 8, minute: 0, second: 0, of: Date())!

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            slider(
                "Night colour", value: $nightKelvin, range: 2700...6500, step: 100, text: Format.kelvin(nightKelvin),
                preview: { model.preview(kelvin: nightKelvin, dimPct: dimPct, tintPct: tintOn ? tintPct : 0) },
                commit: { model.update { $0.nightKelvin = nightKelvin } })
            slider(
                "Night dimming", value: $dimPct, range: 0...80, step: 5, text: Format.percent(dimPct),
                preview: { model.preview(kelvin: nightKelvin, dimPct: dimPct, tintPct: tintOn ? tintPct : 0) },
                commit: { model.update { $0.nightDimPct = dimPct } })

            Toggle("Extra warmth", isOn: Binding(get: { tintOn }, set: { on in
                tintOn = on
                model.update { $0.nightTintPct = on ? tintPct : 0 }
            }))
            .toggleStyle(.checkbox).font(.callout)
            .help("Adds the Accessibility colour tint at night, for an f.lux-like orange. Your own colour filter settings are restored when MacDimScreen stops.")
            if tintOn {
                slider(
                    "Tint strength", value: $tintPct, range: 25...100, step: 5, text: Format.percent(tintPct),
                    preview: { model.preview(kelvin: nightKelvin, dimPct: dimPct, tintPct: tintPct) },
                    commit: { model.update { $0.nightTintPct = tintPct } })
            }

            HStack {
                Text("Transition").font(.callout)
                Spacer()
                Picker("", selection: Binding(get: { config.transitionMinutes }, set: { v in model.update { $0.transitionMinutes = v } })) {
                    ForEach([20.0, 40, 60, 90], id: \.self) { Text(Format.minutes($0)).tag($0) }
                }
                .labelsHidden().fixedSize()
            }

            HStack {
                Toggle("Daylight by", isOn: Binding(get: { !followSunrise }, set: { on in
                    followSunrise = !on
                    model.update { $0.wakeTime = on ? hhmm(wake) : nil }
                }))
                .toggleStyle(.checkbox).font(.callout)
                Spacer()
                if followSunrise {
                    Text("sunrise").font(.callout).foregroundStyle(.secondary)
                } else {
                    DatePicker("", selection: $wake, displayedComponents: .hourAndMinute)
                        .labelsHidden().fixedSize()
                        .onChange(of: wake) { _, w in
                            if hhmm(w) != config.wakeTime { model.update { $0.wakeTime = hhmm(w) } }
                        }
                }
            }

            HStack(spacing: 6) {
                Text("Location").font(.callout)
                Spacer()
                TextField("Lat", text: $latitude).frame(width: 64).onSubmit(saveLocation)
                TextField("Lon", text: $longitude).frame(width: 64).onSubmit(saveLocation)
                Button("Set", action: saveLocation).disabled(!locationChanged)
            }
            .textFieldStyle(.roundedBorder)
            .font(.callout)
        }
        .onAppear(perform: load)
        .onChange(of: config) { _, _ in load() }
    }

    private func slider(
        _ title: String, value: Binding<Double>, range: ClosedRange<Double>, step: Double, text: String,
        preview: @escaping () -> Void, commit: @escaping () -> Void
    ) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(title).font(.callout)
                Spacer()
                Text(text).font(.callout).monospacedDigit().foregroundStyle(.secondary)
            }
            Slider(value: value, in: range, step: step) { editing in
                dragging = editing
                if editing { preview() } else { commit() }
            }
            .onChange(of: value.wrappedValue) { _, _ in if dragging { preview() } }
        }
    }

    private var locationChanged: Bool {
        guard let lat = Double(latitude), let lon = Double(longitude) else { return false }
        return lat != config.latitude || lon != config.longitude || config.locationEstimated
    }

    private func saveLocation() {
        guard let lat = Double(latitude), let lon = Double(longitude), (-90...90).contains(lat), (-180...180).contains(lon)
        else { return }
        model.update {
            $0.latitude = lat
            $0.longitude = lon
            $0.locationEstimated = false
        }
    }

    private func load() {
        nightKelvin = config.nightKelvin
        dimPct = config.nightDimPct
        tintOn = config.nightTintPct > 0
        if tintOn { tintPct = config.nightTintPct }
        latitude = String(format: "%.2f", config.latitude)
        longitude = String(format: "%.2f", config.longitude)
        followSunrise = config.wakeTime == nil
        if let w = config.wakeTime, let d = parse(w) { wake = d }
    }

    private func hhmm(_ d: Date) -> String {
        let c = Calendar.current.dateComponents([.hour, .minute], from: d)
        return String(format: "%02d:%02d", c.hour ?? 0, c.minute ?? 0)
    }

    private func parse(_ s: String) -> Date? {
        let parts = s.split(separator: ":").compactMap { Int($0) }
        guard parts.count == 2 else { return nil }
        return Calendar.current.date(bySettingHour: parts[0], minute: parts[1], second: 0, of: Date())
    }
}

func phaseSymbol(_ p: Phase) -> String {
    switch p {
    case .day: "sun.max.fill"
    case .sunset: "sunset.fill"
    case .night: "moon.fill"
    case .sunrise: "sunrise.fill"
    case .paused: "pause.circle"
    case .off: "sun.max"
    case .manual: "slider.horizontal.3"
    }
}

/// Icon tint: amber when warm, neutral by day.
func warmth(_ kelvin: Double) -> Color {
    let f = min(max((6500 - kelvin) / 3800, 0), 1)
    return Color(hue: 0.09, saturation: 0.25 + 0.65 * f, brightness: 0.95)
}

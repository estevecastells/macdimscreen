// Mirrors the daemon's JSON protocol (crates/dim-core/src/protocol.rs).
// Keys travel in snake_case; use `JSONDecoder.daemon` / `JSONEncoder.daemon`.

import Foundation

public enum Mode: Equatable, Sendable {
    case auto
    case paused(until: Date)
    case off
    case manual(kelvin: Double, dimPct: Double, tintPct: Double = 0)
}

extension Mode: Codable {
    private enum CodingKeys: String, CodingKey { case kind, until, kelvin, dimPct, tintPct }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "auto": self = .auto
        case "off": self = .off
        case "paused":
            self = .paused(until: Date(timeIntervalSince1970: TimeInterval(try c.decode(Int64.self, forKey: .until))))
        case "manual":
            self = .manual(
                kelvin: try c.decode(Double.self, forKey: .kelvin), dimPct: try c.decode(Double.self, forKey: .dimPct),
                tintPct: try c.decodeIfPresent(Double.self, forKey: .tintPct) ?? 0)
        case let other:
            throw DecodingError.dataCorruptedError(forKey: .kind, in: c, debugDescription: "unknown mode \(other)")
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .auto: try c.encode("auto", forKey: .kind)
        case .off: try c.encode("off", forKey: .kind)
        case let .paused(until):
            try c.encode("paused", forKey: .kind)
            try c.encode(Int64(until.timeIntervalSince1970.rounded()), forKey: .until)
        case let .manual(kelvin, dimPct, tintPct):
            try c.encode("manual", forKey: .kind)
            try c.encode(kelvin, forKey: .kelvin)
            try c.encode(dimPct, forKey: .dimPct)
            try c.encode(tintPct, forKey: .tintPct)
        }
    }
}

public enum Phase: String, Codable, Sendable {
    case day, sunset, night, sunrise, paused, off, manual

    public init(from decoder: Decoder) throws {
        // Unknown future phases degrade gracefully instead of failing the whole status.
        self = Phase(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .day
    }

    public var title: String {
        switch self {
        case .day: "Daylight"
        case .sunset: "Sunset"
        case .night: "Night"
        case .sunrise: "Morning"
        case .paused: "Paused"
        case .off: "Off"
        case .manual: "Manual"
        }
    }
}

/// Unix seconds on the wire.
public struct UnixTime: Codable, Equatable, Sendable {
    public let date: Date

    public init(_ date: Date) { self.date = date }

    public init(from decoder: Decoder) throws {
        date = Date(timeIntervalSince1970: TimeInterval(try decoder.singleValueContainer().decode(Int64.self)))
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        try c.encode(Int64(date.timeIntervalSince1970.rounded()))
    }
}

public struct DayPlan: Codable, Equatable, Sendable {
    public var sunrise: UnixTime?
    public var sunset: UnixTime?
    public var morning: UnixTime?
    public var evening: UnixTime?
    public var polarDay: Bool
}

public struct Target: Codable, Equatable, Sendable {
    public var phase: Phase
    public var kelvin: Double
    public var dimPct: Double
    public var tintPct: Double
    public var nightFactor: Double
    public var nextChange: UnixTime?
    public var nextPhase: Phase?
    public var today: DayPlan
}

public struct Status: Codable, Equatable, Sendable {
    public var mode: Mode
    public var target: Target
    /// What Night Shift is showing; nil when it's off.
    public var appliedKelvin: Double?
    /// Extra-warmth colour tint shown, in percent; nil when off.
    public var appliedTintPct: Double?
    public var clamped: Bool
    public var nightShiftAvailable: Bool
    public var latitude: Double
    public var longitude: Double
    public var locationEstimated: Bool
    public var lastError: String?
    public var uptimeS: Double
    public var ticks: UInt64

    /// Equal apart from `uptimeS` and `ticks`, which change on every daemon tick
    /// but aren't shown anywhere. Lets the app skip a redraw when nothing visible changed.
    public func showsSame(as other: Status) -> Bool {
        var a = self
        (a.uptimeS, a.ticks) = (other.uptimeS, other.ticks)
        return a == other
    }
}

public struct Config: Codable, Equatable, Sendable {
    public var mode: Mode
    public var latitude: Double
    public var longitude: Double
    public var locationEstimated: Bool
    public var dayKelvin: Double
    public var nightKelvin: Double
    public var nightDimPct: Double
    /// Extra warmth (Accessibility colour tint): 0 = off, else 25–100.
    public var nightTintPct: Double
    public var transitionMinutes: Double
    /// "HH:MM", or nil to follow sunrise.
    public var wakeTime: String?

    public init(
        mode: Mode = .auto, latitude: Double, longitude: Double, locationEstimated: Bool = false,
        dayKelvin: Double = 6500, nightKelvin: Double = 3400, nightDimPct: Double = 0, nightTintPct: Double = 0,
        transitionMinutes: Double = 40, wakeTime: String? = nil
    ) {
        self.mode = mode
        self.latitude = latitude
        self.longitude = longitude
        self.locationEstimated = locationEstimated
        self.dayKelvin = dayKelvin
        self.nightKelvin = nightKelvin
        self.nightDimPct = nightDimPct
        self.nightTintPct = nightTintPct
        self.transitionMinutes = transitionMinutes
        self.wakeTime = wakeTime
    }
}

public enum Request: Encodable, Equatable, Sendable {
    case ping
    case status
    case getConfig
    case setMode(Mode)
    case setConfig(Config)

    private enum CodingKeys: String, CodingKey { case cmd, mode, config }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .ping: try c.encode("ping", forKey: .cmd)
        case .status: try c.encode("status", forKey: .cmd)
        case .getConfig: try c.encode("get_config", forKey: .cmd)
        case let .setMode(mode):
            try c.encode("set_mode", forKey: .cmd)
            try c.encode(mode, forKey: .mode)
        case let .setConfig(config):
            try c.encode("set_config", forKey: .cmd)
            try c.encode(config, forKey: .config)
        }
    }
}

public enum Response: Decodable, Sendable {
    case pong(version: String, protocol: Int)
    case status(Status)
    case config(Config)
    case error(String)

    private enum CodingKeys: String, CodingKey { case type, version, `protocol`, status, config, message }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "pong": self = .pong(version: try c.decode(String.self, forKey: .version), protocol: try c.decode(Int.self, forKey: .protocol))
        case "status": self = .status(try c.decode(Status.self, forKey: .status))
        case "config": self = .config(try c.decode(Config.self, forKey: .config))
        case "error": self = .error(try c.decode(String.self, forKey: .message))
        case let other:
            throw DecodingError.dataCorruptedError(forKey: .type, in: c, debugDescription: "unknown response \(other)")
        }
    }
}

extension JSONDecoder {
    public static var daemon: JSONDecoder {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        return d
    }
}

extension JSONEncoder {
    public static var daemon: JSONEncoder {
        let e = JSONEncoder()
        e.keyEncodingStrategy = .convertToSnakeCase
        e.outputFormatting = .sortedKeys
        return e
    }
}

extension DaemonClient {
    public func config() async throws -> Config {
        guard case let .config(c) = try await send(.getConfig) else { throw DaemonError.badResponse("expected config") }
        return c
    }
}

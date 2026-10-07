// Protocol conformance checks for DimKit: `swift run KitChecks`.
// Exits non-zero on failure. Wire fixtures live in /fixtures and are shared
// with the Rust tests, so the two sides can't drift apart silently.

import DimKit
import Foundation

var failures = 0
var passed = 0

func check(_ condition: @autoclosure () throws -> Bool, _ name: String, file: StaticString = #file, line: UInt = #line) {
    do {
        if try condition() {
            passed += 1
        } else {
            failures += 1
            print("FAIL \(name) (\(file):\(line))")
        }
    } catch {
        failures += 1
        print("FAIL \(name): threw \(error)")
    }
}

func decode<T: Decodable>(_ type: T.Type, _ json: String) throws -> T {
    try JSONDecoder.daemon.decode(T.self, from: Data(json.utf8))
}

func encode<T: Encodable>(_ v: T) throws -> String {
    String(decoding: try JSONEncoder.daemon.encode(v), as: UTF8.self)
}

let repoRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
func fixture(_ name: String) -> String {
    try! String(contentsOf: repoRoot.appendingPathComponent("fixtures/\(name)"), encoding: .utf8)
}

// Responses
check(try {
    guard case let .status(s) = try decode(Response.self, fixture("status_response.json")) else { return false }
    return s.mode == .auto && s.target.phase == .night && s.target.kelvin == 3400 && s.target.dimPct == 15 && s.target.tintPct == 40 && s.appliedTintPct == 40
        && s.target.nextPhase == .sunrise && s.target.nextChange?.date == Date(timeIntervalSince1970: 1_791_443_400)
        && s.target.today.sunset != nil && !s.target.today.polarDay
        && s.appliedKelvin == 3400 && !s.clamped && s.nightShiftAvailable && s.latitude == 41.5
        && !s.locationEstimated && s.lastError == nil && s.ticks == 240
}(), "decodes a full status response")

let configJSON = fixture("config_response.json")
check(try {
    guard case let .config(c) = try decode(Response.self, configJSON) else { return false }
    return c.mode == .auto && c.nightKelvin == 3400 && c.nightTintPct == 40 && c.wakeTime == "09:00" && c.transitionMinutes == 40
}(), "decodes a config response")

// A config sent back must use exactly the daemon's field names (it rejects unknown fields).
check(try {
    guard case let .config(c) = try decode(Response.self, configJSON) else { return false }
    let sent = try JSONSerialization.jsonObject(with: JSONEncoder.daemon.encode(Request.setConfig(c))) as! [String: Any]
    let raw = try JSONSerialization.jsonObject(with: Data(configJSON.utf8)) as! [String: Any]
    return Set((sent["config"] as! [String: Any]).keys) == Set((raw["config"] as! [String: Any]).keys)
}(), "set_config round-trips the daemon's field names")

check(try decode(Phase.self, "\"something_new\"") == .day, "unknown phases degrade gracefully")
check(try decode(Mode.self, #"{"kind":"manual","kelvin":2700,"dim_pct":20}"#) == .manual(kelvin: 2700, dimPct: 20), "manual mode without tint")

check(try {
    guard case let .error(m) = try decode(Response.self, #"{"type":"error","message":"latitude 91 must be between -90 and 90"}"#) else { return false }
    return m.contains("latitude")
}(), "error response")

check(try {
    guard case let .pong(v, p) = try decode(Response.self, #"{"type":"pong","version":"0.1.0","protocol":1}"#) else { return false }
    return v == "0.1.0" && p == 1
}(), "pong response")

// Requests must match the shared fixtures, which the Rust tests also parse.
let requestFixtures = fixture("requests.jsonl").split(separator: "\n").map(String.init)
let requests: [Request] = [
    .status, .getConfig, .setMode(.auto), .setMode(.off),
    .setMode(.paused(until: Date(timeIntervalSince1970: 1_800_000_000))),
    .setMode(.manual(kelvin: 2700, dimPct: 20, tintPct: 40)),
]
check(requestFixtures.count == requests.count, "request fixture count")
for (req, expected) in zip(requests, requestFixtures) {
    check(try encode(req) == expected, "request encodes as \(expected)")
}

// Formatting
check(Format.kelvin(3412) == "3410K" && Format.kelvin(6500) == "6500K", "kelvin formatting")
check(Format.minutes(40) == "40 min" && Format.minutes(90) == "1 h 30 min" && Format.minutes(60) == "1 h", "minutes")

// Client error mapping: connecting to a missing socket means "not running".
let semaphore = DispatchSemaphore(value: 0)
Task {
    do {
        _ = try await DaemonClient(socketPath: "/tmp/definitely-missing-\(getpid()).sock").status()
        check(false, "missing socket should throw")
    } catch {
        check((error as? DaemonError) == .notRunning, "missing socket maps to notRunning")
    }
    semaphore.signal()
}
semaphore.wait()

print("\(passed) passed, \(failures) failed")
exit(failures == 0 ? 0 : 1)

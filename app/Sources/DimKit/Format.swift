import Foundation

public enum Format {
    public static func kelvin(_ k: Double) -> String {
        "\(Int((k / 10).rounded()) * 10)K"
    }

    public static func percent(_ p: Double) -> String {
        "\(Int(p.rounded()))%"
    }

    /// "21:05" in the user's locale and time zone.
    public static func time(_ date: Date) -> String {
        date.formatted(date: .omitted, time: .shortened)
    }

    /// Minutes as "40 min" or "1 h 30 min".
    public static func minutes(_ m: Double) -> String {
        let m = Int(m.rounded())
        if m < 60 { return "\(m) min" }
        return m % 60 == 0 ? "\(m / 60) h" : "\(m / 60) h \(m % 60) min"
    }
}

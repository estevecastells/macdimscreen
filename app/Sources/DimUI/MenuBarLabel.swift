import DimKit
import SwiftUI

public struct MenuBarLabel: View {
    let symbol: String

    public init(symbol: String) {
        self.symbol = symbol
    }

    public var body: some View {
        Image(systemName: symbol)
    }

    /// The icon for a daemon state. `AppModel` stores the result and only
    /// publishes it when it changes, so polling doesn't redraw the status item.
    public static func symbol(status: Status?, connection: Connection) -> String {
        guard connection == .connected, let status else { return "moon.zzz" }
        if status.lastError != nil { return "exclamationmark.triangle" }
        return phaseSymbol(status.target.phase)
    }
}

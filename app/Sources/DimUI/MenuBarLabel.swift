import DimKit
import SwiftUI

public struct MenuBarLabel: View {
    let status: Status?
    let connection: Connection

    public init(status: Status?, connection: Connection) {
        self.status = status
        self.connection = connection
    }

    public var body: some View {
        Image(systemName: icon)
    }

    private var icon: String {
        guard connection == .connected, let status else { return "moon.zzz" }
        if status.lastError != nil { return "exclamationmark.triangle" }
        return phaseSymbol(status.target.phase)
    }
}

import DimUI
import ServiceManagement
import SwiftUI

@main
@MainActor
struct MacDimScreenApp: App {
    @State private var model: AppModel

    init() {
        // Diagnostics: `MacDimScreen.app/Contents/MacOS/MacDimScreen --login-item-status`
        if CommandLine.arguments.contains("--login-item-status") {
            let names: [SMAppService.Status: String] = [
                .enabled: "enabled", .notRegistered: "not registered",
                .requiresApproval: "requires approval in System Settings › Login Items", .notFound: "not found",
            ]
            print("open at login: \(names[SMAppService.mainApp.status] ?? "unknown")")
            exit(0)
        }
        _model = State(initialValue: AppModel())
    }

    var body: some Scene {
        MenuBarExtra {
            MenuContent(model: model)
        } label: {
            MenuBarLabel(status: model.status, connection: model.connection)
        }
        .menuBarExtraStyle(.window)
    }
}

// Settings: sync with the Mac, the notes on the phone, and the theme.

import RuneCore
import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @Environment(SyncCoordinator.self) private var sync
    @AppStorage("appearance") private var appearance = "system"
    @State private var picking = false

    @State private var disconnecting = false

    private var cloudLine: String {
        switch sync.status {
        case .unavailable: "iCloud Drive is off on this iPhone"
        case .waiting: "Ready. No notes from your Mac yet"
        case .fetching: "Getting changes…"
        case .upToDate: "Up to date"
        case .needsUpdate: "Update Rune on this iPhone to read the new changes"
        case let .failed(message): "Sync stopped. \(message)"
        }
    }

    private var wifiLine: String {
        if let name = sync.connectingTo {
            return "Connecting to \(name)…"
        }
        let name = sync.mac?.name ?? "your Mac"
        return switch sync.wifi {
        case .off: "To connect, show the code for an iPhone in Rune on your Mac. Then point the camera of this iPhone at it."
        case .away: "Waiting for \(name). It syncs when this iPhone and the Mac are on the same Wi-Fi and Rune is open on the Mac."
        case .fetching: "Getting changes…"
        case .upToDate: "Up to date"
        case .needsUpdate: "Update Rune on this iPhone to read the new changes"
        case .refused: "\(name) no longer knows this iPhone. Connect again with a new code."
        case .notTheMac: "Another computer answers in place of \(name). Connect again with a new code."
        case let .failed(message): "Sync stopped. \(message)"
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                Text("Settings")
                    .font(.system(size: 28, weight: .bold))
                    .foregroundStyle(Theme.text)
                    .accessibilityAddTraits(.isHeader)

                VStack(alignment: .leading, spacing: 8) {
                    Text("Sync").sectionHeading()
                    VStack(spacing: 0) {
                        row("iCloud Drive", cloudLine)
                        RowSeparator()
                        row(sync.mac.map { "\($0.name) over Wi-Fi" } ?? "Your Mac over Wi-Fi", wifiLine)
                        if let mac = sync.mac {
                            RowSeparator()
                            actionRow("Disconnect from \(mac.name)…") { disconnecting = true }
                                .confirmationDialog("Disconnect from \(mac.name)?", isPresented: $disconnecting, titleVisibility: .visible) {
                                    Button("Disconnect", role: .destructive) {
                                        Task { await sync.disconnect() }
                                    }
                                } message: {
                                    Text("The notes stay on this iPhone.")
                                }
                        }
                        if !sync.devices.isEmpty {
                            RowSeparator()
                            row("Devices", sync.devices.map(\.name).joined(separator: ", "))
                        }
                    }
                    .card()
                    Text("Your pages stay on your devices. They sync through your own iCloud account, or straight from your Mac over your Wi-Fi, and work offline. This iPhone cannot change them yet.")
                        .font(.system(size: 12.5))
                        .foregroundStyle(Theme.text2)
                        .padding(.horizontal, 4)
                }

                VStack(alignment: .leading, spacing: 8) {
                    Text("Notes").sectionHeading()
                    VStack(spacing: 0) {
                        row(countLine, model.lastChange.map { "Last change: \($0.formatted(date: .abbreviated, time: .shortened))" } ?? "")
                        // A copy loaded by hand would be replaced by sync at once.
                        if !sync.hasNotes {
                            RowSeparator()
                            actionRow("Load a copy from your Mac…") { picking = true }
                        }
                    }
                    .card()
                }

                VStack(alignment: .leading, spacing: 8) {
                    Text("Appearance").sectionHeading()
                    Picker("Theme", selection: $appearance) {
                        Text("System").tag("system")
                        Text("Light").tag("light")
                        Text("Dark").tag("dark")
                    }
                    .pickerStyle(.segmented)
                }
            }
            .padding(.horizontal, 20)
            .padding(.top, 30)
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .workspaceImporter(isPresented: $picking)
    }

    private func row(_ title: String, _ detail: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(Theme.text)
            if !detail.isEmpty {
                Text(detail)
                    .font(.system(size: 12.5))
                    .foregroundStyle(Theme.text2)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    /// A row that does something when it is tapped.
    private func actionRow(_ title: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title)
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(Theme.accent)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .frame(height: 50)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private var countLine: String {
        let pages = model.workspace.pages.values.filter { !$0.trashed }.count
        let events = model.workspace.events.count
        return [pages == 1 ? "1 page" : "\(pages) pages", events == 1 ? "1 event" : "\(events) events"].joined(separator: " · ")
    }
}

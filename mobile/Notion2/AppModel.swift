// What the app knows: the workspace it shows and where its database is.
//
// The app keeps its own database in its private folder and never opens a file
// where it is. The notes get there in one of two ways: sync brings them from
// the Mac, through iCloud Drive or straight over the local network
// (`SyncCoordinator`), or a backup file is loaded by hand. Notes that came
// from sync can be changed here (`NotesWriter`); sync then sends the change.

import Foundation
import Notion2Core
import Observation

/// One thing at a time touches the app's database file: a read, a sync run,
/// or a copy loaded by hand. All of them run here, off the main thread.
actor StoreGate {
    static let shared = StoreGate()

    func run<T: Sendable>(_ body: @Sendable () throws -> T) rethrows -> T {
        try body()
    }
}

@Observable
final class AppModel {
    enum Phase {
        case loading
        /// There are no notes on the phone yet.
        case empty
        case ready
    }

    private(set) var phase: Phase = .loading
    private(set) var workspace = Workspace.empty
    private(set) var index = SearchIndex.empty
    /// Set when a file could not be loaded; shown once.
    var loadError: String?
    /// Changes each time a copy is loaded by hand, so screens start again.
    private(set) var generation = 0
    /// The notes came from sync and are not from a newer Rune: they can be
    /// changed here.
    private(set) var canEdit = false
    /// Why the last change could not be saved; shown once.
    var saveError: String?
    /// Called after a change is saved (sync sends it soon).
    @ObservationIgnored var onChange: () -> Void = {}

    var spaceID: ID = "" {
        didSet { UserDefaults.standard.set(spaceID, forKey: "space") }
    }

    let when = When()

    /// The newest change in the notes: how fresh they are.
    var lastChange: Date? {
        let newest = max(
            workspace.pages.values.map(\.updatedAt).max() ?? 0,
            workspace.events.map(\.updatedAt).max() ?? 0
        )
        return newest > 0 ? When.date(newest) : nil
    }

    var space: Space? {
        workspace.spaces.first { $0.id == spaceID } ?? workspace.spaces.first
    }

    /// The app's own database.
    nonisolated static var storeURL: URL {
        let folder = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Workspace", isDirectory: true)
        return folder.appendingPathComponent("workspace.db")
    }

    /// Show the notes the app has, if it has any.
    func start() async {
        let url = AppModel.storeURL
        guard FileManager.default.fileExists(atPath: url.path) else {
            phase = .empty
            return
        }
        do {
            show(try await AppModel.read(url), restart: true)
        } catch {
            phase = .empty
            loadError = error.localizedDescription
        }
    }

    /// Show the notes again after sync changed them. The user stays where
    /// they are.
    func reload() async {
        let url = AppModel.storeURL
        guard FileManager.default.fileExists(atPath: url.path), let loaded = try? await AppModel.read(url) else { return }
        show(loaded, restart: phase != .ready)
    }

    /// Load a copy of the workspace file at `source` (from the Files picker,
    /// AirDrop or "Open in") in place of the app's notes. They are only
    /// replaced when the new file reads as a workspace.
    func loadCopy(from source: URL) async {
        do {
            show(try await AppModel.bringIn(source), restart: true)
        } catch {
            loadError = error.localizedDescription
            if phase == .loading {
                phase = .empty
            }
        }
    }

    private func show(_ loaded: Loaded, restart: Bool) {
        workspace = loaded.workspace
        index = loaded.index
        canEdit = loaded.canEdit
        let wanted = workspace.spaces.contains { $0.id == spaceID } ? spaceID : (UserDefaults.standard.string(forKey: "space") ?? "")
        let space = workspace.spaces.contains { $0.id == wanted } ? wanted : (workspace.spaces.first?.id ?? "")
        if space != spaceID {
            spaceID = space
        }
        if restart {
            generation += 1
        }
        phase = .ready
    }

    // MARK: Off the main thread

    struct Loaded: Sendable {
        let workspace: Workspace
        let index: SearchIndex
        let canEdit: Bool
    }

    private nonisolated static func load(_ url: URL) throws -> Loaded {
        let workspace = try WorkspaceDatabase.load(from: url)
        let editable = url == storeURL && NotesWriter.canEdit(store: url, deviceID: SyncCoordinator.deviceID)
        return Loaded(workspace: workspace, index: SearchIndex(workspace), canEdit: editable)
    }

    // MARK: Changes made here

    /// Save a change, show the notes as they are now, and let sync send it.
    private func change(_ body: @escaping @Sendable (URL, String) throws -> Void) async {
        let url = AppModel.storeURL
        let device = SyncCoordinator.deviceID
        do {
            let loaded = try await StoreGate.shared.run { () throws -> Loaded in
                try body(url, device)
                return try AppModel.load(url)
            }
            show(loaded, restart: false)
            onChange()
        } catch {
            saveError = error.localizedDescription
        }
    }

    func save(event: Event) async {
        await change { try NotesWriter.save(event: event, store: $0, deviceID: $1) }
    }

    func remove(event id: ID) async {
        await change { try NotesWriter.remove(event: id, store: $0, deviceID: $1) }
    }

    func setChecked(_ checked: Bool, block: ID, page: ID) async {
        await change { try NotesWriter.setChecked(checked, block: block, page: page, store: $0, deviceID: $1) }
    }

    /// A new page at the end of the top level of the space, with one empty
    /// line. Returns its id once it is saved.
    func newPage() async -> ID? {
        let last = workspace.roots(in: spaceID).map(\.position).max() ?? 0
        let now = When.ms(Date())
        let page = Page(id: SyncFormat.newRecordID(), spaceID: spaceID, position: last + 1, createdAt: now, updatedAt: now)
        await save(page: page, blocks: [Block(id: SyncFormat.newRecordID(), kind: .text)])
        return workspace.pages[page.id] == nil ? nil : page.id
    }

    func save(page: Page, blocks: [Block]) async {
        await change { try NotesWriter.save(page: page, blocks: blocks, store: $0, deviceID: $1) }
    }

    private nonisolated static func read(_ url: URL) async throws -> Loaded {
        try await StoreGate.shared.run { try load(url) }
    }

    private nonisolated static func bringIn(_ source: URL) async throws -> Loaded {
        try await StoreGate.shared.run {
            let files = FileManager.default
            let scoped = source.startAccessingSecurityScopedResource()
            defer {
                if scoped {
                    source.stopAccessingSecurityScopedResource()
                }
                // A file handed over by "Open in" is a copy in the app's Inbox:
                // it is not needed again, whether or not it could be read.
                if source.deletingLastPathComponent().lastPathComponent == "Inbox" {
                    try? files.removeItem(at: source)
                }
            }

            let store = storeURL
            let folder = store.deletingLastPathComponent()
            try files.createDirectory(at: folder, withIntermediateDirectories: true)
            let incoming = folder.appendingPathComponent("incoming-\(UUID().uuidString).db")
            defer { try? files.removeItem(at: incoming) }

            // Copy through a file coordinator, so a file in iCloud Drive is
            // fetched first and is not read while it changes.
            var coordinationError: NSError?
            var copyError: Error?
            NSFileCoordinator().coordinate(readingItemAt: source, options: .withoutChanges, error: &coordinationError) { url in
                do {
                    try files.copyItem(at: url, to: incoming)
                } catch {
                    copyError = error
                }
            }
            if let error = coordinationError ?? copyError {
                throw error
            }

            let loaded = try load(incoming)
            try? files.removeItem(at: URL(fileURLWithPath: store.path + "-journal"))
            if files.fileExists(atPath: store.path) {
                _ = try files.replaceItemAt(store, withItemAt: incoming)
            } else {
                try files.moveItem(at: incoming, to: store)
            }
            return loaded
        }
    }
}

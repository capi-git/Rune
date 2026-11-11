// Sync over the local network (`docs/sync-wifi.md`): the link in the QR code,
// the names of the files that travel, which files the iPhone fetches, and a
// whole run against the stand-in Mac (`mobile/scripts/wifi-mac.py`).

import Foundation
import Testing
@testable import Notion2Core

private let mac = String(repeating: "a", count: 32)
private let secondMac = String(repeating: "b", count: 32)
private let phone = String(repeating: "c", count: 32)
private let cert = String(repeating: "0123456789abcdef", count: 4)

@Suite struct PairingLinkTests {
    @Test func aLinkFromTheMacIsRead() throws {
        let url = URL(string: "notion2://pair?v=1&host=Jacks-MacBook-Pro.local&host=192.168.1.20&port=47815&cert=\(cert.uppercased())&code=abcdefghijklmnop_-12&name=MacBook%20Pro")!
        let link = try PairingLink(url)
        #expect(link.hosts == ["Jacks-MacBook-Pro.local", "192.168.1.20"])
        #expect(link.port == 47815)
        #expect(link.fingerprint == cert)
        #expect(link.code == "abcdefghijklmnop_-12")
        #expect(link.name == "MacBook Pro")
        // Written and read again, it is the same link.
        #expect(try PairingLink(link.url) == link)
    }

    @Test func aLinkWithNoNameOrVersionStillWorks() throws {
        let link = try PairingLink(URL(string: "notion2://pair?host=localhost&port=1&cert=\(cert)&code=abcdefghijklmnop")!)
        #expect(link.name == "Mac")
    }

    @Test func otherLinksAreNotPairingLinks() {
        #expect(throws: PairingLink.Problem.notALink) { try PairingLink(URL(string: "notion2://page/abc")!) }
        #expect(throws: PairingLink.Problem.notALink) { try PairingLink(URL(string: "https://pair?host=a&port=1")!) }
        #expect(!PairingLink.isOne(URL(fileURLWithPath: "/tmp/workspace.db")))
        #expect(PairingLink.isOne(URL(string: "NOTION2://pair?x=1")!))
    }

    @Test func aLinkOfALaterVersionAsksForAnUpdate() {
        #expect(throws: PairingLink.Problem.newer) {
            try PairingLink(URL(string: "notion2://pair?v=2&host=localhost&port=1&cert=\(cert)&code=abcdefghijklmnop")!)
        }
    }

    @Test(arguments: [
        "notion2://pair?port=47815&cert=\(cert)&code=abcdefghijklmnop",                       // no host
        "notion2://pair?host=a/b&port=47815&cert=\(cert)&code=abcdefghijklmnop",              // a host with a path in it
        "notion2://pair?host=evil.example%40x&port=47815&cert=\(cert)&code=abcdefghijklmnop", // a host with "@"
        "notion2://pair?host=localhost&port=0&cert=\(cert)&code=abcdefghijklmnop",            // no such port
        "notion2://pair?host=localhost&port=70000&cert=\(cert)&code=abcdefghijklmnop",
        "notion2://pair?host=localhost&port=47815&cert=abc&code=abcdefghijklmnop",            // not a fingerprint
        "notion2://pair?host=localhost&port=47815&cert=\(cert)&code=short",                   // a code too easy to guess
        "notion2://pair?host=localhost&port=47815&cert=\(cert)&code=abcdefghijklmnop%20x",    // a code with a space
    ])
    func aLinkThatIsNotCompleteIsRefused(_ text: String) {
        #expect(throws: PairingLink.Problem.incomplete) { try PairingLink(URL(string: text)!) }
    }
}

@Suite struct SyncPathTests {
    @Test func theFourKindsOfFile() {
        #expect(SyncPath("devices/\(mac).json") == .device(mac))
        #expect(SyncPath("snapshots/\(mac)/0000000012.db") == .snapshot(device: mac, seq: 12))
        #expect(SyncPath("changes/\(mac)/0000000013.json") == .change(device: mac, seq: 13))
        #expect(SyncPath("files/5a3b6b0a1e9b3281740d.png") == .picture("5a3b6b0a1e9b3281740d.png"))
        #expect(SyncPath.change(device: mac, seq: 13).path == "changes/\(mac)/0000000013.json")
        #expect(SyncPath.change(device: mac, seq: 13).owner == mac)
        #expect(SyncPath.picture("a.png").owner == nil)
    }

    @Test(arguments: [
        "",
        "workspace.db",
        "devices/../../etc/passwd",
        "files/../devices/\(String(repeating: "a", count: 32)).json",
        "files/.hidden.png",
        "files/a.png/b.png",
        "files/script.sh",
        "/files/a.png",
        "files//a.png",
        "devices/\(String(repeating: "A", count: 32)).json",     // not lower case
        "devices/\(String(repeating: "a", count: 31)).json",     // too short
        "devices/\(String(repeating: "a", count: 32)).txt",
        "changes/\(String(repeating: "a", count: 32))/13.json",  // not ten digits
        "changes/\(String(repeating: "a", count: 32))/0000000000.json",
        "changes/\(String(repeating: "a", count: 32))/0000000013.db",
        "snapshots/\(String(repeating: "a", count: 32))/0000000012.db/x",
        "snapshots/\(String(repeating: "a", count: 32))/.0000000012.db.icloud",
    ])
    func anyOtherNameIsRefused(_ path: String) {
        #expect(SyncPath(path) == nil)
    }
}

@Suite struct WifiPlanTests {
    private func file(_ path: SyncPath, _ size: Int64 = 10) -> WifiFile {
        WifiFile(path: path.path, size: size)
    }

    /// The Mac after a while: a first snapshot, changes, a newer snapshot,
    /// more changes, and a picture.
    private var remote: [WifiFile] {
        [
            file(.snapshot(device: mac, seq: 1), 5000),
            file(.change(device: mac, seq: 2)),
            file(.change(device: mac, seq: 3)),
            file(.snapshot(device: mac, seq: 4), 6000),
            file(.change(device: mac, seq: 5)),
            file(.picture("abc.png"), 700),
            WifiFile(path: "devices/../../outside", size: 1),
        ]
    }

    @Test func aNewPhoneTakesTheNewestSnapshotAndEveryChange() {
        let plan = WifiMirror.plan(remote: remote, local: [], marked: [], this: phone, applied: nil)
        #expect(plan.downloads.map(\.path) == [
            "snapshots/\(mac)/0000000004.db",
            "changes/\(mac)/0000000002.json",
            "changes/\(mac)/0000000003.json",
            "changes/\(mac)/0000000005.json",
            "files/abc.png",
        ])
        // The older snapshot is listed, not fetched.
        #expect(plan.markers == ["snapshots/\(mac)/0000000001.db"])
        #expect(plan.removals.isEmpty)
        #expect(plan.uploads.isEmpty)
    }

    @Test func aPhoneThatIsUpToDateFetchesNothing() {
        let local = [file(.picture("abc.png"), 700)]
        let marked: Set = ["snapshots/\(mac)/0000000001.db", "snapshots/\(mac)/0000000004.db"]
        let plan = WifiMirror.plan(remote: remote, local: local, marked: marked, this: phone, applied: [mac: 5])
        #expect(plan == WifiPlan())
    }

    @Test func onlyTheChangesAfterTheLastOneAppliedAreFetched() {
        let local = [file(.change(device: mac, seq: 2)), file(.change(device: mac, seq: 3)), file(.picture("abc.png"), 700)]
        let plan = WifiMirror.plan(remote: remote, local: local, marked: [], this: phone, applied: [mac: 3])
        #expect(plan.downloads.map(\.path) == ["changes/\(mac)/0000000005.json"])
        // No file is missing between 3 and the snapshot at 4: it is not fetched.
        #expect(plan.markers == ["snapshots/\(mac)/0000000001.db", "snapshots/\(mac)/0000000004.db"])
        // What was applied is of no use in the copy any more.
        #expect(plan.removals == ["changes/\(mac)/0000000002.json", "changes/\(mac)/0000000003.json"])
    }

    @Test func aGapBeforeTheNewestSnapshotBringsThatSnapshot() {
        // The Mac removed its old change files; the phone was at 1.
        let pruned = remote.filter { $0.path != "changes/\(mac)/0000000002.json" && $0.path != "changes/\(mac)/0000000003.json" }
        let plan = WifiMirror.plan(remote: pruned, local: [file(.picture("abc.png"), 700)], marked: [], this: phone, applied: [mac: 1])
        #expect(plan.downloads.map(\.path) == ["snapshots/\(mac)/0000000004.db", "changes/\(mac)/0000000005.json"])
        #expect(plan.markers == ["snapshots/\(mac)/0000000001.db"])
    }

    @Test func aFileThatCameShortIsFetchedAgain() {
        let local = [file(.change(device: mac, seq: 5), 3), file(.picture("abc.png"), 1)]
        let marked: Set = ["snapshots/\(mac)/0000000001.db", "snapshots/\(mac)/0000000004.db"]
        let plan = WifiMirror.plan(remote: remote, local: local, marked: marked, this: phone, applied: [mac: 4])
        #expect(plan.downloads.map(\.path) == ["changes/\(mac)/0000000005.json", "files/abc.png"])
        #expect(plan.removals.isEmpty)
    }

    @Test func whatTheMacNoLongerListsLeavesTheCopy() {
        let local = [file(.snapshot(device: secondMac, seq: 9)), file(.change(device: secondMac, seq: 10)), file(.picture("old.png"))]
        let marked: Set = ["snapshots/\(secondMac)/0000000003.db"]
        let plan = WifiMirror.plan(remote: [], local: local, marked: marked, this: phone, applied: [mac: 5])
        #expect(plan.removals == [
            "changes/\(secondMac)/0000000010.json",
            "snapshots/\(secondMac)/.0000000003.db.icloud",
            "snapshots/\(secondMac)/0000000009.db",
        ])
        // A picture stays: a page may still name it.
        #expect(!plan.removals.contains("files/old.png"))
    }

    @Test func thisDevicesOwnFilesGoToTheMac() {
        let local = [file(.change(device: phone, seq: 1)), file(.change(device: phone, seq: 2)), file(.device(phone))]
        let plan = WifiMirror.plan(remote: [file(.change(device: phone, seq: 1))], local: local, marked: [], this: phone, applied: [:])
        #expect(plan.uploads == ["changes/\(phone)/0000000002.json"])
        #expect(plan.downloads.isEmpty)
        #expect(plan.removals.isEmpty)
    }

    @Test func theCopyOnDiskIsReadWithItsMarks() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("notion2-wifi-tests-\(UUID().uuidString)/sync-v1")
        defer { try? FileManager.default.removeItem(at: root.deletingLastPathComponent()) }
        var plan = WifiPlan()
        plan.markers = ["snapshots/\(mac)/0000000001.db"]
        let device = SyncDeviceFile(device: mac, name: "MacBook Pro", kind: "mac", at: 5, seq: 5)
        try WifiMirror.prepare(plan, devices: [device], in: root, this: phone)

        let fetched = root.deletingLastPathComponent().appendingPathComponent("fetched")
        try Data("12345".utf8).write(to: fetched)
        try WifiMirror.place(fetched, at: "changes/\(mac)/0000000002.json", in: root)
        try Data("not a sync file".utf8).write(to: root.appendingPathComponent("changes/\(mac)/notes.txt"))

        let here = WifiMirror.contents(of: root)
        #expect(here.files.map(\.path) == ["changes/\(mac)/0000000002.json", "devices/\(mac).json"])
        #expect(here.files.first?.size == 5)
        #expect(here.marked == ["snapshots/\(mac)/0000000001.db"])
        // The engine reads the mark as a file that is listed, but not here.
        let entries = SyncEngine.entries(in: root.appendingPathComponent("snapshots/\(mac)"), isHere: { _ in true })
        #expect(entries.map(\.name) == ["0000000001.db"])
        #expect(entries.first?.here == false)

        // A device the Mac no longer lists goes; a fetched snapshot loses its mark.
        try Data("db".utf8).write(to: fetched)
        try WifiMirror.place(fetched, at: "snapshots/\(mac)/0000000001.db", in: root)
        try WifiMirror.prepare(WifiPlan(), devices: [], in: root, this: phone)
        let after = WifiMirror.contents(of: root)
        #expect(after.marked.isEmpty)
        #expect(after.files.map(\.path) == ["changes/\(mac)/0000000002.json", "snapshots/\(mac)/0000000001.db"])
    }
}

#if os(macOS)
/// One thing at a time on the database, as the app's `StoreGate`.
private actor Gate: StoreAccess {
    func withStore<T: Sendable>(_ body: @Sendable () throws -> T) async throws -> T {
        try body()
    }
}

/// The stand-in Mac: a sync folder that `make-sync-folder.py` made, served by
/// `wifi-mac.py` (or the app's own server, see `command`) on a free port of
/// this computer.
private final class StandInMac {
    static let scripts = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("scripts")

    let base: URL
    var folder: URL { base.appendingPathComponent("icloud") }
    var mirror: URL { base.appendingPathComponent("phone/wifi/sync-v1") }
    var store: URL { base.appendingPathComponent("phone/workspace.db") }
    private var server: Process?
    private(set) var link: PairingLink!

    init() throws {
        base = FileManager.default.temporaryDirectory.appendingPathComponent("notion2-wifi-run-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: base.appendingPathComponent("phone"), withIntermediateDirectories: true)
        let source = try Fixture.make("""
            INSERT INTO spaces(id, name, color, position, created_at) VALUES ('home', 'My Space', 'orange', 1.0, 1);
            INSERT INTO pages(id, parent_id, title, icon, position, favorite, trashed, created_at, updated_at, space_id) VALUES
                ('plan', NULL, 'Plan', 'rocket:orange', 1.0, 0, 0, 1, 100, 'home');
            INSERT INTO blocks(id, page_id, position, kind, text) VALUES ('b1', 'plan', 0, 'text', 'First');
            """)
        defer { Fixture.remove(source) }
        // The name of a backup: the tool refuses the name of the live file.
        let backup = base.appendingPathComponent("backup.db")
        try FileManager.default.copyItem(at: source, to: backup)
        try StandInMac.python("make-sync-folder.py", "init", backup.path, folder.path, "Test Mac")
        try start()
    }

    deinit {
        server?.terminate()
        try? FileManager.default.removeItem(at: base)
    }

    @discardableResult
    static func python(_ script: String, _ arguments: String...) throws -> String {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["python3", scripts.appendingPathComponent(script).path] + arguments
        let out = Pipe()
        process.standardOutput = out
        process.standardError = out
        try process.run()
        let text = String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        process.waitUntilExit()
        guard process.terminationStatus == 0 else { throw WifiError.failed("\(script): \(text)") }
        return text
    }

    /// As the Mac would: change the notes and write a change file.
    func change(_ arguments: String...) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["python3", StandInMac.scripts.appendingPathComponent("make-sync-folder.py").path, arguments[0], folder.path] + arguments.dropFirst()
        process.standardOutput = Pipe()
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else { throw WifiError.failed("make-sync-folder.py \(arguments)") }
    }

    /// The command of the Mac's own server (the Rust one in the app) in
    /// place of `wifi-mac.py`, run by `/bin/sh` with `RUNE_STANDIN_FOLDER`
    /// and `RUNE_STANDIN_PORT` set; it prints the same `LINK …` line and ends
    /// when its parent does. From the repository's root, after `cargo test
    /// --no-run` (or give the test binary itself in place of `cargo test`):
    ///
    ///     RUNE_STANDIN_MAC_CMD="cargo test --manifest-path $PWD/Cargo.toml --bin notion2 -q -- --ignored --exact devsync::wifi::tests::stand_in_mac --nocapture"
    static let command = ProcessInfo.processInfo.environment["RUNE_STANDIN_MAC_CMD"].flatMap { $0.isEmpty ? nil : $0 }

    /// Start the server (again): it then shows a new code.
    func start() throws {
        server?.terminate()
        server?.waitUntilExit()
        let process = Process()
        var environment = ProcessInfo.processInfo.environment.merging(["QUIET": "1"]) { _, new in new }
        if let command = StandInMac.command {
            process.executableURL = URL(fileURLWithPath: "/bin/sh")
            process.arguments = ["-c", "exec \(command)"]
            environment["RUNE_STANDIN_FOLDER"] = folder.path
            environment["RUNE_STANDIN_PORT"] = "0"
        } else {
            process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
            process.arguments = ["python3", "-u", StandInMac.scripts.appendingPathComponent("wifi-mac.py").path, folder.path, "--port", "0"]
        }
        process.environment = environment
        let out = Pipe()
        process.standardOutput = out
        process.standardError = FileHandle.nullDevice
        try process.run()
        server = process
        // It prints the link of the QR code on a line of its own (the Rust
        // one after the test runner's own first line).
        var text = ""
        var found: Substring?
        while found == nil {
            let data = out.fileHandleForReading.availableData
            if data.isEmpty { break }
            text += String(decoding: data, as: UTF8.self)
            found = text.split(separator: "\n").dropLast(text.hasSuffix("\n") ? 0 : 1).first { $0.hasPrefix("LINK ") }
        }
        guard let line = found, let url = URL(string: String(line.dropFirst(5))) else {
            throw WifiError.failed("The stand-in Mac did not start: \(text)")
        }
        link = try PairingLink(url)
    }

    func titles() throws -> [String] {
        try WorkspaceDatabase.load(from: store).pages.values.map(\.title).sorted()
    }

    /// The blocks of the one page of the notes.
    func blocks() throws -> [Block] {
        try WorkspaceDatabase.load(from: store).blocks.values.first ?? []
    }

    func run(_ client: WifiClient) async throws -> SyncReport {
        try await WifiSync.run(client: client, mirror: mirror, store: store, deviceID: phone, deviceName: "Test iPhone", gate: Gate())
    }
}

@Suite(.serialized) struct WifiRunTests {
    @Test func aPhonePairsGetsTheNotesAndThenTheChanges() async throws {
        let scene = try StandInMac()
        #expect(scene.link.name == "Test Mac")
        let (peer, token) = try await WifiClient.pair(scene.link, deviceID: phone, deviceName: "Test iPhone")
        #expect(peer.name == "Test Mac")
        #expect(SyncPath.isDeviceID(peer.device))
        let client = WifiClient(peer: peer, token: token)

        // The first run starts from the Mac's snapshot.
        let first = try await scene.run(client)
        #expect(first.adoptedSnapshot)
        #expect(first.isSynced)
        #expect(try scene.titles() == ["Plan"])
        #expect(first.devices.map(\.name) == ["Test Mac", "Test iPhone"])
        // The copy does not keep a second set of the notes.
        let kept = WifiMirror.contents(of: scene.mirror)
        #expect(!kept.files.contains { $0.path.hasPrefix("snapshots/") })
        #expect(kept.marked.count == 1)
        // The Mac now knows how far the phone is.
        let list = try await client.list()
        #expect(list.devices.first { $0.device == phone }?.applied == [peer.device: 1])
        #expect(FileManager.default.fileExists(atPath: scene.folder.appendingPathComponent("sync-v1/devices/\(phone).json").path))

        // Nothing new: nothing happens.
        let idle = try await scene.run(client)
        #expect(!idle.changed)
        #expect(idle.isSynced)

        // The Mac changes a page and adds one to-do: the phone gets both.
        try scene.change("rename", "Plan", "The plan")
        try scene.change("todo", "The plan", "Call Sam")
        let second = try await scene.run(client)
        #expect(second.applied == 2)
        #expect(!second.adoptedSnapshot)
        #expect(try scene.titles() == ["The plan"])
        #expect(try scene.blocks().map(\.text) == ["First", "Call Sam"])
        // Applied change files leave the copy at the next run.
        _ = try await scene.run(client)
        #expect(!WifiMirror.contents(of: scene.mirror).files.contains { $0.path.hasPrefix("changes/") })

        // A due day and a page's font come through as the Mac stores them.
        try scene.change("todo", "The plan", "Pay rent", "2026-11-12")
        try scene.change("font", "The plan", "serif")
        #expect(try await scene.run(client).applied == 2)
        let db = try Connection(url: scene.store)
        defer { db.close() }
        var stored: [String] = []
        try db.each("SELECT p.font || '|' || b.text || '|' || b.due FROM blocks b JOIN pages p ON p.id = b.page_id ORDER BY b.position") { stored.append($0.text(0)) }
        #expect(stored == ["serif|First|", "serif|Call Sam|", "serif|Pay rent|2026-11-12"])
    }

    @Test func anotherMacStartsTheNotesAgain() async throws {
        // The phone has the notes of one Mac.
        let first = try StandInMac()
        let (firstMac, firstToken) = try await WifiClient.pair(first.link, deviceID: phone, deviceName: "Test iPhone")
        _ = try await first.run(WifiClient(peer: firstMac, token: firstToken))
        #expect(try first.titles() == ["Plan"])
        // The same Mac again: nothing starts again.
        #expect(try SyncEngine.startAgain(unlessSyncedWith: firstMac.device, store: first.store, deviceID: phone) == false)

        // Then it is paired with a Mac it has no notes from.
        let second = try StandInMac()
        try second.change("rename", "Plan", "The other Mac's plan")
        let (secondMac, secondToken) = try await WifiClient.pair(second.link, deviceID: phone, deviceName: "Test iPhone")
        #expect(secondMac.device != firstMac.device)
        #expect(try SyncEngine.startAgain(unlessSyncedWith: secondMac.device, store: first.store, deviceID: phone))
        // The app removes the copy of the first Mac's folder; the database stays.
        let report = try await WifiSync.run(
            client: WifiClient(peer: secondMac, token: secondToken), mirror: second.mirror, store: first.store,
            deviceID: phone, deviceName: "Test iPhone", gate: Gate()
        )
        #expect(report.adoptedSnapshot)
        #expect(try first.titles() == ["The other Mac's plan"])
        // A copy loaded by hand has no sync state: nothing to start again.
        #expect(try SyncEngine.startAgain(unlessSyncedWith: secondMac.device, store: second.base.appendingPathComponent("backup.db"), deviceID: phone) == false)
    }

    @Test func aPictureComesWithTheNotes() async throws {
        let scene = try StandInMac()
        let picture = scene.base.appendingPathComponent("photo.png")
        try Data(repeating: 7, count: 3000).write(to: picture)
        try scene.change("picture", "Plan", picture.path)
        let (peer, token) = try await WifiClient.pair(scene.link, deviceID: phone, deviceName: "Test iPhone")
        _ = try await scene.run(WifiClient(peer: peer, token: token))

        let name = try #require(try scene.blocks().last { $0.kind == .image }?.data)
        let here = scene.mirror.appendingPathComponent("files/\(name)")
        #expect(try Data(contentsOf: here) == Data(repeating: 7, count: 3000))
    }

    @Test func theCodeWorksOnce() async throws {
        let scene = try StandInMac()
        _ = try await WifiClient.pair(scene.link, deviceID: phone, deviceName: "Test iPhone")
        await #expect(throws: WifiError.badCode) {
            _ = try await WifiClient.pair(scene.link, deviceID: secondMac, deviceName: "Another iPhone")
        }
        var wrong = scene.link!
        wrong.code = "aaaaaaaaaaaaaaaaaaaaaa"
        await #expect(throws: WifiError.badCode) {
            _ = try await WifiClient.pair(wrong, deviceID: phone, deviceName: "Test iPhone")
        }
    }

    @Test func onlyThePairedMacAndThePairedPhoneGetThrough() async throws {
        let scene = try StandInMac()
        let (peer, token) = try await WifiClient.pair(scene.link, deviceID: phone, deviceName: "Test iPhone")

        // Another computer at the same address has another certificate.
        var other = peer
        other.fingerprint = cert
        await #expect(throws: WifiError.notTheMac) { _ = try await WifiClient(peer: other, token: token).list() }

        // A token the Mac does not know.
        await #expect(throws: WifiError.refused) { _ = try await WifiClient(peer: peer, token: String(repeating: "x", count: 43)).list() }

        // After "Disconnect" on the phone, its token is of no use.
        let client = WifiClient(peer: peer, token: token)
        _ = try await client.list()
        try await client.unpair()
        await #expect(throws: WifiError.refused) { _ = try await client.list() }

        // Nothing listens at the next address: the Mac is not there.
        var away = peer
        away.hosts = ["127.0.0.1"]
        away.port = 9
        await #expect(throws: WifiError.unreachable) { _ = try await WifiClient(peer: away, token: token).list() }
    }

    @Test func aPhoneWritesOnlyInItsOwnPlaces() async throws {
        let scene = try StandInMac()
        let (peer, token) = try await WifiClient.pair(scene.link, deviceID: phone, deviceName: "Test iPhone")
        let client = WifiClient(peer: peer, token: token)
        let file = scene.base.appendingPathComponent("file.json")
        try Data("{}".utf8).write(to: file)

        try await client.upload("changes/\(phone)/0000000001.json", from: file)
        #expect(FileManager.default.fileExists(atPath: scene.folder.appendingPathComponent("sync-v1/changes/\(phone)/0000000001.json").path))
        // The Mac's own files cannot be written from outside.
        await #expect(throws: WifiError.forbidden) { try await client.upload("changes/\(peer.device)/0000000099.json", from: file) }
        await #expect(throws: WifiError.forbidden) { try await client.upload("devices/\(peer.device).json", from: file) }
        // A name outside the four kinds never leaves the phone.
        await #expect(throws: WifiError.self) { try await client.upload("../outside.json", from: file) }
    }

    @Test func aSecondAddressIsTriedWhenTheFirstDoesNotAnswer() async throws {
        let scene = try StandInMac()
        var link = scene.link!
        // The Mac's name is not found on this network; its address is.
        link.hosts = ["no-such-mac-\(UUID().uuidString.prefix(8)).invalid", "127.0.0.1"]
        let (peer, token) = try await WifiClient.pair(link, deviceID: phone, deviceName: "Test iPhone")
        let report = try await scene.run(WifiClient(peer: peer, token: token))
        #expect(report.isSynced)
    }
}
#endif

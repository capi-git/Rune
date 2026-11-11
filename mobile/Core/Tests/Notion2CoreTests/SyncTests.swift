// Sync from the shared folder (`docs/sync-format.md`): a snapshot to start
// from, change files in order, and a record changed in two places at once.

import Foundation
import Testing
@testable import Notion2Core

private let mac = String(repeating: "a", count: 32)
private let secondMac = String(repeating: "b", count: 32)
private let phone = String(repeating: "c", count: 32)

/// A sync folder and a phone's database, in a folder of their own.
private final class Scene {
    let base: URL
    var root: URL { base.appendingPathComponent("icloud/sync-v1") }
    var store: URL { base.appendingPathComponent("app/workspace.db") }

    init() throws {
        base = FileManager.default.temporaryDirectory.appendingPathComponent("notion2-sync-tests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: store.deletingLastPathComponent(), withIntermediateDirectories: true)
    }

    deinit {
        try? FileManager.default.removeItem(at: base)
    }

    /// The notes every test starts from, as the Mac's snapshot.
    static let notes = """
        INSERT INTO spaces(id, name, color, position, created_at) VALUES ('home', 'My Space', 'orange', 1.0, 1);
        INSERT INTO pages(id, parent_id, title, icon, position, favorite, trashed, created_at, updated_at, space_id) VALUES
            ('plan', NULL, 'Plan', 'rocket:orange', 1.0, 0, 0, 1, 100, 'home'),
            ('old', NULL, 'Old', NULL, 2.0, 0, 0, 1, 100, 'home');
        INSERT INTO blocks(id, page_id, position, kind, text) VALUES
            ('b1', 'plan', 0, 'text', 'First'),
            ('b2', 'plan', 1, 'todo', 'Second');
        INSERT INTO events(id, space_id, title, start_ms, end_ms) VALUES ('standup', 'home', 'Standup', 1000, 2000);
        INSERT INTO settings(key, value) VALUES ('theme', 'dark');
        INSERT INTO sync_records(kind, id, clock, device, synced_clock, synced_device) VALUES
            ('space', 'home', 1, '\(mac)', 1, '\(mac)'),
            ('page', 'plan', 2, '\(mac)', 2, '\(mac)'),
            ('page', 'old', 3, '\(mac)', 3, '\(mac)'),
            ('event', 'standup', 4, '\(mac)', 4, '\(mac)');
        """

    func addSnapshot(device: String = mac, seq: Int = 1, clock: Int64 = 4, schema: String = Fixture.schema, extra: String = "") throws {
        let made = try Fixture.make(
            schema: schema + SyncFormat.tables,
            Scene.notes + extra + "INSERT INTO sync_meta(key, value) VALUES ('device', '\(device)'), ('clock', '\(clock)'), ('seq', '\(seq)');"
        )
        defer { Fixture.remove(made) }
        let folder = root.appendingPathComponent("snapshots/\(device)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try FileManager.default.moveItem(at: made, to: folder.appendingPathComponent("\(SyncFormat.name(ofSeq: seq)).db"))
    }

    func changeURL(_ device: String, _ seq: Int) -> URL {
        root.appendingPathComponent("changes/\(device)/\(SyncFormat.name(ofSeq: seq)).json")
    }

    func addChanges(device: String = mac, seq: Int, format: Int = 1, _ changes: [SyncChange]) throws {
        let url = changeURL(device, seq)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let file = SyncChangeFile(format: format, device: device, seq: seq, at: 0, changes: changes)
        try JSONEncoder().encode(file).write(to: url)
    }

    @discardableResult
    func pull(isHere: (URL) -> Bool = { _ in true }) throws -> SyncReport {
        try SyncEngine.pull(root: root, store: store, deviceID: phone, deviceName: "iPhone", now: 5000, isHere: isHere)
    }

    func notes() throws -> Workspace {
        try WorkspaceDatabase.load(from: store)
    }

    /// One column of a query on the phone's database.
    func column(_ sql: String) throws -> [String] {
        let db = try Connection(url: store)
        defer { db.close() }
        var out: [String] = []
        try db.each(sql) { out.append($0.text(0)) }
        return out
    }
}

private func version(_ clock: Int64, _ device: String = mac) -> SyncVersion {
    SyncVersion(clock: clock, device: device)
}

private func planChange(_ v: SyncVersion, base: SyncVersion?, title: String, updatedAt: Int64 = 200) -> SyncChange {
    SyncChange(
        kind: "page", id: "plan", version: v, base: base,
        page: SyncPage(
            spaceID: "home", title: title, icon: "rocket:orange", position: 1, createdAt: 1, updatedAt: updatedAt,
            blocks: [SyncBlock(id: "b1", kind: "heading1", text: title), SyncBlock(id: "b9", kind: "page", target: "old")]
        )
    )
}

@Suite struct SyncTests {
    @Test func startsFromTheNewestSnapshot() throws {
        let scene = try Scene()
        #expect(try scene.pull().isSynced == false, "nothing to start from yet")
        #expect(!FileManager.default.fileExists(atPath: scene.store.path))

        try scene.addSnapshot()
        let report = try scene.pull()
        #expect(report.adoptedSnapshot && report.isSynced && report.changed && !report.waiting)

        let notes = try scene.notes()
        #expect(notes.pages.keys.sorted() == ["old", "plan"])
        #expect(notes.blocks["plan"]?.map(\.text) == ["First", "Second"])
        #expect(notes.events.map(\.title) == ["Standup"])
        // The database is this device's now, and goes on after the snapshot.
        #expect(try scene.column("SELECT value FROM sync_meta WHERE key = 'device'") == [phone])
        #expect(try scene.column("SELECT device || ':' || seq FROM sync_peers") == ["\(mac):1"])
        // The snapshot file itself is not touched, and nothing else is in the app's folder.
        #expect(Fixture.files(beside: scene.store) == ["workspace.db"])

        let again = try scene.pull()
        #expect(!again.changed && again.isSynced)
    }

    @Test func takesTheSnapshotWithTheNewestNotes() throws {
        let scene = try Scene()
        try scene.addSnapshot(device: mac, seq: 1, clock: 4)
        try scene.addSnapshot(device: mac, seq: 9, clock: 30, extra: "UPDATE pages SET title = 'Plan, later' WHERE id = 'plan';")
        try scene.addSnapshot(device: secondMac, seq: 2, clock: 12, extra: "UPDATE pages SET title = 'Plan, second Mac' WHERE id = 'plan';")
        try scene.pull()
        #expect(try scene.notes().pages["plan"]?.title == "Plan, later")
        #expect(try scene.column("SELECT seq FROM sync_peers WHERE device = '\(mac)'") == ["9"])
    }

    @Test func appliesChangeFilesInOrder() throws {
        let scene = try Scene()
        try scene.addSnapshot()
        try scene.addChanges(seq: 2, [
            planChange(version(5), base: version(2), title: "Plan A"),
            SyncChange(kind: "space", id: "work", version: version(6), base: nil, space: SyncSpace(name: "Work", color: "green", position: 2)),
        ])
        try scene.addChanges(seq: 3, [
            planChange(version(7), base: version(5), title: "Plan B"),
            SyncChange(
                kind: "event", id: "sync", version: version(8), base: nil,
                event: SyncEvent(spaceID: "home", title: "Team sync", startMS: 3000, endMS: 4000, remindMin: 10, repeatRule: #"{"every":"week","interval":1}"#)
            ),
        ])
        let report = try scene.pull()
        #expect(report.applied == 4 && report.conflicts == 0 && !report.waiting)

        let notes = try scene.notes()
        #expect(notes.pages["plan"]?.title == "Plan B")
        #expect(notes.pages["plan"]?.updatedAt == 200)
        #expect(notes.blocks["plan"]?.map(\.kind) == [.heading1, .pageLink], "the page's blocks are replaced, in order")
        #expect(notes.blocks["plan"]?[1].target == "old")
        #expect(notes.spaces.map(\.name) == ["My Space", "Work"])
        #expect(notes.events.map(\.title) == ["Standup", "Team sync"])
        #expect(notes.events[1].repeatRule == Repeat(every: .week) && notes.events[1].remind == 10)
        #expect(try scene.column("SELECT clock FROM sync_records WHERE id = 'plan'") == ["7"])
        #expect(try scene.column("SELECT value FROM sync_meta WHERE key = 'clock'") == ["8"])
        #expect(try scene.column("SELECT count(*) FROM page_versions") == ["0"], "a plain update makes no history")

        // The same files again change nothing.
        #expect(try scene.pull().applied == 0)
    }

    @Test func tellsTheOtherDevicesHowFarItIs() throws {
        let scene = try Scene()
        try scene.addSnapshot()
        try scene.addChanges(seq: 2, [planChange(version(5), base: version(2), title: "Plan A")])
        let macFile = SyncDeviceFile(device: mac, name: "MacBook Pro", kind: "mac", at: 4000, seq: 2, snapshot: .init(seq: 1, clock: 4))
        try FileManager.default.createDirectory(at: scene.root.appendingPathComponent("devices"), withIntermediateDirectories: true)
        try JSONEncoder().encode(macFile).write(to: scene.root.appendingPathComponent("devices/\(mac).json"))

        let report = try scene.pull()
        #expect(report.devices.map(\.name) == ["MacBook Pro", "iPhone"])
        #expect(report.devices.map(\.isThisDevice) == [false, true])

        let url = scene.root.appendingPathComponent("devices/\(phone).json")
        let mine = try JSONDecoder().decode(SyncDeviceFile.self, from: Data(contentsOf: url))
        #expect(mine.kind == "iphone" && mine.applied == [mac: 2] && mine.seq == 0)

        // With nothing new, the file is not written again.
        let before = try FileManager.default.attributesOfItem(atPath: url.path)[.modificationDate] as? Date
        try SyncEngine.pull(root: scene.root, store: scene.store, deviceID: phone, deviceName: "iPhone", now: 9000)
        let after = try FileManager.default.attributesOfItem(atPath: url.path)[.modificationDate] as? Date
        #expect(before == after)
    }

    @Test func waitsAtAGapAndGoesOnWhenItCloses() throws {
        let scene = try Scene()
        try scene.addSnapshot()
        try scene.addChanges(seq: 2, [planChange(version(5), base: version(2), title: "Plan A")])
        try scene.addChanges(seq: 4, [planChange(version(9), base: version(7), title: "Plan C")])
        var report = try scene.pull()
        #expect(report.applied == 1 && report.waiting)
        #expect(try scene.notes().pages["plan"]?.title == "Plan A", "file 4 is not applied before file 3")

        try scene.addChanges(seq: 3, [planChange(version(7), base: version(5), title: "Plan B")])
        report = try scene.pull()
        #expect(report.applied == 2 && !report.waiting)
        #expect(try scene.notes().pages["plan"]?.title == "Plan C")
    }

    @Test func waitsForAFileThatIsNotHereYet() throws {
        let scene = try Scene()
        try scene.addSnapshot()
        try scene.addChanges(seq: 2, [planChange(version(5), base: version(2), title: "Plan A")])
        // As the iPhone shows a file that iCloud has not brought yet.
        try FileManager.default.moveItem(
            at: scene.changeURL(mac, 2),
            to: scene.changeURL(mac, 2).deletingLastPathComponent().appendingPathComponent(".0000000002.json.icloud")
        )
        var report = try scene.pull()
        #expect(report.applied == 0 && report.waiting)

        // Or a file that is listed with its own name, but without its content.
        try scene.addChanges(seq: 2, [planChange(version(5), base: version(2), title: "Plan A")])
        report = try scene.pull(isHere: { !$0.path.hasSuffix("0000000002.json") })
        #expect(report.applied == 0 && report.waiting)

        // Or half a file.
        try Data("{\"format\": 1, \"chan".utf8).write(to: scene.changeURL(mac, 2))
        #expect(try scene.pull().waiting)
        #expect(try scene.notes().pages["plan"]?.title == "Plan")
    }

    @Test func aSnapshotNumberIsNotAGap() throws {
        let scene = try Scene()
        try scene.addSnapshot(seq: 1)
        try scene.addChanges(seq: 2, [planChange(version(5), base: version(2), title: "Plan A")])
        try scene.pull()
        // The Mac writes snapshot 3, then goes on with change 4.
        try scene.addSnapshot(seq: 3, clock: 5, extra: "UPDATE pages SET title = 'Plan A' WHERE id = 'plan';")
        try scene.addChanges(seq: 4, [planChange(version(7), base: version(5), title: "Plan B")])
        let report = try scene.pull()
        #expect(report.applied == 1 && !report.adoptedSnapshot && !report.waiting)
        #expect(try scene.column("SELECT seq FROM sync_peers WHERE device = '\(mac)'") == ["4"])
    }

    @Test func startsAgainFromANewerSnapshotWhenOldFilesAreGone() throws {
        let scene = try Scene()
        try scene.addSnapshot(seq: 1)
        try scene.pull()
        // While the phone was away the Mac wrote files 2…7, a snapshot 8, removed 2…7, and wrote 9.
        try scene.addSnapshot(seq: 8, clock: 20, extra: "UPDATE pages SET title = 'Plan, much later' WHERE id = 'plan';")
        try scene.addChanges(seq: 9, [
            SyncChange(kind: "space", id: "work", version: version(21), base: nil, space: SyncSpace(name: "Work")),
        ])
        let report = try scene.pull()
        #expect(report.adoptedSnapshot && report.applied == 1 && !report.waiting)
        #expect(try scene.notes().pages["plan"]?.title == "Plan, much later")
        #expect(try scene.notes().spaces.count == 2)
        #expect(try scene.column("SELECT seq FROM sync_peers WHERE device = '\(mac)'") == ["9"])
    }

    @Test func aRemovalDeletesAndStaysRemoved() throws {
        let scene = try Scene()
        try scene.addSnapshot()
        try scene.addChanges(seq: 2, [
            SyncChange(kind: "page", id: "old", version: version(5), base: version(3), removed: true),
            SyncChange(kind: "event", id: "standup", version: version(6), base: version(4), removed: true),
        ])
        try scene.pull()
        #expect(try scene.notes().pages.keys.sorted() == ["plan"])
        #expect(try scene.notes().events.isEmpty)
        #expect(try scene.column("SELECT removed FROM sync_records WHERE id = 'old'") == ["1"])
    }

    @Test func anEditBeatsARemovalMadeAtTheSameTime() throws {
        // The Mac removes the page; the second Mac, which did not know, edits it.
        var scene = try Scene()
        try scene.addSnapshot()
        try scene.addChanges(device: mac, seq: 2, [SyncChange(kind: "page", id: "old", version: version(9), base: version(3), removed: true)])
        try scene.addChanges(device: secondMac, seq: 1, [
            SyncChange(kind: "page", id: "old", version: version(4, secondMac), base: version(3), page: SyncPage(spaceID: "home", title: "Old, edited")),
        ])
        var report = try scene.pull()
        #expect(report.conflicts == 1)
        #expect(try scene.notes().pages["old"]?.title == "Old, edited", "the edit wins though its version is older")

        // The other way round: the edit comes first, the removal after it.
        scene = try Scene()
        try scene.addSnapshot()
        try scene.addChanges(device: mac, seq: 2, [
            SyncChange(kind: "page", id: "old", version: version(4), base: version(3), page: SyncPage(spaceID: "home", title: "Old, edited")),
        ])
        try scene.addChanges(device: secondMac, seq: 1, [
            SyncChange(kind: "page", id: "old", version: version(9, secondMac), base: version(3), removed: true),
        ])
        report = try scene.pull()
        #expect(report.conflicts == 1)
        #expect(try scene.notes().pages["old"]?.title == "Old, edited")
    }

    @Test func twoChangesToOnePage_theNewerWins_theOtherGoesToHistory() throws {
        // Both Macs change the page from the same starting point.
        let scene = try Scene()
        try scene.addSnapshot()
        try scene.addChanges(device: mac, seq: 2, [planChange(version(5), base: version(2), title: "From the Mac", updatedAt: 300)])
        try scene.addChanges(device: secondMac, seq: 1, [planChange(version(6, secondMac), base: version(2), title: "From the second Mac", updatedAt: 400)])
        let report = try scene.pull()
        #expect(report.conflicts == 1 && report.applied == 2)

        #expect(try scene.notes().pages["plan"]?.title == "From the second Mac", "the higher clock wins")
        #expect(try scene.column("SELECT title FROM page_versions WHERE page_id = 'plan'") == ["From the Mac"])
        #expect(try scene.column("SELECT at FROM page_versions WHERE page_id = 'plan'") == ["300"])
        // The history is in the form the desktop app reads (`Vec<Block>` in src/model.rs).
        let blocks = try scene.column("SELECT blocks FROM page_versions WHERE page_id = 'plan'")[0]
        let parsed = try #require(JSONSerialization.jsonObject(with: Data(blocks.utf8)) as? [[String: Any]])
        #expect(parsed.map { $0["kind"] as? String } == ["Heading1", "PageLink"])
        #expect(parsed[0]["text"] as? String == "From the Mac")
        #expect(parsed[1]["target"] as? String == "old")
        #expect(parsed[0]["checked"] as? Bool == false && parsed[0]["data"] as? String == "")
    }

    @Test func twoChangesToOnePage_theOlderLoses_andIsKeptInHistory() throws {
        let scene = try Scene()
        try scene.addSnapshot()
        try scene.addChanges(device: mac, seq: 2, [planChange(version(8), base: version(2), title: "From the Mac")])
        try scene.addChanges(device: secondMac, seq: 1, [planChange(version(6, secondMac), base: version(2), title: "From the second Mac", updatedAt: 400)])
        let report = try scene.pull()
        #expect(report.conflicts == 1 && report.applied == 1)
        #expect(try scene.notes().pages["plan"]?.title == "From the Mac")
        #expect(try scene.column("SELECT title FROM page_versions WHERE page_id = 'plan'") == ["From the second Mac"])
        #expect(try scene.column("SELECT clock || ':' || device FROM sync_records WHERE id = 'plan'") == ["8:\(mac)"])
    }

    @Test func coversPicturesAndTablesPassThroughUnchanged() throws {
        let scene = try Scene()
        try scene.addSnapshot()
        let table = #"{"rows":[["Name","Qty"],["Apples","3"]]}"#
        let page = SyncPage(
            spaceID: "home", title: "Plan", updatedAt: 300, cover: "file:0123456789abcdef0123.jpg", coverY: 20,
            blocks: [
                SyncBlock(id: "i1", kind: "image", data: "0123456789abcdef0123.png"),
                SyncBlock(id: "t1", kind: "table", text: "Name Qty\nApples 3", data: table),
                SyncBlock(id: "x1", kind: "a-kind-from-the-future", text: "words", data: "{\"anything\":true}"),
            ]
        )
        try scene.addChanges(device: mac, seq: 2, [SyncChange(kind: "page", id: "plan", version: version(5), base: version(2), page: page)])
        // The second Mac changed the same page at the same time, and wins: the first change goes to the history.
        try scene.addChanges(device: secondMac, seq: 1, [planChange(version(6, secondMac), base: version(2), title: "Wins")])
        try scene.pull()

        // Before the conflict: exactly what was sent is in the database…
        let other = try Scene()
        try other.addSnapshot()
        try other.addChanges(device: mac, seq: 2, [SyncChange(kind: "page", id: "plan", version: version(5), base: version(2), page: page)])
        try other.pull()
        #expect(try other.column("SELECT cover || ' ' || cover_y FROM pages WHERE id = 'plan'") == ["file:0123456789abcdef0123.jpg 20"])
        #expect(try other.column("SELECT kind FROM blocks WHERE page_id = 'plan' ORDER BY position") == ["image", "table", "a-kind-from-the-future"])
        #expect(try other.column("SELECT data FROM blocks WHERE page_id = 'plan' ORDER BY position") == ["0123456789abcdef0123.png", table, "{\"anything\":true}"])
        let notes = try other.notes()
        #expect(notes.blocks["plan"]?.map(\.kind) == [.image, .table, .text], "a kind this app does not know reads as text")
        #expect(Cover(stored: notes.pages["plan"]?.cover) == .file("0123456789abcdef0123.jpg"))

        // …and after it, the history names the kinds as the desktop app does.
        let blocks = try scene.column("SELECT blocks FROM page_versions WHERE page_id = 'plan'")[0]
        let parsed = try #require(JSONSerialization.jsonObject(with: Data(blocks.utf8)) as? [[String: Any]])
        #expect(parsed.map { $0["kind"] as? String } == ["Image", "Table", "Text"])
        #expect(parsed[1]["data"] as? String == table)
    }

    @Test func aDatabaseFromBeforeCoversGetsTheColumns() throws {
        let scene = try Scene()
        try scene.addSnapshot(schema: Fixture.schemaBeforeCovers)
        let dated = SyncPage(spaceID: "home", title: "Plan", cover: "preset:sea", font: "mono", blocks: [SyncBlock(id: "d1", kind: "todo", text: "Call Sam", due: "2026-11-12")])
        try scene.addChanges(seq: 2, [
            SyncChange(kind: "page", id: "plan", version: version(5), base: version(2), page: dated),
            SyncChange(kind: "page", id: "new", version: version(6), base: nil, page: SyncPage(spaceID: "home", title: "No cover named")),
        ])
        #expect(try scene.pull().applied == 2)
        #expect(try scene.notes().pages["plan"]?.cover == "preset:sea")
        #expect(try scene.notes().pages["old"]?.cover == nil)
        #expect(try scene.column("SELECT cover_y FROM pages WHERE id = 'new'") == ["50"])
        // The newer columns came too: a font, a due day, a picture's place.
        #expect(try scene.column("SELECT font FROM pages ORDER BY id") == ["", "", "mono"])
        #expect(try scene.column("SELECT due || '|' || place FROM blocks WHERE id = 'd1'") == ["2026-11-12|"])
    }

    @Test func aRowPageAReminderAndACallLinkPassThrough() throws {
        let scene = try Scene()
        // A database from before these columns: they are added.
        try scene.addSnapshot(schema: Fixture.schemaBeforeCovers)
        let row = SyncPage(spaceID: "home", parentID: "plan", title: "AAPL", rowOf: "tbl/r1")
        let plan = SyncPage(spaceID: "home", title: "Plan", blocks: [
            SyncBlock(id: "c1", kind: "chart", data: #"{"shape":"pie"}"#),
            SyncBlock(id: "d1", kind: "todo", text: "Call Sam", due: "2026-11-12", remind: "09:30"),
        ])
        let call = SyncEvent(spaceID: "home", title: "Stand-up", startMS: 0, endMS: 1, joinURL: "https://zoom.us/j/1")
        try scene.addChanges(seq: 2, [
            SyncChange(kind: "page", id: "plan", version: version(5), base: version(2), page: plan),
            SyncChange(kind: "page", id: "row", version: version(6), base: nil, page: row),
            SyncChange(kind: "event", id: "call", version: version(7), base: nil, event: call),
        ])
        #expect(try scene.pull().applied == 3)
        let notes = try scene.notes()
        #expect(notes.pages["row"]?.rowOf == "tbl/r1" && notes.pages["plan"]?.rowOf == nil)
        #expect(notes.blocks["plan"]?.map(\.kind) == [.chart, .todo])
        #expect(notes.blocks["plan"]?.last?.remind == "09:30")
        #expect(notes.events.first { $0.id == "call" }?.join == "https://zoom.us/j/1")

        // A device that does not write these names gives none of them.
        let bare = try JSONDecoder().decode(SyncEvent.self, from: Data(#"{"start_ms":0,"end_ms":1}"#.utf8))
        #expect(bare.joinURL == "")
    }

    @Test func aFontAPicturesPlaceAndADueDayPassThroughUnchanged() throws {
        let scene = try Scene()
        try scene.addSnapshot()
        // Not a form this app reads: it must come out exactly as it went in.
        let place = #"{"free":true,"right":true,"x":-40,"y":0,"w":320}"#
        let page = SyncPage(
            spaceID: "home", title: "Plan", updatedAt: 300, font: "serif",
            blocks: [
                SyncBlock(id: "i1", kind: "image", text: "A caption", data: "0123456789abcdef0123.png", place: place),
                SyncBlock(id: "d1", kind: "todo", text: "Call Sam", due: "2026-11-12"),
                SyncBlock(id: "p1", kind: "text", text: "Plain"),
            ]
        )
        try scene.addChanges(device: mac, seq: 2, [SyncChange(kind: "page", id: "plan", version: version(5), base: version(2), page: page)])
        try scene.pull()
        #expect(try scene.column("SELECT font FROM pages WHERE id = 'plan'") == ["serif"])
        #expect(try scene.column("SELECT place FROM blocks WHERE page_id = 'plan' ORDER BY position") == [place, "", ""])
        #expect(try scene.column("SELECT due FROM blocks WHERE page_id = 'plan' ORDER BY position") == ["", "2026-11-12", ""])
        #expect(try scene.column("SELECT text FROM blocks WHERE id = 'i1'") == ["A caption"], "a picture's caption is its text")

        // The names are the column names, also in the file.
        let file = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: scene.changeURL(mac, 2))) as? [String: Any])
        let sent = try #require(((file["changes"] as? [[String: Any]])?.first?["page"]) as? [String: Any])
        #expect(sent["font"] as? String == "serif")
        #expect((sent["blocks"] as? [[String: Any]])?.map { $0["due"] as? String } == ["", "2026-11-12", ""])
        #expect((sent["blocks"] as? [[String: Any]])?.first?["place"] as? String == place)

        // A page from a device that does not write these names has none of them.
        let bare = try JSONDecoder().decode(SyncPage.self, from: Data(#"{"title":"Old Mac","blocks":[{"id":"b","kind":"todo"}]}"#.utf8))
        #expect(bare.font == "" && bare.blocks[0].place == "" && bare.blocks[0].due == "")

        // The page loses to a change from the second Mac: its history entry is
        // in the desktop's form (a place is the object itself; a line with
        // no place or day has neither name).
        try scene.addChanges(device: secondMac, seq: 1, [planChange(version(6, secondMac), base: version(2), title: "Wins")])
        try scene.pull()
        #expect(try scene.column("SELECT font FROM pages WHERE id = 'plan'") == [""])
        let blocks = try scene.column("SELECT blocks FROM page_versions WHERE page_id = 'plan'")[0]
        let parsed = try #require(JSONSerialization.jsonObject(with: Data(blocks.utf8)) as? [[String: Any]])
        let kept = try #require(parsed[0]["place"] as? [String: Any])
        #expect(kept["free"] as? Bool == true && kept["right"] as? Bool == true)
        #expect(kept["x"] as? Int == -40 && kept["w"] as? Int == 320)
        #expect(parsed[0]["text"] as? String == "A caption")
        #expect(parsed[1]["due"] as? String == "2026-11-12")
        #expect(parsed[1]["place"] == nil && parsed[2]["place"] == nil && parsed[2]["due"] == nil && parsed[0]["due"] == nil)
    }

    @Test func equalClocksAreDecidedByTheDeviceId() {
        #expect(version(5, mac) < version(5, secondMac))
        #expect(version(6, mac) > version(5, secondMac))
        #expect(version(5, mac) == version(5, mac))
    }

    @Test func aFileOfANewerFormatStopsTheReading() throws {
        let scene = try Scene()
        try scene.addSnapshot()
        try scene.addChanges(seq: 2, format: 2, [planChange(version(5), base: version(2), title: "Plan A")])
        try scene.addChanges(seq: 3, [planChange(version(7), base: version(5), title: "Plan B")])
        let report = try scene.pull()
        #expect(report.needsUpdate && report.applied == 0)
        #expect(try scene.notes().pages["plan"]?.title == "Plan")
    }

    @Test func aCopyLoadedByHandGivesWayToSync() throws {
        let scene = try Scene()
        // A backup file loaded by hand: no sync tables.
        let manual = try Fixture.make("INSERT INTO pages(id, title) VALUES ('manual', 'Loaded by hand');")
        defer { Fixture.remove(manual) }
        try FileManager.default.copyItem(at: manual, to: scene.store)
        #expect(try scene.pull().isSynced == false, "kept while there is nothing to sync from")
        #expect(try scene.notes().pages.keys.sorted() == ["manual"])

        try scene.addSnapshot()
        #expect(try scene.pull().adoptedSnapshot)
        #expect(try scene.notes().pages.keys.sorted() == ["old", "plan"])
    }

    @Test func readsAFileAsTheFormatDocumentShowsIt() throws {
        let scene = try Scene()
        try scene.addSnapshot()
        let json = """
            {
              "format": 1, "device": "\(mac)", "seq": 2, "at": 1794268000000, "a_name_from_the_future": [1, 2],
              "changes": [
                {"kind": "page", "id": "plan", "version": {"clock": 57, "device": "\(mac)"}, "base": {"clock": 2, "device": "\(mac)"},
                 "page": {"space_id": "home", "parent_id": null, "title": "Mobile app", "icon": "rocket:orange", "position": 3.0,
                          "favorite": true, "trashed": false, "created_at": 1794000000000, "updated_at": 1794268000000, "also_new": true,
                          "blocks": [{"id": "t1", "kind": "todo", "text": "Sync with the Mac", "checked": true, "indent": 1, "collapsed": false,
                                      "icon": null, "language": null, "target": null, "data": ""}]}},
                {"kind": "event", "id": "sync", "version": {"clock": 58, "device": "\(mac)"}, "base": null,
                 "event": {"space_id": "home", "title": "Team sync", "start_ms": 1794268800000, "end_ms": 1794272400000, "all_day": false,
                           "color": "blue", "notes": "", "page_id": null, "created_at": 0, "updated_at": 0, "remind_min": 10,
                           "repeat": "{\\"every\\":\\"week\\",\\"interval\\":1}"}},
                {"kind": "widget", "id": "w", "version": {"clock": 59, "device": "\(mac)"}, "base": null, "widget": {}},
                {"kind": "page", "id": "old", "version": {"clock": 60, "device": "\(mac)"}, "base": {"clock": 3, "device": "\(mac)"}, "removed": true}
              ]
            }
            """
        try FileManager.default.createDirectory(at: scene.changeURL(mac, 2).deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(json.utf8).write(to: scene.changeURL(mac, 2))
        let report = try scene.pull()
        #expect(report.applied == 3, "the kind this app does not know is passed over")

        let notes = try scene.notes()
        let page = try #require(notes.pages["plan"])
        #expect(page.title == "Mobile app" && page.favorite && page.position == 3 && page.updatedAt == 1_794_268_000_000)
        let block = try #require(notes.blocks["plan"]?.first)
        #expect(block.kind == .todo && block.checked && block.indent == 1)
        #expect(notes.pages["old"] == nil)
        #expect(notes.events.last?.repeatRule == Repeat(every: .week))

        // What this app writes reads back the same.
        let file = try JSONDecoder().decode(SyncChangeFile.self, from: Data(json.utf8))
        let again = try JSONDecoder().decode(SyncChangeFile.self, from: JSONEncoder().encode(file))
        #expect(again.changes.map(\.version) == file.changes.map(\.version))
        #expect(again.changes[0].page == file.changes[0].page && again.changes[3].removed)
    }

    @Test func fileNames() {
        #expect(SyncFormat.name(ofSeq: 7) == "0000000007")
        #expect(SyncFormat.seq(inFileName: "0000000007.json") == 7)
        #expect(SyncFormat.seq(inFileName: "0000000123.db") == 123)
        #expect(SyncFormat.seq(inFileName: "7.json") == nil)
        #expect(SyncFormat.seq(inFileName: "notes.json") == nil)
        #expect(SyncFormat.newDeviceID().count == 32)
    }
}

// Editing on this device (`NotesWriter`) and sending it (`SyncEngine.push`).
@Suite struct EditingTests {
    private func synced() throws -> Scene {
        let scene = try Scene()
        try scene.addSnapshot()
        try scene.pull()
        return scene
    }

    private var outbox: (Scene) -> URL { { $0.base.appendingPathComponent("app/outbox/sync-v1") } }

    private func sent(_ scene: Scene, _ seq: Int) throws -> SyncChangeFile {
        let url = outbox(scene).appendingPathComponent("changes/\(phone)/\(SyncFormat.name(ofSeq: seq)).json")
        return try JSONDecoder().decode(SyncChangeFile.self, from: Data(contentsOf: url))
    }

    @Test func aPageChangedHereGoesOutOnceWithTheMacsVersionAsItsBase() throws {
        let scene = try synced()
        #expect(NotesWriter.canEdit(store: scene.store, deviceID: phone))
        let notes = try scene.notes()
        var page = try #require(notes.pages["plan"])
        page.title = "Plan from the phone"
        var blocks = try #require(notes.blocks["plan"])
        blocks[1].checked = true
        try NotesWriter.save(page: page, blocks: blocks, store: scene.store, deviceID: phone, at: 9000)
        #expect(try scene.notes().pages["plan"]?.title == "Plan from the phone")
        #expect(try scene.notes().pages["plan"]?.updatedAt == 9000)

        #expect(try SyncEngine.push(outbox: outbox(scene), store: scene.store, deviceID: phone) == 1)
        let file = try sent(scene, 1)
        let change = try #require(file.changes.first)
        #expect(file.device == phone && file.seq == 1)
        #expect(change.base == version(2) && change.version.device == phone && change.version.clock > 4)
        #expect(change.page?.title == "Plan from the phone" && change.page?.blocks.map(\.checked) == [false, true])
        // Sent: nothing more. The Mac's own records were never this phone's to send.
        #expect(try SyncEngine.push(outbox: outbox(scene), store: scene.store, deviceID: phone) == 0)

        // Into a sync folder, once; the device file says how far the phone wrote.
        try SyncEngine.publish(outbox: outbox(scene), to: scene.root)
        #expect(FileManager.default.fileExists(atPath: scene.changeURL(phone, 1).path))
        try scene.pull()
        let mine = try JSONDecoder().decode(SyncDeviceFile.self, from: Data(contentsOf: scene.root.appendingPathComponent("devices/\(phone).json")))
        #expect(mine.seq == 1)
    }

    @Test func aTickKeepsTheRestOfThePageAndADrawingsScene() throws {
        let scene = try Scene()
        try scene.addSnapshot(extra: "INSERT INTO blocks(id, page_id, position, kind, text, data) VALUES ('d1', 'plan', 2, 'drawing', '', '{\"type\":\"excalidraw\"}');")
        try scene.pull()
        try NotesWriter.setChecked(true, block: "b2", page: "plan", store: scene.store, deviceID: phone)
        #expect(try scene.notes().blocks["plan"]?.map(\.checked) == [false, true, false])
        #expect(try scene.column("SELECT data FROM blocks WHERE id = 'd1'") == [#"{"type":"excalidraw"}"#])
    }

    @Test func anEventMadeChangedAndDeletedHereTravels() throws {
        let scene = try synced()
        let event = Event(id: "gym", spaceID: "home", title: "Gym", start: 10_000, end: 20_000, color: "green", repeatRule: Repeat(every: .week))
        try NotesWriter.save(event: event, store: scene.store, deviceID: phone)
        try SyncEngine.push(outbox: outbox(scene), store: scene.store, deviceID: phone)
        let made = try #require(try sent(scene, 1).changes.first?.event)
        #expect(made.title == "Gym" && made.color == "green" && Repeat.fromJSON(made.repeatRule)?.every == .week)
        #expect(try sent(scene, 1).changes.first?.base == nil, "new here: no base")

        try NotesWriter.remove(event: "standup", store: scene.store, deviceID: phone)
        try SyncEngine.push(outbox: outbox(scene), store: scene.store, deviceID: phone)
        let gone = try #require(try sent(scene, 2).changes.first)
        #expect(gone.removed && gone.id == "standup" && gone.base == version(4))
        #expect(try scene.notes().events.map(\.id) == ["gym"])
    }

    @Test func notesThatAreACopyOrFromANewerRuneAreOnlyRead() throws {
        // A copy loaded by hand: no sync, nowhere to send a change.
        let copy = try Fixture.make(Scene.notes.components(separatedBy: "INSERT INTO sync_records").first ?? "")
        defer { Fixture.remove(copy) }
        #expect(!NotesWriter.canEdit(store: copy, deviceID: phone))
        #expect(throws: NotesWriterError.notSynced) {
            try NotesWriter.remove(event: "standup", store: copy, deviceID: phone)
        }
        // Notes from a newer Rune.
        let scene = try Scene()
        try scene.addSnapshot(extra: "PRAGMA user_version = 99;")
        try scene.pull()
        #expect(!NotesWriter.canEdit(store: scene.store, deviceID: phone))
        #expect(throws: NotesWriterError.newerNotes) {
            try NotesWriter.remove(event: "standup", store: scene.store, deviceID: phone)
        }
        #expect(try scene.notes().events.count == 1)
    }

    @Test func aChangeFromTheMacOnTopOfThePhonesIsTaken() throws {
        let scene = try synced()
        var page = try #require(try scene.notes().pages["plan"])
        page.title = "Phone"
        try NotesWriter.save(page: page, blocks: try scene.notes().blocks["plan"] ?? [], store: scene.store, deviceID: phone)
        try SyncEngine.push(outbox: outbox(scene), store: scene.store, deviceID: phone)
        let phoneVersion = try #require(try sent(scene, 1).changes.first?.version)
        // The Mac took the phone's change, then changed the page again.
        try scene.addChanges(seq: 2, [planChange(version(phoneVersion.clock + 1), base: phoneVersion, title: "Mac again")])
        let report = try scene.pull()
        #expect(report.applied == 1 && report.conflicts == 0)
        #expect(try scene.notes().pages["plan"]?.title == "Mac again")
    }
}

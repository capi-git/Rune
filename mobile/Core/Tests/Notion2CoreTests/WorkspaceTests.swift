// Reading a workspace file, the shape of a page, the page tree, search and
// the icons.

import Foundation
import Testing
@testable import Notion2Core

private let sample = """
    INSERT INTO spaces(id, name, color, position, created_at) VALUES
        ('work', 'Work', 'green', 2.0, 20),
        ('home', 'My Space', 'orange', 1.0, 10);
    INSERT INTO pages(id, parent_id, title, icon, position, favorite, trashed, created_at, updated_at, space_id) VALUES
        ('projects', NULL, 'Projects', 'folder:blue', 2.0, 0, 0, 1, 500, 'home'),
        ('start', NULL, 'Getting Started', '👋', 1.0, 1, 0, 2, 100, 'home'),
        ('mobile', 'projects', 'Mobile app', 'rocket:orange', 1.0, 1, 0, 3, 900, 'home'),
        ('roadmap', 'projects', 'Roadmap', 'target', 2.0, 0, 0, 4, 300, 'home'),
        ('old', NULL, 'Old ideas', NULL, 3.0, 0, 1, 5, 800, 'home'),
        ('blank', NULL, '  ', NULL, 4.0, 0, 0, 6, 50, ''),
        ('q3', NULL, 'Q3 plan', 'chart:green', 1.0, 0, 0, 7, 700, 'work');
    INSERT INTO blocks(id, page_id, position, kind, text, checked, indent, collapsed, icon, language, target, data) VALUES
        ('b3', 'mobile', 2, 'todo', 'Sync with the Mac', 0, 9, 0, NULL, NULL, NULL, ''),
        ('b1', 'mobile', 0, 'text', 'The same pages as on the Mac, made for **one hand**.', 0, 0, 0, NULL, NULL, NULL, ''),
        ('b2', 'mobile', 1, 'todo', 'Read every page', 1, 0, 0, NULL, NULL, NULL, ''),
        ('b4', 'mobile', 3, 'callout', 'Sync comes _first_', 0, 0, 0, 'lightbulb:yellow', NULL, NULL, ''),
        ('b5', 'mobile', 4, 'code', 'let a = b * c * d', 0, 0, 0, NULL, 'swift', NULL, ''),
        ('b6', 'mobile', 5, 'page', '', 0, 0, 0, NULL, NULL, 'roadmap', ''),
        ('b7', 'mobile', 6, 'drawing', '', 0, 0, 0, NULL, NULL, NULL, '{"elements":[]}'),
        ('b8', 'mobile', 7, 'mystery', 'A kind from a newer version', 0, 0, 0, NULL, NULL, NULL, ''),
        ('s1', 'start', 0, 'text', 'A short list of things to try with the new blocks', 0, 0, 0, NULL, NULL, NULL, ''),
        ('o1', 'old', 0, 'text', 'A list that is in the trash', 0, 0, 0, NULL, NULL, NULL, '');
    INSERT INTO events(id, space_id, title, start_ms, end_ms, all_day, color, notes, page_id, created_at, updated_at, remind_min, repeat) VALUES
        ('e2', 'home', 'Team sync', 2000, 3000, 0, 'blue', 'Bring the list of risks', 'mobile', 0, 0, 10, '{"every":"week","interval":1}'),
        ('e1', '', 'Lisbon trip planning', 1000, 1500, 1, 'green', '', NULL, 0, 0, NULL, '');
    INSERT INTO settings(key, value) VALUES ('theme', 'dark');
    """

@Suite struct WorkspaceDatabaseTests {
    @Test func readsEverything() throws {
        let url = try Fixture.make(sample)
        defer { Fixture.remove(url) }
        let ws = try WorkspaceDatabase.load(from: url)

        #expect(ws.spaces.map(\.id) == ["home", "work"], "in order of position")
        #expect(ws.spaces[0].color == "orange")
        #expect(ws.spaces[0].initial == "M")
        #expect(ws.settings["theme"] == "dark")

        #expect(ws.pages.count == 7)
        let mobile = try #require(ws.pages["mobile"])
        #expect(mobile.parentID == "projects")
        #expect(mobile.icon == "rocket:orange")
        #expect(mobile.favorite && !mobile.trashed)
        #expect(mobile.updatedAt == 900)
        #expect(ws.pages["projects"]?.parentID == nil)
        #expect(ws.pages["old"]?.trashed == true)
        #expect(ws.pages["blank"]?.spaceID == "home", "a page with no space is in the first one")
        #expect(ws.pages["blank"]?.displayTitle == "Untitled")

        let blocks = try #require(ws.blocks["mobile"])
        #expect(blocks.map(\.id) == ["b1", "b2", "b3", "b4", "b5", "b6", "b7", "b8"], "in order of position")
        #expect(blocks[0].plainText == "The same pages as on the Mac, made for one hand.")
        #expect(blocks[1].checked && !blocks[2].checked)
        #expect(blocks[2].indent == maxIndent, "an indent out of range is brought back in")
        #expect(blocks[3].kind == .callout && blocks[3].icon == "lightbulb:yellow")
        #expect(blocks[4].kind == .code && blocks[4].language == "swift")
        #expect(blocks[4].plainText == "let a = b * c * d", "code is stored raw")
        #expect(blocks[5].kind == .pageLink && blocks[5].target == "roadmap")
        #expect(blocks[6].kind == .drawing && blocks[6].dataSize == 15)
        #expect(blocks[7].kind == .text, "an unknown kind reads as text")
        #expect(try WorkspaceDatabase.blockData(in: url, blockID: "b7") == #"{"elements":[]}"#)
        #expect(try WorkspaceDatabase.blockData(in: url, blockID: "nope") == "")

        #expect(ws.events.map(\.id) == ["e1", "e2"], "in order of start")
        #expect(ws.events[0].allDay && ws.events[0].spaceID == "home" && ws.events[0].remind == nil)
        #expect(ws.events[1].remind == 10)
        #expect(ws.events[1].repeatRule == Repeat(every: .week))
        #expect(ws.events[1].pageID == "mobile")
    }

    @Test func writesNothing() throws {
        let url = try Fixture.make(sample)
        defer { Fixture.remove(url) }
        let before = try Data(contentsOf: url)
        _ = try WorkspaceDatabase.load(from: url)
        _ = try WorkspaceDatabase.blockData(in: url, blockID: "b7")
        #expect(try Data(contentsOf: url) == before)
        #expect(Fixture.files(beside: url) == ["work space #1.db"], "no journal or shared-memory file is made")
    }

    @Test func readsAFileFromAnOldVersion() throws {
        let url = try Fixture.make(schema: Fixture.oldSchema, """
            INSERT INTO pages(id, title, position) VALUES ('p', 'Notes', 1.0);
            INSERT INTO blocks(id, page_id, position, kind, text) VALUES
                ('a', 'p', 0, 'text', '2 * 3 * 4 = [24]'),
                ('b', 'p', 1, 'code', 'a * b');
            INSERT INTO settings(key, value) VALUES ('workspace_name', 'Capis Space');
            """)
        defer { Fixture.remove(url) }
        let ws = try WorkspaceDatabase.load(from: url)
        #expect(ws.spaces.count == 1)
        #expect(ws.spaces[0].name == "Capis Space")
        #expect(ws.pages["p"]?.spaceID == ws.spaces[0].id)
        #expect(ws.events.isEmpty)
        // Text from before inline formatting shows exactly as it was typed.
        #expect(ws.blocks["p"]?[0].plainText == "2 * 3 * 4 = [24]")
        #expect(Inline.parse(ws.blocks["p"]![0].text).count == 1)
        #expect(ws.blocks["p"]?[1].text == "a * b")
    }

    @Test func refusesOtherFiles() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("notion2-core-tests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }

        let text = folder.appendingPathComponent("notes.db")
        try Data("These are not the bytes of a database, only some text in a file.".utf8).write(to: text)
        #expect(throws: WorkspaceDatabaseError.notAWorkspace) { try WorkspaceDatabase.load(from: text) }

        #expect(throws: WorkspaceDatabaseError.self) {
            try WorkspaceDatabase.load(from: folder.appendingPathComponent("missing.db"))
        }

        let other = try Fixture.make(schema: "CREATE TABLE things(id TEXT);", "")
        defer { Fixture.remove(other) }
        #expect(throws: WorkspaceDatabaseError.notAWorkspace) { try WorkspaceDatabase.load(from: other) }
    }
}

@Suite struct OutlineTests {
    @Test func collapsedToggleHidesChildren() {
        let blocks = [
            Block(kind: .toggle, text: "t", collapsed: true),
            Block(kind: .text, text: "child", indent: 1),
            Block(kind: .text, text: "after"),
        ]
        #expect(Outline.visibility(blocks) == [true, false, true])
        // Opened on the phone, without a change to the file.
        #expect(Outline.visibility(blocks, collapsed: { _ in false }) == [true, true, true])
        #expect(Outline.hasChildren(blocks, 0) && !Outline.hasChildren(blocks, 2))
        #expect(Outline.subtreeEnd(blocks, 0) == 2)
    }

    @Test func aClosedToggleInsideAClosedToggleStaysHidden() {
        let blocks = [
            Block(kind: .toggle, text: "outer", collapsed: true),
            Block(kind: .toggle, text: "inner", indent: 1, collapsed: true),
            Block(kind: .text, text: "deep", indent: 2),
            Block(kind: .text, text: "after", indent: 0),
        ]
        #expect(Outline.visibility(blocks) == [true, false, false, true])
    }

    @Test func numberingRestartsAfterOtherBlocks() {
        let blocks = [
            Block(kind: .numbered, text: "a"),
            Block(kind: .numbered, text: "b"),
            Block(kind: .numbered, text: "b1", indent: 1),
            Block(kind: .numbered, text: "c"),
            Block(kind: .text, text: "break"),
            Block(kind: .numbered, text: "x"),
        ]
        #expect(Outline.listNumbers(blocks) == [1, 2, 1, 3, nil, 1])
    }

    @Test func thePageTree() throws {
        let url = try Fixture.make(sample)
        defer { Fixture.remove(url) }
        let ws = try WorkspaceDatabase.load(from: url)
        #expect(ws.roots(in: "home").map(\.id) == ["start", "projects", "blank"], "in order, without the trash")
        #expect(ws.roots(in: "work").map(\.id) == ["q3"])
        #expect(ws.children(of: "projects").map(\.id) == ["mobile", "roadmap"])
        #expect(ws.ancestors(of: "mobile").map(\.id) == ["projects"])
        #expect(ws.path(of: "mobile") == "Projects")
        #expect(ws.path(of: "projects") == "")
        #expect(ws.favorites(in: "home").map(\.id) == ["start", "mobile"])
        #expect(ws.trashed(in: "home").map(\.id) == ["old"])
        #expect(ws.recent(in: "home", limit: 2).map(\.id) == ["mobile", "projects"])
        #expect(ws.events(in: "work").isEmpty)
        #expect(ws.storedEvent("e2@2026-11-10")?.id == "e2")
    }
}

@Suite struct CoverImageTableTests {
    private static let notes = """
        INSERT INTO pages(id, title, cover, cover_y, updated_at) VALUES
            ('sea', 'By the sea', 'preset:sea', 50, 3),
            ('photo', 'With a photo', 'file:0123456789abcdef0123.jpg', 20, 2),
            ('odd', 'A cover from the future', 'video:clip', 500, 1),
            ('plain', 'No cover', NULL, 50, 0);
        INSERT INTO blocks(id, page_id, position, kind, text, data) VALUES
            ('i1', 'sea', 0, 'image', '', '0123456789abcdef0123.png'),
            ('i2', 'sea', 1, 'image', '', ''),
            ('t1', 'sea', 2, 'table', 'Name Qty\nApples 3\nPears 2 * 2', '{"rows":[["Name","Qty"],["Apples","3"],["Pears","2 * 2"]]}'),
            ('d1', 'sea', 3, 'drawing', '', '{"elements":[1,2,3]}');
        """

    @Test func readsCoversPicturesAndTables() throws {
        let url = try Fixture.make(Self.notes)
        defer { Fixture.remove(url) }
        let ws = try WorkspaceDatabase.load(from: url)

        #expect(ws.pages["sea"]?.cover == "preset:sea")
        #expect(Cover(stored: ws.pages["sea"]?.cover) == .preset(CoverPreset.named("sea")!))
        #expect(Cover(stored: ws.pages["photo"]?.cover) == .file("0123456789abcdef0123.jpg"))
        #expect(ws.pages["photo"]?.coverY == 20)
        #expect(ws.pages["odd"]?.cover == "video:clip", "kept as stored")
        #expect(Cover(stored: ws.pages["odd"]?.cover) == nil, "but not drawn")
        #expect(ws.pages["odd"]?.coverY == 100, "brought back in range")
        #expect(ws.pages["plain"]?.cover == nil && Cover(stored: nil) == nil)

        let blocks = try #require(ws.blocks["sea"])
        #expect(blocks.map(\.kind) == [.image, .image, .table, .drawing])
        #expect(blocks[0].data == "0123456789abcdef0123.png")
        #expect(blocks[1].data == "", "a picture block with no picture yet")
        let table = try #require(TableData.parse(blocks[2].data))
        #expect(table.rows == [["Name", "Qty"], ["Apples", "3"], ["Pears", "2 * 2"]])
        #expect(table.columns == 2 && table.body.count == 2)
        #expect(blocks[3].data == "" && blocks[3].dataSize == 20, "a drawing's scene is not read with the page")
    }

    @Test func readsTheNewestForm() throws {
        let url = try Fixture.make(schema: Fixture.schemaForm6, """
            INSERT INTO pages(id, title, font) VALUES ('journal', 'Journal', 'serif');
            INSERT INTO pages(id, parent_id, title, row_of) VALUES ('trade', 'journal', 'AAPL', 'tbl/r1');
            INSERT INTO pages(id, parent_id, title, row_of) VALUES ('plain', 'journal', 'Notes', '');
            INSERT INTO blocks(id, page_id, position, kind, data) VALUES ('tbl', 'journal', 0, 'table', '{"rows":[["Name"],["AAPL"]]}');
            INSERT INTO blocks(id, page_id, position, kind, data, place) VALUES ('t2', 'journal', 1, 'table', '{"rows":[["A"]]}', '{"free":false,"right":false,"beside":true}');
            INSERT INTO blocks(id, page_id, position, kind, data) VALUES ('c', 'journal', 2, 'chart', '{"shape":"pie"}');
            INSERT INTO blocks(id, page_id, position, kind, text, due, remind) VALUES ('todo', 'journal', 3, 'todo', 'Call', '2026-11-12', '09:30');
            INSERT INTO events(id, title, start_ms, end_ms, join_url) VALUES ('e', 'Stand-up', 0, 1, 'https://zoom.us/j/1');
            """)
        defer { Fixture.remove(url) }
        let ws = try WorkspaceDatabase.load(from: url)
        #expect(ws.pages["journal"]?.font == "serif")
        #expect(ws.pages["trade"]?.rowOf == "tbl/r1" && ws.pages["trade"]?.isRowPage == true)
        #expect(ws.pages["plain"]?.rowOf == nil, "an empty mark is no row")
        let blocks = try #require(ws.blocks["journal"])
        #expect(blocks.map(\.kind) == [.table, .table, .chart, .todo])
        #expect(!blocks[0].standsBeside && blocks[1].standsBeside)
        #expect(blocks[2].data == #"{"shape":"pie"}"#, "a chart keeps what it draws")
        #expect(blocks[3].due == "2026-11-12" && blocks[3].remind == "09:30")
        #expect(ws.events.first?.join == "https://zoom.us/j/1")
    }

    @Test func aFileFromBeforeCoversStillReads() throws {
        let url = try Fixture.make(schema: Fixture.schemaBeforeCovers, "INSERT INTO pages(id, title) VALUES ('p', 'Old');")
        defer { Fixture.remove(url) }
        let page = try #require(try WorkspaceDatabase.load(from: url).pages["p"])
        #expect(page.cover == nil && page.coverY == 50)
    }

    @Test func tablesAreMadeSquare() throws {
        #expect(TableData.parse(#"{"rows":[["a","b","c"],["d"],[]]}"#)?.rows == [["a", "b", "c"], ["d", "", ""], ["", "", ""]])
        #expect(TableData.parse(#"{"rows":[]}"#)?.rows == [[""]], "at least one cell")
        #expect(TableData.parse(#"{"rows":[["a"]],"more":1}"#)?.rows == [["a"]])
        #expect(TableData.parse("") == nil)
        #expect(TableData.parse("not json") == nil)
        #expect(TableData.parse(#"{"cells":[]}"#) == nil)
        let wide = TableData(rows: [Array(repeating: "x", count: 40)] + Array(repeating: ["y"], count: 300))
        #expect(wide.columns == TableData.mostColumns && wide.rows.count == TableData.mostRows)
        #expect(TableData(rows: [["Name", "Qty"], ["Apples", "3"]]).plainText == "Name Qty\nApples 3")
    }

    @Test func searchLooksInTableCellsNotInPictures() throws {
        let url = try Fixture.make(Self.notes)
        defer { Fixture.remove(url) }
        let index = SearchIndex(try WorkspaceDatabase.load(from: url))
        #expect(index.search("apples").inside.map(\.title) == ["By the sea"])
        #expect(index.search("2 * 2").inside.count == 1, "a cell is plain text: its marks are not formatting")
        #expect(index.search("0123456789").isEmpty, "a file name is not text")
        #expect(index.search("rows").isEmpty, "nor is the table's JSON")
    }

    @Test func theAppsOwnCovers() {
        #expect(CoverPreset.all.map(\.name) == [
            "sand", "clay", "ember", "moss", "sea", "slate", "dawn", "night",
            "meadow", "shore", "stone", "rust", "honey", "fern", "lake", "plum",
        ])
        #expect(CoverPreset.named("stone")?.stops.count == 1, "a plain colour")
        #expect(CoverPreset.named("meadow")?.glow != nil)
        #expect(CoverPreset.named("nope") == nil)
        #expect(Cover(stored: "preset:nope") == nil)
    }

    @Test func onlyNamesTheAppMadeArePictures() {
        #expect(Pictures.isValidName("0123456789abcdef0123.png"))
        #expect(Pictures.isValidName("a.heic"))
        #expect(!Pictures.isValidName("../secret.png"))
        #expect(!Pictures.isValidName("a.b.png"))
        #expect(!Pictures.isValidName("photo.exe"))
        #expect(!Pictures.isValidName(".png"))
        #expect(!Pictures.isValidName("noending"))
        #expect(!Pictures.isValidName(String(repeating: "a", count: 41) + ".png"))
        #expect(Cover(stored: "file:../../etc/passwd") == nil)
    }
}

@Suite struct OpenTodoTests {
    @Test func openTodosComeFromAllPagesOfTheSpace() throws {
        let url = try Fixture.make("""
            INSERT INTO spaces(id, name, position) VALUES ('home', 'My Space', 1.0), ('work', 'Work', 2.0);
            INSERT INTO pages(id, title, trashed, updated_at, space_id) VALUES
                ('old', 'Edited first', 0, 100, 'home'),
                ('new', 'Edited last', 0, 900, 'home'),
                ('none', '', 0, 500, 'home'),
                ('bin', 'In the trash', 1, 950, 'home'),
                ('job', 'At work', 0, 990, 'work');
            INSERT INTO blocks(id, page_id, position, kind, text, checked) VALUES
                ('o2', 'old', 1, 'todo', 'Second on the old page', 0),
                ('o1', 'old', 0, 'todo', 'First on the old page', 0),
                ('n1', 'new', 0, 'text', 'Not a to-do', 0),
                ('n2', 'new', 1, 'todo', 'Done already', 1),
                ('n3', 'new', 2, 'todo', 'Send the **plan** to _Sam_', 0),
                ('n4', 'new', 3, 'todo', '   ', 0),
                ('u1', 'none', 0, 'todo', 'On a page with no title', 0),
                ('t1', 'bin', 0, 'todo', 'On a page in the trash', 0),
                ('w1', 'job', 0, 'todo', 'In the other space', 0);
            """)
        defer { Fixture.remove(url) }
        let ws = try WorkspaceDatabase.load(from: url)

        let todos = ws.openTodos(in: "home")
        #expect(todos.map(\.blockID) == ["n3", "u1", "o1", "o2"], "pages edited last first, then the order on the page")
        #expect(todos[0].text == "Send the plan to Sam", "without formatting marks")
        #expect(todos[0].pageTitle == "Edited last" && todos[0].pageID == "new")
        #expect(todos[1].pageTitle == "Untitled")
        #expect(ws.openTodos(in: "work").map(\.text) == ["In the other space"])
        #expect(ws.openTodos(in: "nowhere").isEmpty)
    }
}

@Suite struct SearchTests {
    private func index() throws -> SearchIndex {
        let url = try Fixture.make(sample)
        defer { Fixture.remove(url) }
        return SearchIndex(try WorkspaceDatabase.load(from: url))
    }

    @Test func titlesThenTextInsidePages() throws {
        let found = try index().search("lis")
        #expect(found.names.isEmpty)
        #expect(found.inside.map(\.title) == ["Getting Started"], "the page in the trash is left out")
        #expect(found.inside[0].snippet == "A short list of things to try with the new blocks")
        #expect(found.events.map(\.title) == ["Team sync", "Lisbon trip planning"], "the latest first")
        #expect(found.events[0].snippet == "Bring the list of risks")
    }

    @Test func titlesThatStartWithTheWordsComeFirst() throws {
        let found = try index().search("  PRO ")
        #expect(found.names.map(\.title) == ["Projects"])
        let all = try index().search("p")
        #expect(all.names.map(\.title) == ["Projects", "Mobile app", "Q3 plan", "Roadmap"])
        #expect(all.names[1].path == "Projects")
        #expect(all.names[0].path == "My Space", "a top-level page shows its space")
    }

    @Test func oneSpaceOrAll() throws {
        #expect(try index().search("plan").names.map(\.title) == ["Q3 plan"])
        #expect(try index().search("plan", space: "home").names.isEmpty)
        #expect(try index().search("plan", space: "home").events.count == 1)
        #expect(try index().search("").isEmpty)
        #expect(try index().search("zzzz").isEmpty)
    }

    @Test func formattingMarksAreNotSearched() throws {
        #expect(try index().search("one hand").inside.map(\.title) == ["Mobile app"])
        #expect(try index().search("**").isEmpty)
        // Accents and case do not matter.
        #expect(SearchIndex.ranges(of: "cafe", in: "Café, CAFE, cafés").count == 3)
    }

    @Test func longTextIsCutAroundTheMatch() {
        let text = String(repeating: "a", count: 100) + " needle " + String(repeating: "b", count: 100)
        let match = text.range(of: "needle")!
        let snippet = SearchIndex.snippet(around: match, in: text)
        #expect(snippet.hasPrefix("…") && snippet.hasSuffix("…"))
        #expect(snippet.contains("needle"))
        #expect(snippet.count == 30 + 6 + 50 + 2)
    }
}

@Suite struct IconTests {
    @Test func storedIconsBecomeNameAndColour() {
        #expect(Icons.parse("rocket:purple") == IconRef(name: "rocket", color: "purple"))
        #expect(Icons.parse("rocket") == IconRef(name: "rocket", color: "gray"))
        #expect(Icons.parse("rocket:teal") == IconRef(name: "rocket", color: "gray"))
        #expect(Icons.parse("nonsense:blue") == IconRef(name: "file", color: "gray"))
        #expect(Icons.parse("") == IconRef(name: "file", color: "gray"))
        #expect(Icons.parse(nil) == IconRef(name: "file", color: "gray"))
    }

    @Test func oldEmojiMapToIcons() {
        #expect(Icons.parse("👋") == IconRef(name: "smile", color: "yellow"))
        #expect(Icons.parse("🚀") == IconRef(name: "rocket", color: "purple"))
        #expect(Icons.parse("✈️") == IconRef(name: "plane", color: "blue"), "with the emoji's variation mark")
        #expect(Icons.parse("🦄") == IconRef(name: "file", color: "gray"))
    }

    @Test func everyIconDraws() {
        #expect(Icons.names.count >= 79)
        #expect(Icons.colors == ["gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red"])
        for name in Icons.names {
            let shapes = Icons.shapes(for: name)
            #expect(!shapes.isEmpty, "\(name) has no shapes")
            for shape in shapes {
                guard case .move = shape.elements.first else {
                    Issue.record("\(name): a shape does not start with a move")
                    continue
                }
                #expect(shape.fill || shape.stroke, "\(name): a shape that draws nothing")
                for element in shape.elements {
                    let (x, y): (Double, Double)
                    switch element {
                    case let .move(px, py), let .line(px, py): (x, y) = (px, py)
                    case let .curve(px, py, _, _, _, _): (x, y) = (px, py)
                    case .close: continue
                    }
                    #expect(x.isFinite && y.isFinite && (0...24).contains(x) && (0...24).contains(y), "\(name): a point at \(x), \(y) is off the grid")
                }
            }
        }
        #expect(Icons.shapes(for: "no-such-icon") == Icons.shapes(for: "file"))
    }

    @Test func readsPathsRectsAndCircles() {
        // "check": three points joined by lines.
        #expect(Icons.shapes(for: "check")[0].elements == [.move(x: 5, y: 12.5), .line(x: 9.5, y: 17), .line(x: 19, y: 7.5)])
        // Numbers run together ("0 0 1 12 0", ".45.5") and relative commands.
        var path = PathData("M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 1.75h-15z")
        let bell = path.elements()
        #expect(bell.first == .move(x: 6, y: 16.5))
        #expect(bell[1] == .line(x: 6, y: 11))
        #expect(bell.last == .close)
        guard case let .curve(x, y, _, _, _, _) = bell[3] else {
            Issue.record("the arc did not become curves")
            return
        }
        #expect(abs(x - 18) < 1e-9 && abs(y - 11) < 1e-9, "the half circle ends where it should")
        guard case let .curve(mx, my, _, _, _, _) = bell[2] else { return }
        #expect(abs(mx - 12) < 1e-9 && abs(my - 5) < 1e-9, "its top is 6 above the centre")

        // A filled dot with no stroke, and a stroke of another width.
        let bot = Icons.shapes(for: "bot")
        #expect(bot.last?.fill == true && bot.last?.stroke == false)
        #expect(Icons.shapes(for: "bold")[0].strokeWidth == 2.25)
        #expect(Icons.shapes(for: "divider")[1].strokeOpacity == 0.45)
        // A rounded rectangle is closed and keeps its corners.
        let note = Icons.shapes(for: "note")[0].elements
        #expect(note.first == .move(x: 7.25, y: 3.75) && note.last == .close)
    }
}

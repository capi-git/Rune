#!/usr/bin/env python3
"""Make a sample workspace file to try the iPhone app with.

It has two spaces, pages inside pages, a long page with every kind of block
(tables and pictures too), pages with covers, and events around today
(repeating, all-day, with reminders). The tables are
the ones the desktop app makes (`migrate` in src/store.rs).

    python3 mobile/scripts/make-sample-db.py /path/to/sample.db
"""

import datetime as dt
import json
import os
import shutil
import sqlite3
import sys
import uuid

SCHEMA = """
CREATE TABLE pages(id TEXT PRIMARY KEY, parent_id TEXT, title TEXT NOT NULL DEFAULT '', icon TEXT,
    position REAL NOT NULL DEFAULT 0, favorite INTEGER NOT NULL DEFAULT 0, trashed INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL DEFAULT 0, space_id TEXT NOT NULL DEFAULT '',
    cover TEXT, cover_y INTEGER NOT NULL DEFAULT 50, font TEXT NOT NULL DEFAULT '', row_of TEXT);
CREATE TABLE blocks(id TEXT PRIMARY KEY, page_id TEXT NOT NULL, position INTEGER NOT NULL, kind TEXT NOT NULL,
    text TEXT NOT NULL DEFAULT '', checked INTEGER NOT NULL DEFAULT 0, indent INTEGER NOT NULL DEFAULT 0,
    collapsed INTEGER NOT NULL DEFAULT 0, icon TEXT, language TEXT, target TEXT, data TEXT NOT NULL DEFAULT '',
    place TEXT NOT NULL DEFAULT '', due TEXT NOT NULL DEFAULT '', remind TEXT NOT NULL DEFAULT '');
CREATE INDEX blocks_by_page ON blocks(page_id, position);
CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE events(id TEXT PRIMARY KEY, space_id TEXT NOT NULL DEFAULT '', title TEXT NOT NULL DEFAULT '',
    start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, all_day INTEGER NOT NULL DEFAULT 0,
    color TEXT NOT NULL DEFAULT 'blue', notes TEXT NOT NULL DEFAULT '', page_id TEXT,
    created_at INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL DEFAULT 0, remind_min INTEGER,
    repeat TEXT NOT NULL DEFAULT '', join_url TEXT NOT NULL DEFAULT '');
CREATE INDEX events_by_time ON events(start_ms);
CREATE TABLE page_versions(id INTEGER PRIMARY KEY AUTOINCREMENT, page_id TEXT NOT NULL, at INTEGER NOT NULL,
    title TEXT NOT NULL DEFAULT '', blocks TEXT NOT NULL);
CREATE INDEX versions_by_page ON page_versions(page_id, at);
CREATE TABLE spaces(id TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '', color TEXT NOT NULL DEFAULT 'blue',
    position REAL NOT NULL DEFAULT 0, created_at INTEGER NOT NULL DEFAULT 0);
PRAGMA user_version = 6;
"""

# The name of the sample picture, in the form the Mac app gives a picture.
PICTURE = "5a3b6b0a1e9b3281740d.png"

NOW = dt.datetime.now().replace(second=0, microsecond=0)
TODAY = NOW.date()


def new_id() -> str:
    return uuid.uuid4().hex


def ms(moment: dt.datetime) -> int:
    return int(moment.timestamp() * 1000)


def at(day: dt.date, hour: int = 0, minute: int = 0) -> int:
    return ms(dt.datetime(day.year, day.month, day.day, hour, minute))


class Book:
    def __init__(self, db: sqlite3.Connection):
        self.db = db
        self.order = 0.0

    def space(self, name: str, color: str) -> str:
        sid = new_id()
        self.order += 1
        self.db.execute("INSERT INTO spaces VALUES (?,?,?,?,?)", (sid, name, color, self.order, ms(NOW)))
        return sid

    def page(self, space, title, icon=None, parent=None, favorite=False, trashed=False, edited_min_ago=60, blocks=(), cover=None, cover_y=50, font="", row_of=None, pid=None):
        pid = pid or new_id()
        self.order += 1
        edited = ms(NOW - dt.timedelta(minutes=edited_min_ago))
        self.db.execute(
            "INSERT INTO pages VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            (pid, parent, title, icon, self.order, int(favorite), int(trashed), edited - 86_400_000, edited, space, cover, cover_y, font, row_of),
        )
        self.blocks(pid, blocks)
        return pid

    def blocks(self, page_id, blocks):
        start = self.db.execute("SELECT count(*) FROM blocks WHERE page_id=?", (page_id,)).fetchone()[0]
        for i, b in enumerate(blocks):
            self.db.execute(
                "INSERT INTO blocks VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                (
                    b.get("id") or new_id(), page_id, start + i, b["kind"], b.get("text", ""), int(b.get("checked", False)),
                    b.get("indent", 0), int(b.get("collapsed", False)), b.get("icon"), b.get("language"),
                    b.get("target"), b.get("data", ""), b.get("place", ""), b.get("due", ""), b.get("remind", ""),
                ),
            )

    def event(self, space, title, start, end, color="blue", all_day=False, notes="", page=None, remind=None, repeat=None, join=""):
        self.db.execute(
            "INSERT INTO events VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            (
                new_id(), space, title, start, end, int(all_day), color, notes, page, ms(NOW), ms(NOW), remind,
                json.dumps(repeat, separators=(",", ":")) if repeat else "", join,
            ),
        )


def b(kind, text="", **more):
    return {"kind": kind, "text": text, **more}


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    path = sys.argv[1]
    if os.path.exists(path):
        os.remove(path)
    db = sqlite3.connect(path)
    db.executescript(SCHEMA)
    db.execute("INSERT INTO settings VALUES ('text_format', 'inline-md-1')")
    book = Book(db)

    home = book.space("My Space", "orange")
    work = book.space("Work", "green")

    start = book.page(home, "Getting Started", "sparkles:purple", favorite=True, edited_min_ago=3 * 24 * 60, blocks=[
        b("text", "Welcome. This page shows what a page can hold."),
        b("bulleted", "Pages go inside pages."),
        b("bulleted", "Type `/` on the Mac to add a block."),
    ])
    ideas = book.page(home, "Ideas", "lightbulb:yellow", parent=start, edited_min_ago=5 * 24 * 60, blocks=[
        b("text", "A short list of things to try with the new blocks."),
        b("todo", "A drawing of the page tree"),
        b("todo", "A calendar week on one screen", checked=True),
    ])
    projects = book.page(home, "Projects", "folder:gray", edited_min_ago=26 * 60)
    roadmap = book.page(home, "Roadmap", "target:blue", parent=projects, edited_min_ago=4 * 24 * 60, cover="preset:rust", blocks=[
        b("heading2", "This quarter"),
        b("numbered", "Read-only iPhone app"),
        b("numbered", "Editing on the iPhone"),
        b("numbered", "Sync with the Mac"),
    ])
    old = book.page(home, "Old plan", "note:gray", trashed=True, edited_min_ago=9 * 24 * 60, blocks=[
        b("text", "This page is in the trash."),
    ])
    reading = book.page(home, "Reading List", "book:blue", favorite=True, edited_min_ago=22 * 60, blocks=[
        b("todo", "_The Design of Everyday Things_", checked=True),
        b("todo", "_A Pattern Language_"),
        b("todo", "_Thinking in Systems_"),
    ])
    journal = book.page(home, "Journal", "note:green", edited_min_ago=3 * 24 * 60)
    lisbon = book.page(home, "Trip to Lisbon", "globe:green", parent=journal, edited_min_ago=2 * 24 * 60, cover=f"file:{PICTURE}", cover_y=35, blocks=[
        b("text", "Three days in Lisbon. The list of places is short."),
        b("image", data=PICTURE),
        b("bulleted", "Alfama, early"),
        b("bulleted", "LX Factory"),
        b("bulleted", "Pastéis de Belém"),
    ])
    book.page(home, "To-dos", "check-circle:green", edited_min_ago=90, blocks=[
        b("todo", "Send the notes of the team sync"),
        b("todo", "Book the dentist", checked=True),
        b("todo", "Pack for Lisbon"),
    ])
    meetings = book.page(home, "Meeting notes", "folder:blue", edited_min_ago=5 * 60)
    sync_notes = book.page(home, f"Team sync, {NOW.strftime('%b')} {NOW.day}", "chat:blue", parent=meetings, edited_min_ago=5 * 60, blocks=[
        b("text", f"{NOW.strftime('%A, %B')} {NOW.day}, 2:00 PM"),
        b("heading3", "Decisions"),
        b("bulleted", "The iPhone app reads a copy of the notes first."),
        b("bulleted", "Sync is the next step."),
        b("heading3", "Tasks"),
        b("todo", "Write the plan for sync"),
    ])
    book.page(home, "", None, edited_min_ago=8 * 24 * 60, blocks=[b("text", "A page with no title and no icon.")])

    drawing = json.dumps({
        "type": "excalidraw", "version": 2,
        "elements": [
            {"type": "rectangle", "x": 40, "y": 40, "width": 160, "height": 90},
            {"type": "ellipse", "x": 260, "y": 50, "width": 120, "height": 80},
            {"type": "arrow", "x": 200, "y": 85, "points": [[0, 0], [60, 5]]},
        ],
    })
    table = json.dumps({"rows": [
        ["Screen", "State", "Notes"],
        ["Home", "Done", "The pages changed last, the next events, to-dos"],
        ["Page", "Done", "Every kind of block"],
        ["Calendar", "Done", "Week and month"],
        ["Editing", "Later", ""],
    ]})
    wide = json.dumps({"rows": [
        ["Month", "Pages", "Blocks", "Events", "To-dos done", "Longest page", "Notes"],
        ["September", "41", "1,203", "18", "64", "Reading List", "A slow month"],
        ["October", "57", "1,988", "25", "91", "Mobile app", "The plan for the iPhone app was written"],
    ]})
    mobile = book.page(home, "Mobile app", "rocket:orange", parent=projects, favorite=True, edited_min_ago=10, cover="preset:sea", blocks=[
        b("text", "The same pages as on the Mac, made for **one hand** and short moments. This page has every kind of block, so it shows how each one looks on the iPhone."),
        b("heading1", "Heading 1: the plan"),
        b("text", "Inline text can be **bold**, _italic_, **_both_**, ~~struck out~~ or `inline code`. A link goes to [the web](https://example.com/notes) or to [another page](rune://page/ROADMAP). Characters such as 2 \\* 3 \\* 4, snake\\_case and \\[brackets\\] show as typed."),
        b("heading2", "Heading 2: first version"),
        b("todo", "Read every page", checked=True),
        b("todo", "Search page names and text", checked=True),
        b("todo", "The calendar, with **repeating** events", checked=True),
        b("todo", "Sync with the Mac", due=(TODAY + dt.timedelta(days=1)).isoformat(), remind="09:30"),
        b("todo", "A to-do inside a to-do", indent=1),
        b("heading3", "Heading 3: lists"),
        b("bulleted", "A bulleted item"),
        b("bulleted", "A nested item, with text long enough to go onto a second line on a small screen", indent=1),
        b("bulleted", "Nested again", indent=2),
        b("bulleted", "Back at the top level"),
        b("numbered", "First"),
        b("numbered", "Second"),
        b("numbered", "A step inside the second", indent=1),
        b("numbered", "Another step inside", indent=1),
        b("numbered", "Third"),
        b("text", "A paragraph stops a numbered list. The next list starts at 1 again."),
        b("numbered", "A new list"),
        b("toggle", "An open toggle (tap the arrow to close it)"),
        b("text", "This text is inside the toggle.", indent=1),
        b("bulleted", "So is this item.", indent=1),
        b("toggle", "A closed toggle (tap the arrow to open it)", collapsed=True),
        b("text", "Hidden until the toggle is open.", indent=1),
        b("quote", "Simplicity is not the absence of clutter.\nIt is the result of careful work."),
        b("callout", "Sync comes first: a page must look the same on both devices.", icon="lightbulb:yellow"),
        b("callout", "The iPhone shows a **copy** of the notes in this version.", icon="info:blue"),
        b("callout", "Never put the live database file in a synced folder.", icon="warning:red"),
        b("code", "func occurrences(of event: Event, from: Int64, to: Int64) -> [Event] {\n    guard let rule = event.repeatRule else { return event.overlaps(from, to) ? [event] : [] }\n    return days(first: day(of: event.start), rule: rule).map { at(event, $0) }\n}", language="swift"),
        b("code", "SELECT title FROM pages WHERE trashed = 0 ORDER BY position;"),
        b("divider"),
        b("text", "Links to other pages:"),
        b("page", target="ROADMAP"),
        b("page", target="IDEAS"),
        b("page", target="OLD"),
        b("page", target="0000missing0000"),
        b("drawing", data=drawing),
        b("heading2", "Tables and pictures"),
        b("table", "Screen State Notes", data=table),
        b("text", "A table that is wider than the page moves sideways:"),
        b("table", "Month Pages Blocks", data=wide),
        b("image", data=PICTURE),
        b("image", data="0000000000missing000.png"),
        b("image"),
        b("heading2", "A longer text"),
        b("text", "The page must stay easy to read when it is long. Each paragraph keeps a calm line length, the same size of text as the rest of the phone, and enough room between the lines. Nothing moves when the page scrolls, and nothing covers the text."),
        b("text", "Notes are for short moments on the go: a list in the shop, the address of a meeting, the thing somebody said that must not be lost. So a page opens at once and the text comes first."),
        b("text", "Other scripts and emoji stay whole: 日本語のメモ, Ελληνικά, café, 👩‍👩‍👧 and 🚀."),
        b("text", ""),
        b("text", "An empty line is above this one."),
        b("heading2", "Open questions"),
        b("bulleted", "How large can a page be before it feels slow?"),
        b("bulleted", "Which block kinds are used most on a phone?"),
        b("bulleted", "Where does the quick note go when there is no network?"),
        b("text", "The end of the long page."),
        b("text", ""),
        b("text", ""),
    ])
    refs = {"ROADMAP": roadmap, "IDEAS": ideas, "OLD": old}
    for key, value in refs.items():
        db.execute("UPDATE blocks SET target=? WHERE target=?", (value, key))
        db.execute("UPDATE blocks SET text=replace(text, ?, ?)", (f"rune://page/{key}", f"rune://page/{value}"))
    book.blocks(projects, [b("page", target=mobile), b("page", target=roadmap)])
    book.blocks(journal, [b("text", "Notes by day and by trip."), b("page", target=lisbon)])
    book.blocks(meetings, [b("page", target=sync_notes)])

    book.page(work, "Q4 plan", "chart:green", favorite=True, edited_min_ago=3 * 60, cover="preset:meadow", blocks=[
        b("heading2", "Goals"),
        b("numbered", "Ship the catalog"),
        b("numbered", "Hire two people"),
    ])
    book.page(work, "People", "briefcase:brown", edited_min_ago=30 * 60)

    # A trading journal, as Rune 0.1.3 and later make it: a typed table
    # (money that is green or red, tags, a status, a formula, totals at the
    # foot, a sort and a colour rule), charts of it, a table beside another,
    # pages for two rows, and a diagram.
    def day_ago(n):
        return (TODAY - dt.timedelta(days=n)).isoformat()
    trades = {
        "rows": [
            ["Ticker", "Date", "Side", "Result", "P&L", "Shares", "Price", "Cost", "Tags", "Status", "Reviewed"],
            ["AAPL", day_ago(1), "Long", "Win", "$412.50", "50", "$228.10", "", "Breakout, Earnings", "Done", "true"],
            ["TSLA", day_ago(2), "Short", "Loss", "-$186.00", "20", "$251.40", "", "Reversal", "In progress", "false"],
            ["NVDA", day_ago(4), "Long", "Win", "$1,020.00", "30", "$131.75", "", "Momentum", "Done", "true"],
            ["AMD", day_ago(8), "Long", "Loss", "-$95.25", "40", "$148.20", "", "Breakout", "Not started", "false"],
            ["MSFT", day_ago(9), "Long", "Win", "$260.00", "15", "$418.90", "", "Earnings", "Done", "true"],
            ["SPY", day_ago(15), "Short", "Win", "$140.00", "10", "$585.00", "", "Hedge", "Done", "false"],
        ],
        "columns": [
            {"kind": "text", "total": "count"},
            {"kind": "date"},
            {"kind": "choice", "options": [{"name": "Long", "color": "green"}, {"name": "Short", "color": "red"}]},
            {"kind": "choice", "options": [{"name": "Win", "color": "green"}, {"name": "Loss", "color": "red"}]},
            {"kind": "number", "format": "USD", "total": "sum", "signed": True},
            {"kind": "number", "total": "sum"},
            {"kind": "number", "format": "USD"},
            {"kind": "formula", "format": "USD", "formula": "Shares * Price", "total": "sum"},
            {"kind": "multi", "options": [{"name": "Breakout", "color": "blue"}, {"name": "Earnings", "color": "purple"}, {"name": "Momentum", "color": "orange"}, {"name": "Reversal", "color": "yellow"}, {"name": "Hedge", "color": "gray"}]},
            {"kind": "status"},
            {"kind": "checkbox", "total": "checked"},
        ],
        "name": "Trades",
        "icon": "target:green",
        "info": [{"id": ""}, {"id": "r1"}, {"id": "r2"}, {"id": "r3"}, {"id": "r4"}, {"id": "r5"}, {"id": "r6"}],
        "view": {"sort": [{"column": 1, "down": True}], "frozen": 1, "paint": [{"column": 4, "op": "below", "text": "0", "color": "red"}]},
    }
    goals = {
        "rows": [["Month", "Goal", "Done"], ["October", "$2,000.00", "68%"], ["November", "$2,500.00", "41%"]],
        "columns": [{"kind": "text"}, {"kind": "number", "format": "USD"}, {"kind": "number", "format": "percent", "show": "ring", "goal": 100}],
        "name": "Goals",
        "icon": "trophy:yellow",
    }
    board = dict(trades, name="Trades by status", view={"layout": "board", "group": 9})
    flow = "flowchart LR\n  A[Plan the trade] --> B{Setup there?}\n  B -- yes --> C[Enter]\n  B -- no --> A\n  C --> D[Write the review]"
    journal_id = new_id()
    table_id = new_id()
    book.page(work, "Trading journal", "chart:green", favorite=True, edited_min_ago=5, font="serif", pid=journal_id, blocks=[
        b("text", "Each trade on one row. The charts below draw the table above them."),
        b("table", id=table_id, data=json.dumps(trades)),
        b("chart", data=json.dumps({"shape": "pie", "x": "Result", "add": "count", "title": "Wins and losses"})),
        b("chart", data=json.dumps({"shape": "bars", "x": "Date", "by": "row", "y": "P&L", "add": "sum", "running": True, "title": "Running P&L"})),
        b("table", data=json.dumps(goals)),
        b("table", data=json.dumps({"rows": [["Rule"], ["No trade in the first 15 minutes"], ["Risk at most 1% a trade"]], "name": "Rules", "icon": "book:blue"}), place=json.dumps({"free": False, "right": False, "beside": True})),
        b("table", data=json.dumps(board)),
        b("heading2", "How I trade"),
        b("code", flow, language="mermaid"),
        b("code", "flowchart TD\n  A --> ", language="mermaid"),
    ])
    book.page(work, "AAPL", parent=journal_id, row_of=f"{table_id}/r1", edited_min_ago=60 * 24, blocks=[
        b("heading2", "Why I took it"),
        b("text", "A clean break of the range on the day after earnings."),
    ])
    book.page(work, "TSLA", parent=journal_id, row_of=f"{table_id}/r2", edited_min_ago=60 * 48, blocks=[
        b("text", "Shorted too early. Wait for the second lower high."),
    ])

    day = dt.timedelta(days=1)
    monday = TODAY - TODAY.weekday() * day
    thursday = monday + 3 * day if (monday + 3 * day) > TODAY else monday + 10 * day
    saturday = monday + 5 * day
    book.event(home, "Stand-up", at(monday - 28 * day, 9), at(monday - 28 * day, 9, 15), color="gray",
               repeat={"every": "weekday", "interval": 1})
    book.event(home, "Team sync", at(TODAY - 21 * day, 14), at(TODAY - 21 * day, 15), color="blue", remind=10,
               page=sync_notes, notes="Bring the list of risks.\nRoom 2, or the call link in the invite.",
               repeat={"every": "week", "interval": 1})
    book.event(home, "Gym", at(TODAY - 14 * day, 18, 30), at(TODAY - 14 * day, 19, 30), color="orange",
               repeat={"every": "day", "interval": 2, "skip": [(TODAY + 2 * day).isoformat()]})
    book.event(home, "Pack for Lisbon", at(TODAY), at(TODAY + day), color="green", all_day=True)
    book.event(home, "Dentist", at(thursday, 9, 30), at(thursday, 10, 15), color="red", remind=60)
    book.event(home, "Trip to Lisbon", at(saturday), at(saturday + 3 * day), color="green", all_day=True, page=lisbon)
    book.event(home, "Rent", at(dt.date(TODAY.year - 1, 1, 1)), at(dt.date(TODAY.year - 1, 1, 2)), color="brown", all_day=True,
               repeat={"every": "month", "interval": 1})
    book.event(home, "Mum's birthday", at(dt.date(2020, TODAY.month, min(TODAY.day + 3, 28))), at(dt.date(2020, TODAY.month, min(TODAY.day + 3, 28)) + day),
               color="pink", all_day=True, remind=24 * 60, repeat={"every": "year", "interval": 1})
    book.event(home, "Late call with Sydney", at(TODAY + day, 23), at(TODAY + 2 * day, 0, 30), color="purple", remind=15)
    book.event(home, "Book club", at(TODAY - 56 * day, 19), at(TODAY - 56 * day, 20, 30), color="yellow",
               repeat={"every": "week", "interval": 2, "until": (TODAY + 40 * day).isoformat()})
    book.event(work, "Planning", at(TODAY + day, 10), at(TODAY + day, 11), color="green", remind=10,
               join="https://zoom.us/j/1234567890")

    db.execute("INSERT INTO settings VALUES ('expanded', ?)", (json.dumps([projects, f"fav:{mobile}"]),))
    db.commit()
    db.execute("VACUUM")
    db.close()
    # The pictures of a workspace are files beside its database. The sample
    # has one: the app's icon, in "<file>.files".
    icon = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "Rune", "Assets.xcassets", "AppIcon.appiconset", "AppIcon.png")
    if os.path.exists(icon):
        os.makedirs(path + ".files", exist_ok=True)
        shutil.copyfile(icon, os.path.join(path + ".files", PICTURE))
    print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

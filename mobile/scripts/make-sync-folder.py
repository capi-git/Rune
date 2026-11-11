#!/usr/bin/env python3
"""Play the part of the Mac in sync: write the files of docs/sync-format.md.

The Mac app does not sync yet. Until it does, this tool lets the iPhone app be
tried against real files, and it shows a writer what to do: give every change
a version, then write the records that changed to the next change file.

    make-sync-folder.py init SOURCE.db DIR       start: a snapshot of SOURCE.db in DIR/sync-v1
    make-sync-folder.py rename DIR "Page" "New"  change a page's title
    make-sync-folder.py todo DIR "Page" "Text"   add a to-do at the end of a page
    make-sync-folder.py todo DIR "Page" "Text" 2026-11-12   the same, due on that day
    make-sync-folder.py font DIR "Page" serif    set the face of a page's title (serif, mono, or "" for sans)
    make-sync-folder.py event DIR "Title" 90     add an event that starts in 90 minutes
    make-sync-folder.py trash DIR "Page"         put a page in the trash
    make-sync-folder.py cover DIR "Page" sea     give a page one of the app's covers
    make-sync-folder.py picture DIR "Page" FILE  add a picture at the end of a page
    make-sync-folder.py snapshot DIR             write a new snapshot

DIR stands for the "Documents" folder of the iCloud container. The Mac's own
database (a copy of SOURCE.db) is kept beside it in DIR.mac/workspace.db: a
live database is never inside the sync folder.
"""

import json
import os
import shutil
import sqlite3
import sys
import time
import uuid

FORMAT = 1
SYNC_TABLES = """
CREATE TABLE IF NOT EXISTS sync_records(kind TEXT NOT NULL, id TEXT NOT NULL, clock INTEGER NOT NULL, device TEXT NOT NULL,
    synced_clock INTEGER, synced_device TEXT, removed INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(kind, id));
CREATE TABLE IF NOT EXISTS sync_peers(device TEXT PRIMARY KEY, seq INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS sync_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
"""


def now_ms() -> int:
    return int(time.time() * 1000)


def seq_name(seq: int) -> str:
    return f"{seq:010d}"


class Mac:
    """The Mac's own database, with the sync tables."""

    def __init__(self, folder: str):
        self.root = os.path.join(folder, "sync-v1")
        self.home = folder.rstrip("/") + ".mac"
        self.path = os.path.join(self.home, "workspace.db")
        self.db = sqlite3.connect(self.path)
        self.db.row_factory = sqlite3.Row

    def meta(self, key: str) -> str:
        row = self.db.execute("SELECT value FROM sync_meta WHERE key=?", (key,)).fetchone()
        return row[0] if row else ""

    def set_meta(self, key: str, value) -> None:
        self.db.execute(
            "INSERT INTO sync_meta(key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            (key, str(value)),
        )

    # -- every write to a record raises the clock and sets the record's version

    def touch(self, kind: str, record_id: str, removed: bool = False) -> None:
        clock = int(self.meta("clock") or 0) + 1
        self.set_meta("clock", clock)
        self.db.execute(
            """INSERT INTO sync_records(kind, id, clock, device, removed) VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(kind, id) DO UPDATE SET clock=excluded.clock, device=excluded.device, removed=excluded.removed""",
            (kind, record_id, clock, self.meta("device"), int(removed)),
        )

    # -- a record as it travels

    def body(self, kind: str, record_id: str):
        if kind == "page":
            p = self.db.execute("SELECT * FROM pages WHERE id=?", (record_id,)).fetchone()
            if p is None:
                return None
            # A column that a database from an older Mac app lacks reads as "".
            def text(row, name):
                return (row[name] if name in row.keys() else "") or ""

            blocks = [
                {
                    "id": b["id"], "kind": b["kind"], "text": b["text"], "checked": bool(b["checked"]), "indent": b["indent"],
                    "collapsed": bool(b["collapsed"]), "icon": b["icon"], "language": b["language"], "target": b["target"],
                    "data": b["data"] or "", "place": text(b, "place"), "due": text(b, "due"),
                }
                for b in self.db.execute("SELECT * FROM blocks WHERE page_id=? ORDER BY position", (record_id,))
            ]
            return {
                "space_id": p["space_id"], "parent_id": p["parent_id"], "title": p["title"], "icon": p["icon"],
                "position": p["position"], "favorite": bool(p["favorite"]), "trashed": bool(p["trashed"]),
                "created_at": p["created_at"], "updated_at": p["updated_at"],
                "cover": p["cover"], "cover_y": p["cover_y"], "font": text(p, "font"), "blocks": blocks,
            }
        if kind == "event":
            e = self.db.execute("SELECT * FROM events WHERE id=?", (record_id,)).fetchone()
            if e is None:
                return None
            return {
                "space_id": e["space_id"], "title": e["title"], "start_ms": e["start_ms"], "end_ms": e["end_ms"],
                "all_day": bool(e["all_day"]), "color": e["color"], "notes": e["notes"], "page_id": e["page_id"],
                "created_at": e["created_at"], "updated_at": e["updated_at"], "remind_min": e["remind_min"], "repeat": e["repeat"],
            }
        s = self.db.execute("SELECT * FROM spaces WHERE id=?", (record_id,)).fetchone()
        if s is None:
            return None
        return {"name": s["name"], "color": s["color"], "position": s["position"], "created_at": s["created_at"]}

    # -- files: made beside the database first, then moved into the sync folder

    def place(self, data: bytes, *parts: str) -> None:
        target = os.path.join(self.root, *parts)
        os.makedirs(os.path.dirname(target), exist_ok=True)
        temp = os.path.join(self.home, f"out-{uuid.uuid4().hex}")
        with open(temp, "wb") as f:
            f.write(data)
        os.replace(temp, target)

    # -- pictures: files beside the database; each goes to the sync folder once

    def pictures_of(self, body) -> list:
        names = [b["data"] for b in body.get("blocks", []) if b["kind"] == "image" and b["data"]]
        cover = body.get("cover") or ""
        if cover.startswith("file:"):
            names.append(cover[len("file:"):])
        return names

    def publish_pictures(self, names) -> None:
        for name in names:
            source = os.path.join(self.home, "Files", name)
            target = os.path.join(self.root, "files", name)
            if os.path.isfile(source) and not os.path.exists(target):
                with open(source, "rb") as f:
                    self.place(f.read(), "files", name)

    def next_seq(self) -> int:
        seq = int(self.meta("seq") or 0) + 1
        self.set_meta("seq", seq)
        return seq

    def write_device_file(self, name: str = "") -> None:
        device = self.meta("device")
        name = name or self.meta("name") or "Mac"
        self.set_meta("name", name)
        snapshot_seq = int(self.meta("snapshot_seq") or 0)
        info = {
            "format": FORMAT, "device": device, "name": name, "kind": "mac", "at": now_ms(),
            "seq": int(self.meta("seq") or 0),
            "applied": {r["device"]: r["seq"] for r in self.db.execute("SELECT device, seq FROM sync_peers")},
        }
        if snapshot_seq:
            info["snapshot"] = {"seq": snapshot_seq, "clock": int(self.meta("snapshot_clock") or 0)}
        self.place(json.dumps(info, indent=2).encode(), "devices", f"{device}.json")

    def flush(self) -> int:
        """Write every record whose version differs from its synced version."""
        dirty = self.db.execute(
            """SELECT kind, id, clock, device, synced_clock, synced_device, removed FROM sync_records
               WHERE synced_clock IS NULL OR clock != synced_clock OR device != synced_device
               ORDER BY clock"""
        ).fetchall()
        if not dirty:
            return 0
        changes = []
        for r in dirty:
            change = {
                "kind": r["kind"], "id": r["id"],
                "version": {"clock": r["clock"], "device": r["device"]},
                "base": None if r["synced_clock"] is None else {"clock": r["synced_clock"], "device": r["synced_device"]},
            }
            body = None if r["removed"] else self.body(r["kind"], r["id"])
            if body is None:
                change["removed"] = True
            else:
                change[r["kind"]] = body
                # A picture is in the folder before the change that names it.
                if r["kind"] == "page":
                    self.publish_pictures(self.pictures_of(body))
            changes.append(change)
        seq = self.next_seq()
        batch = {"format": FORMAT, "device": self.meta("device"), "seq": seq, "at": now_ms(), "changes": changes}
        self.place(json.dumps(batch, ensure_ascii=False).encode(), "changes", self.meta("device"), f"{seq_name(seq)}.json")
        for r in dirty:
            self.db.execute(
                "UPDATE sync_records SET synced_clock=clock, synced_device=device WHERE kind=? AND id=? AND clock=? AND device=?",
                (r["kind"], r["id"], r["clock"], r["device"]),
            )
        self.db.commit()
        self.write_device_file()
        self.db.commit()
        return len(changes)

    def snapshot(self) -> int:
        """A complete, cleaned copy: no settings but text_format, no history."""
        self.flush()
        for row in self.db.execute("SELECT id FROM pages").fetchall():
            self.publish_pictures(self.pictures_of(self.body("page", row[0])))
        seq = self.next_seq()
        self.set_meta("snapshot_seq", seq)
        self.set_meta("snapshot_clock", self.meta("clock") or 0)
        self.db.commit()
        temp = os.path.join(self.home, f"snapshot-{uuid.uuid4().hex}.db")
        self.db.execute("VACUUM INTO ?", (temp,))
        copy = sqlite3.connect(temp)
        copy.execute("DELETE FROM settings WHERE key != 'text_format'")
        copy.execute("DELETE FROM page_versions")
        copy.execute("DELETE FROM sync_meta WHERE key NOT IN ('device', 'clock', 'seq')")
        copy.commit()
        copy.execute("PRAGMA journal_mode=DELETE")
        copy.execute("VACUUM")
        copy.close()
        target = os.path.join(self.root, "snapshots", self.meta("device"), f"{seq_name(seq)}.db")
        os.makedirs(os.path.dirname(target), exist_ok=True)
        os.replace(temp, target)
        self.write_device_file()
        self.db.commit()
        return seq

    def page_id(self, title: str) -> str:
        row = self.db.execute("SELECT id FROM pages WHERE title=? AND trashed=0 ORDER BY updated_at DESC", (title,)).fetchone()
        if row is None:
            sys.exit(f'No page named "{title}".')
        return row[0]


def init(source: str, folder: str, name: str) -> None:
    if os.path.basename(source) == "workspace.db":
        sys.exit("That is the name of the live file. Use a backup copy.")
    home = folder.rstrip("/") + ".mac"
    os.makedirs(home, exist_ok=True)
    os.makedirs(folder, exist_ok=True)
    path = os.path.join(home, "workspace.db")
    for old in (path, path + "-journal"):
        if os.path.exists(old):
            os.remove(old)
    shutil.copyfile(source, path)
    # The Mac's pictures: "Files" beside its database.
    shutil.rmtree(os.path.join(home, "Files"), ignore_errors=True)
    for pictures in (source + ".files", os.path.join(os.path.dirname(source), "..", "Files")):
        if os.path.isdir(pictures):
            shutil.copytree(pictures, os.path.join(home, "Files"))
            break
    mac = Mac(folder)
    mac.db.executescript(SYNC_TABLES)
    # The columns a database from an older Mac app lacks.
    pages = [c[1] for c in mac.db.execute("PRAGMA table_info(pages)")]
    blocks = [c[1] for c in mac.db.execute("PRAGMA table_info(blocks)")]
    if "cover" not in pages:
        mac.db.executescript("ALTER TABLE pages ADD COLUMN cover TEXT; ALTER TABLE pages ADD COLUMN cover_y INTEGER NOT NULL DEFAULT 50;")
    if "font" not in pages:
        mac.db.executescript("ALTER TABLE pages ADD COLUMN font TEXT NOT NULL DEFAULT '';")
    if "place" not in blocks:
        mac.db.executescript("ALTER TABLE blocks ADD COLUMN place TEXT NOT NULL DEFAULT '';")
    if "due" not in blocks:
        mac.db.executescript("ALTER TABLE blocks ADD COLUMN due TEXT NOT NULL DEFAULT '';")
    mac.db.execute("CREATE TABLE IF NOT EXISTS page_versions(id INTEGER PRIMARY KEY AUTOINCREMENT, page_id TEXT NOT NULL, at INTEGER NOT NULL, title TEXT NOT NULL DEFAULT '', blocks TEXT NOT NULL)")
    mac.set_meta("device", uuid.uuid4().hex)
    mac.set_meta("clock", 0)
    mac.set_meta("seq", 0)
    # The first time sync is on, every record gets a version.
    for kind, table in (("space", "spaces"), ("page", "pages"), ("event", "events")):
        for row in mac.db.execute(f"SELECT id FROM {table}").fetchall():
            mac.touch(kind, row[0])
    mac.db.execute("UPDATE sync_records SET synced_clock=clock, synced_device=device")
    mac.db.commit()
    seq = mac.snapshot()
    mac.write_device_file(name)
    mac.db.commit()
    print(f"Sync folder made in {mac.root} (snapshot {seq}).")


def main() -> int:
    args = sys.argv[1:]
    if len(args) < 2:
        print(__doc__)
        return 2
    command = args[0]
    if command == "init":
        if len(args) < 3:
            print(__doc__)
            return 2
        init(args[1], args[2], args[3] if len(args) > 3 else "MacBook Pro")
        return 0

    mac = Mac(args[1])
    if not os.path.exists(mac.path) or not mac.meta("device"):
        sys.exit("Run init first.")
    at = now_ms()
    if command == "rename" and len(args) == 4:
        page = mac.page_id(args[2])
        mac.db.execute("UPDATE pages SET title=?, updated_at=? WHERE id=?", (args[3], at, page))
        mac.touch("page", page)
    elif command == "todo" and len(args) in (4, 5):
        page = mac.page_id(args[2])
        position = mac.db.execute("SELECT coalesce(max(position), -1) + 1 FROM blocks WHERE page_id=?", (page,)).fetchone()[0]
        mac.db.execute(
            "INSERT INTO blocks(id, page_id, position, kind, text, due) VALUES (?, ?, ?, 'todo', ?, ?)",
            (uuid.uuid4().hex, page, position, args[3], args[4] if len(args) == 5 else ""),
        )
        mac.db.execute("UPDATE pages SET updated_at=? WHERE id=?", (at, page))
        mac.touch("page", page)
    elif command == "font" and len(args) == 4:
        page = mac.page_id(args[2])
        mac.db.execute("UPDATE pages SET font=?, updated_at=? WHERE id=?", (args[3], at, page))
        mac.touch("page", page)
    elif command == "event" and len(args) == 4:
        space = mac.db.execute("SELECT id FROM spaces ORDER BY position, created_at LIMIT 1").fetchone()[0]
        start = at + int(args[3]) * 60_000
        event = uuid.uuid4().hex
        mac.db.execute(
            "INSERT INTO events(id, space_id, title, start_ms, end_ms, color, created_at, updated_at, remind_min) VALUES (?, ?, ?, ?, ?, 'green', ?, ?, 10)",
            (event, space, args[2], start, start + 3_600_000, at, at),
        )
        mac.touch("event", event)
    elif command == "trash" and len(args) == 3:
        page = mac.page_id(args[2])
        mac.db.execute("UPDATE pages SET trashed=1, updated_at=? WHERE id=?", (at, page))
        mac.touch("page", page)
    elif command == "cover" and len(args) == 4:
        page = mac.page_id(args[2])
        mac.db.execute("UPDATE pages SET cover=?, cover_y=50, updated_at=? WHERE id=?", (f"preset:{args[3]}", at, page))
        mac.touch("page", page)
    elif command == "picture" and len(args) == 4:
        page = mac.page_id(args[2])
        ending = os.path.splitext(args[3])[1].lower().lstrip(".").replace("jpeg", "jpg")
        name = f"{uuid.uuid4().hex[:20]}.{ending}"
        os.makedirs(os.path.join(mac.home, "Files"), exist_ok=True)
        shutil.copyfile(args[3], os.path.join(mac.home, "Files", name))
        position = mac.db.execute("SELECT coalesce(max(position), -1) + 1 FROM blocks WHERE page_id=?", (page,)).fetchone()[0]
        mac.db.execute(
            "INSERT INTO blocks(id, page_id, position, kind, text, data) VALUES (?, ?, ?, 'image', '', ?)",
            (uuid.uuid4().hex, page, position, name),
        )
        mac.db.execute("UPDATE pages SET updated_at=? WHERE id=?", (at, page))
        mac.touch("page", page)
    elif command == "snapshot" and len(args) == 2:
        print(f"Snapshot {mac.snapshot()} written.")
        return 0
    else:
        print(__doc__)
        return 2
    mac.db.commit()
    print(f"{mac.flush()} change(s) written.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

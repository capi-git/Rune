# Sync between devices: the files (format 1)

How Rune on the Mac and on the iPhone keep the same notes with no server
of ours. Decided on 2026-11-09 (see `docs/mobile-plan.md`, decision 2). Both
apps must follow this file exactly.

The files of this document travel in two ways, each on its own or both at once
(decided on 2026-11-10): through the user's own iCloud Drive, which this file
describes, and straight between the Mac and the iPhone over the local network,
which `docs/sync-wifi.md` describes. The files and the rules are the same.

State on 2026-11-11: the iPhone reads (`mobile/Core/Sources/RuneCore/Sync*.swift`,
tested). The Mac writes and reads (`src/devsync/engine.rs`, tested; on from
Settings ▸ Sync, off until then); `mobile/scripts/make-sync-folder.py` is
still a reference writer for tests. Covers, pictures and
tables (added to the Mac app on 2026-11-09) are in, and so are a page's font,
a picture's place and caption, and a to-do's due day (2026-11-10, the Mac's
schema mark 4). Since 2026-11-11 also the page of a table's row (`row_of`), a
to-do's reminder (`remind`) and an event's call link (`join_url`): the Mac's
schema mark 6 (Rune 0.1.3 and later). A table's column kinds, view and rows,
and a chart, are in the block's `data` and pass through as they are.
Since 2026-11-12 a page can be locked with a password: its whole block list
is then one block of the kind `locked`, whose `data` holds the page's lines
sealed (see `src/lock.rs`); it passes through as it is, and a device that
cannot open it must not change it (the Mac's schema mark 7).

## The idea

- Each device keeps its own SQLite database in its own private folder. A live
  database file never goes in the sync folder.
- A device tells the others what changed by writing small files into a shared
  folder. A file is written once, by one device, under a name no other device
  uses, and is never changed afterwards. So iCloud Drive never has to merge.
- What syncs: spaces, pages (with their blocks and covers), events, and the
  pictures that pages name. Settings and page history stay on each device.
- The unit is one record: one page with its title, icon, place and all of its
  blocks; one event; one space.

## The folder

The folder belongs to the iPhone app's iCloud container
`iCloud.com.jackhagan.notion2`. The iPhone app makes it. The Mac app must not
make it: if it is not there, sync is not available yet ("open Rune on the
iPhone once").

| On | Path |
| --- | --- |
| Mac | `~/Library/Mobile Documents/iCloud~com~jackhagan~notion2/Documents/sync-v1/` |
| iPhone | `<ubiquity container>/Documents/sync-v1/` |

```
sync-v1/
  devices/<device>.json             who a device is and how far it is (rewritten by its owner only)
  snapshots/<device>/<seq>.db       a complete copy of that device's notes (written once)
  changes/<device>/<seq>.json       what that device changed (written once)
  files/<name>                      a picture (written once, by the device that added it)
```

- `<device>`: 32 lower-case hex characters, made once when the app is installed.
- `<seq>`: the device's own write counter, 10 digits with leading zeros,
  starting at `0000000001`. Snapshots and change files share the one counter, so
  the files of one device have one order.
- Write a file somewhere else first (the app's own folder, on the same volume),
  then move it into place, so a half-written file is never seen.
- A device writes only in its own three places and never changes or replaces a
  snapshot or change file.
- A file that is listed but cannot be read yet (iCloud has not brought it) is
  "not here yet": try again later. Never skip it.

### More than one folder

With sync over the local network (`docs/sync-wifi.md`) there are more folders
of this layout than the one in iCloud Drive: the Mac keeps its files in a
folder of its own (and copies them into the iCloud folder when that exists),
and the iPhone keeps a copy of the Mac's folder that it fetched.

- A device reads every folder it has, with the rules below. The same file from
  two folders is applied once: a file with a number at or below the last one
  applied for that device is passed over.
- A device writes its own files in each folder it has (the iPhone its device
  file; later its change files), and a file with the same name has the same
  content in every folder. A device file may differ in `at`.
- The rule "the Mac app must not make the folder" holds for the folder in
  iCloud Drive only.

## Versions

Every record has a version: `{"clock": 57, "device": "<device>"}`.

- A device keeps one number, its clock. When it changes a record, the clock goes
  up by one and the record's version becomes (clock, this device).
- When it applies a change from another device, its clock becomes at least that
  change's clock.
- Version A is newer than version B when its clock is higher, or the clocks are
  equal and its device id is the greater string.

Wall-clock time is never used to decide anything.

## A change file

```json
{
  "format": 1,
  "device": "5f0c…",
  "seq": 7,
  "at": 1794268000000,
  "changes": [
    {
      "kind": "page",
      "id": "2204c56d…",
      "version": {"clock": 57, "device": "5f0c…"},
      "base": {"clock": 41, "device": "9a1e…"},
      "page": {
        "space_id": "bed57996…", "parent_id": null, "title": "Mobile app", "icon": "rocket:orange",
        "position": 3.0, "favorite": true, "trashed": false, "created_at": 1794000000000, "updated_at": 1794268000000,
        "cover": "preset:sea", "cover_y": 50, "font": "serif", "row_of": null,
        "blocks": [
          {"id": "…", "kind": "todo", "text": "Sync with the Mac", "checked": false, "indent": 0,
           "collapsed": false, "icon": null, "language": null, "target": null, "data": "",
           "place": "", "due": "2026-11-12", "remind": "09:00"}
        ]
      }
    },
    {
      "kind": "event",
      "id": "…",
      "version": {"clock": 58, "device": "5f0c…"},
      "base": null,
      "event": {
        "space_id": "…", "title": "Team sync", "start_ms": 1794268800000, "end_ms": 1794272400000, "all_day": false,
        "color": "blue", "notes": "", "page_id": null, "created_at": 0, "updated_at": 0, "remind_min": 10,
        "repeat": "{\"every\":\"week\",\"interval\":1}", "join_url": ""
      }
    },
    {
      "kind": "space",
      "id": "…",
      "version": {"clock": 59, "device": "5f0c…"},
      "base": null,
      "space": {"name": "Work", "color": "green", "position": 2.0, "created_at": 0}
    },
    {"kind": "page", "id": "…", "version": {"clock": 60, "device": "5f0c…"}, "base": {"clock": 12, "device": "5f0c…"}, "removed": true}
  ]
}
```

- The names inside `page`, `event` and `space` are the column names of the
  database (`src/store.rs`); `blocks` is the page's whole block list in order
  (`kind` as stored: `text`, `heading1`, …, `page`, `drawing`, `image`, `table`,
  `chart`, `locked`).
  Numbers that are 0 or 1 in the database are `true` / `false` here.
- `cover` is the page's cover as stored (`null`, `preset:<name>` or
  `file:<name>`), `cover_y` the part of a cover picture that shows (0 to 100; 50
  when it is left out).
- `font` is the face of the page's title and headings as stored: `""` (sans),
  `serif` or `mono` (`""` when it is left out).
- In a block, `place` and `due` are the columns of the same names, as text
  exactly as stored (`""` when left out): `place` is where a picture sits and
  how wide it is (JSON such as `{"free":true,"right":true,"x":42,"y":0,"w":300}`;
  `Place` in `src/model.rs`), `due` the day a to-do is due (`2026-11-12`). A
  picture's caption is the `text` of its `image` block. `remind` is the time of
  a to-do's reminder on its due day (`09:00`; `""` for none). For a table,
  `place` only says whether it stands beside the table above it
  (`{"beside":true,…}`).
- `tint` (since Rune 0.1.17) is the colour of a line: `red` for its words, `red-bg` for a band, or both with the words first (`red blue-bg`):
  behind it (gray, brown, orange, yellow, green, blue, purple, pink, red), or `""` for none.
  A device that does not know colours leaves the key out; the line then keeps the colour it
  has on the device that reads the change.
- `row_of` marks the page of a row of a table: `"<block id>/<row id>"` (or
  `"<block id>/template"` for the page a new row starts from); `null` or left
  out for an ordinary page. Such a page is not listed among the pages: it
  opens from its row.
- `join_url` is the address to join an event's call (`""` when left out).
- A block passes through whole. A device that does not know a block's `kind`
  keeps the `kind` and the `data` exactly as they came, and writes them back
  unchanged. The same holds for a `cover` it cannot draw, and for `font`,
  `place`, `due`, `remind`, `row_of` and `join_url`: a device never reads them to write them again in its own
  form.
- A page's history entry (`page_versions.blocks`) is in the desktop's form
  (`Vec<Block>` in `src/model.rs`): there `place` is the object itself and is
  left out when it is empty, and `due` and `remind` are left out when there is
  none.
- A record is always sent whole. `base` is the version the writer last wrote to,
  or read from, the sync folder for this record (`null`: never).
- `"removed": true` means the record was deleted for good (a page removed from
  the trash, an event deleted). A page in the trash is a normal change with
  `"trashed": true`.
- `at` (milliseconds since 1970) is only for people to read.
- Keep a file under about 1 MB: more changes go in more files.
- A reader ignores names it does not know. A file with `format` greater than 1
  stops the reading of that device's files until the app is updated.

## Applying a change

Read each device's files in order of `seq`, with no gaps: when the next number
is missing, wait. The order between devices does not matter.

For each change, with L the version this device has for that record:

1. No L: take the change.
2. The change's version equals L: nothing to do.
3. The change's `base` equals L: take the change. This is the normal case.
4. Anything else means the record was changed in two places at once. The winner
   is the one that is not a removal; if both or neither are removals, the newer
   version. Every device picks the same winner.
   - The change wins: for a page, first save the page as it is here to its
     history (`page_versions`), then take the change.
   - The change loses: for a page, save the change's page to the history, and
     keep what is here.
   - A locked page against plain lines (the Mac, since 2026-11-12): when one of
     the two pages is one `locked` block and the other is not, and neither is a
     removal, the locked one wins whatever the versions say.
     - The Mac has the plain lines and the change is the locked page: it saves
       its plain lines to the history, then takes the change (as above).
     - The Mac has the locked page and the change is the plain lines: it keeps
       the locked page and does not save the plain lines to that page's
       history. When the plain change is the newer version, the Mac gives its
       locked page a new version, newer than the change's, and writes it
       again with the `base` it had (not the change's version). The device
       that has the plain lines then sees two changes at once (rule 4), the
       locked page wins there as the newer version, and that device saves its
       own lines to its history.
     The files are as before.

"Take the change" replaces the record's row (and all blocks of a page) and sets
the record's version to the change's. Taking a removal deletes the rows and keeps
the version, marked removed, so that an older change cannot bring the record
back. Nothing is lost silently: a page that loses is in that page's history on
every device that saw both. One case is narrower: plain lines that lose to a
locked page are in the history of the device that wrote them, and not in the
history of a Mac that holds the locked page.

## Pictures

A picture (an `image` block names one in its `data`; a cover names one as
`file:<name>`) is a file in the folder `Files` beside the Mac's database
(`src/files.rs`), not a row. Its name is made once (letters and digits, a dot, a
known ending) and its content never changes, so it fits the rule "written once":

- Before a device writes a change (or a snapshot) that names a picture, it puts
  the picture in `files/<name>`, if it is not there yet. Made beside the
  database first, then moved in, like every file.
- A reader does not have to fetch pictures to apply a change. It fetches a
  picture when a page shows it. Until the file is here the page shows a stand-in.
- A device may keep its own copy (its `Files` folder) or read from `files/`.
- Pictures are not removed in format 1 (a later format can remove those that no
  page names).

## What each database keeps

```sql
CREATE TABLE IF NOT EXISTS sync_records(
    kind TEXT NOT NULL,            -- 'page', 'event' or 'space'
    id TEXT NOT NULL,
    clock INTEGER NOT NULL,        -- the version of the record as it is in this database
    device TEXT NOT NULL,
    synced_clock INTEGER,          -- the version last written to, or read from, the folder
    synced_device TEXT,            --   (NULL: never). Different from the version: to be written.
    removed INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(kind, id)
);
CREATE TABLE IF NOT EXISTS sync_peers(
    device TEXT PRIMARY KEY,
    seq INTEGER NOT NULL           -- the last file of that device that was applied
);
CREATE TABLE IF NOT EXISTS sync_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- 'device': this database's device id, 'clock': its clock, 'seq': its last seq
```

## Writing (the Mac now; the iPhone when it can edit)

- Every write to a page, its blocks, an event or a space gives that record a
  new version. On the Mac, triggers in the notes file note each written
  record in `sync_dirty` (kind, id), whichever program writes (the app,
  `rune --mcp`, an older Rune); when the engine runs it gives each noted
  record one clock step and empties the list. While the engine applies
  another device's change it holds the row `applying` in `sync_meta` inside
  its transaction, and the triggers pass over those writes. A record made and
  removed between two runs never travels. A snapshot has no triggers and no
  `sync_dirty`.
- A few seconds after the last change, and when the app quits, one task (off the
  UI thread, like `backup.rs`) takes every record whose version differs from its
  synced version, writes them to the next change file with `base` = the synced
  version, and then sets synced = version for the records that did not change
  again in the meantime. If the app stops between the two steps the same changes
  are written again later; readers skip them (rule 2).
- The same task reads the other devices' files and applies them in one
  transaction per file. The windows then reload by themselves: a change from
  another connection is seen through `PRAGMA data_version` (`src/sync.rs`).
- The first time sync is turned on, every existing record gets a version, and
  the device writes a snapshot.

## Snapshots

A snapshot lets a new device start, and lets old change files go.

- It is a complete SQLite file in rollback-journal mode (one file, no `-wal`):
  `VACUUM INTO` a file outside the sync folder, clean it, then move it in.
- Before it is written, every picture that its pages name is in `files/`.
- It holds `spaces`, `pages`, `blocks`, `events`, the three sync tables with
  `sync_meta.seq` = the snapshot's own seq, and `settings` with only the row
  `text_format`. No other settings and no rows in `page_versions`.
- A device with no notes yet takes the newest snapshot (`devices/*.json` says
  which: the highest `snapshot.clock`) as its database, sets `sync_meta.device`
  to its own id, and goes on from there: the author's files after the snapshot's
  seq, and each other device's files after the seq in the snapshot's
  `sync_peers`.
- A device that finds a gap that does not close (its next file of device X is
  gone and X has a snapshot at or after that number) takes that snapshot record
  by record, with the rules above (a record with no `base`).
- A device writes a new snapshot when it has written about 500 change files
  since its last one. It may then delete its own change files that are both
  older than 30 days and before that snapshot, and its snapshots except the two
  newest.

## The device file

`devices/<device>.json`, rewritten by its owner after it wrote or applied
something. It is the only file that is ever rewritten.

```json
{
  "format": 1,
  "device": "5f0c…",
  "name": "MacBook Pro",
  "kind": "mac",
  "at": 1794268000000,
  "seq": 57,
  "snapshot": {"seq": 12, "clock": 340},
  "applied": {"9a1e…": 9}
}
```

`seq` is the last file this device wrote (so a reader knows whether more is on
the way), `snapshot` its newest snapshot (absent: none), `applied` the last file
of each other device it has applied. `kind` is `mac` or `iphone`.

## The schema mark

The Mac marks its notes file with `PRAGMA user_version` (`store::SCHEMA`; 4 on
2026-11-10) and a program built for a smaller number reads the file and does
not write it. A snapshot is a copy of that file, so it carries the mark.

- A reader takes a snapshot whatever its mark is: it reads the columns it
  knows by name and keeps the rest of the file as it is.
- A device that writes (the iPhone, when it can edit) keeps the number it was
  built for. Before it changes a record in a database whose mark is greater,
  it must stop and ask for an update: it would write rows without the columns
  it does not know.

## Not in format 1

- A drawing's scene and a table's cells travel inside their page each time the
  page changes. Large drawings make large files; a later format can send them
  apart. (Pictures already travel apart, as files.)
- One page is one unit: a move of the page on one device and an edit of its text
  on the other, at the same moment, is a conflict (the loser is in the history).
  A later format can split place and content.

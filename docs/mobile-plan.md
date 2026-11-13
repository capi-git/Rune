# Rune on the iPhone: plan

Written 2026-11-09. The goal: read, and later edit, the same notes and calendar on
the user's own iPhone. One user, no App Store, no server of ours.

The designs are the canvas "Notion 2.0 Mobile"
(<https://claude.ai/artifact/6JSQPxSGL3HJUx8UHTgJZb>): Home, Pages, Page, Insert,
Search, Calendar, Ask AI and Settings, each in light and dark.

## Milestones

| | What | State |
| --- | --- | --- |
| 1 | Read-only app: opens a copy of `workspace.db`; spaces, nested pages, every block kind, calendar (week and month, repeats), search | built in `mobile/` |
| 2 | Sync, Mac to iPhone: the phone follows the Mac through iCloud Drive, and straight over Wi-Fi after a QR code paired the two (decision 2) | built on both sides (Mac: `src/devsync*`, Settings ▸ Sync, 2026-11-11) |
| 3 | Editing on the phone and sync in both directions: quick note, to-do ticks, the block editor and the Insert sheet, events | started 2026-11-11: events (new, change, delete), to-do ticks (page, Home, list), typing in a line with Return for the next line, renaming a page, a new line at the end, new pages; sent through the outbox by iCloud Drive and Wi-Fi. Not yet: the Insert sheet (other kinds of line, pictures), moving and nesting lines, deleting pages, a change to one repeat only, table cells |
| 4 | Reminders on the phone, Ask AI through the Mac | not started |

Sync comes before editing: a change made on a phone that cannot send it back would
be lost work, and a phone that follows the Mac is useful by itself.

Milestone 1 touches nothing outside `mobile/` and this file.

## Decision 1: what the app is made with

**Recommended: native SwiftUI, with the rules written again in Swift.**

| Option | For | Against |
| --- | --- | --- |
| **SwiftUI** | The real iPhone feel and the real system glass (iOS 26 "Liquid Glass" API), SQLite is already on the phone, small and fast, the simplest thing to sign and install | The rules that live in Rust (inline text, repeat rules, the block outline) exist twice |
| Dioxus mobile (reuse the desktop UI) | One code base | It is a web view. The desktop UI is made for a mouse, panes and hover. Glass would be a CSS copy, text editing in a web view on iOS is poor, and the iOS tooling is young |
| Rust core behind a Swift UI (UniFFI) | The rules exist once | The desktop crate is one binary, not a library: it would have to be split, which collides with the desktop work on `main`. Adds a cross-compile step and a bindings layer for about 1,000 lines of rules |

The cost of SwiftUI is kept small this way: the Swift rules live in one package
(`mobile/Core`, no UI code) and their tests use the same cases as the Rust tests in
`src/inline.rs`, `src/repeat.rs` and `src/model.rs`. When a rule changes on the
desktop, the matching Swift test case changes with it. The icon set is not copied
by hand: `mobile/scripts/gen-icons.py` writes it from `src/ui/icons.rs`.

If the shared rules grow a lot (for example a block-level merge for sync), move
them to a Rust library used by both apps at that point.

## Decision 2: how notes travel between the Mac and the iPhone

**Decided on 2026-11-09: option A, the user's own iCloud Drive with small change
files.** The exact files are in `docs/sync-format.md`.

**Decided on 2026-11-10: option C as well, straight over the home Wi-Fi, paired
with a QR code.** The user wants both. The files and the rules of option A are
used as they are; only the way they travel is added (`docs/sync-wifi.md`). Each
way works without the other, and with both on a change is applied once. What
each gives: iCloud Drive reaches the phone anywhere and while the Mac sleeps;
Wi-Fi needs no iCloud and no internet, and only works while both are on the
same network with the Mac's app open.

The options as they were weighed stay below.

Rules that hold for every option:

- The live SQLite file never goes into a synced folder. Each device keeps its own
  database in its own private folder.
- Nothing is ever lost silently. When both sides changed the same page, the copy
  that loses goes into that page's history (`page_versions`, which the desktop
  already shows as "Page history").

### Option A (recommended): the user's own iCloud Drive, with small change files

Each device writes small files that say "this page (or event, or space) is now
this". The files go in the iPhone app's iCloud Drive folder. iCloud carries them.
Each device reads the other's files and applies them to its own database.

- **Why it is safe.** A change file is written once, by one device, under a name no
  other device uses (`changes/<device id>/<counter>.json`). So iCloud never has two
  devices writing one file, which is where iCloud Drive makes duplicates. A full
  snapshot (a closed `VACUUM INTO` copy, the same thing a backup is) lets a new
  device start and lets old change files be removed.
- **Same page changed on both.** The whole page (title, icon, blocks) is one unit.
  The newer change wins; the older one is saved to that page's history first, so it
  can be read or restored. Different pages changed on each side never collide.
  Order is decided by a counter that each device raises past the highest it has
  seen (so a wrong clock cannot make an old change win), with the device id as the
  tie-break.
- **Deletes.** A page is trashed, not removed, so that is a normal change. Emptying
  the trash and deleting an event write a small "removed" file.
- **What it costs the user.** iCloud Drive must be on, on both devices. It uses a
  little iCloud storage (text only). Sync is not instant: usually seconds, now and
  then a minute or more, and the iPhone fetches when the app is open.
- **What the iPhone app needs.** The iCloud Documents capability (possible with the
  paid developer account that the Developer ID certificate shows).
- **What the desktop app needs** (Rust, not sandboxed, Developer ID; describe only,
  no change in this session):
  1. A device id and a `sync_state` table: per record, the last counter seen and a
     "changed here" mark.
  2. Marks set in the few places `src/store.rs` writes: `save_page`, `save_blocks`
     / `save_blocks_at`, `save_block_text`, `save_block_data`, `save_event`,
     `delete_event`, `save_space`, `delete_space`, `delete_pages`.
  3. A background task (like `backup.rs`, off the UI thread) that, a few seconds
     after a change, writes one change file per marked record into
     `~/Library/Mobile Documents/iCloud~com~jackhagan~notion2/Documents/`
     and that reads new files from other devices and applies them in one
     transaction. A plain folder path: no iCloud entitlement, no provisioning
     profile, no change to signing or to the self-updater.
  4. Nothing new for the windows: a change applied on another connection is already
     noticed through `PRAGMA data_version` and reloaded (`src/sync.rs`).
  5. A "Sync" row in Settings: on/off, last sync time, the devices seen.
- **To prove first** (it needs the real iPhone and the real iCloud account): that
  the phone app's iCloud folder appears on the Mac under
  `~/Library/Mobile Documents/`, and which permission prompt macOS shows the Mac
  app the first time it writes there. Seen on 2026-11-09: iCloud Drive is on on
  this Mac and that folder holds 90 folders of other apps' iCloud containers, so
  the method is the usual one; our own folder can only be checked after the first
  install on the iPhone.
- **Cannot be undone:** the first install on an iPhone makes the iCloud container
  `iCloud.com.jackhagan.notion2` in the developer account. Apple does not let a
  container be deleted.

### Option B: CloudKit private database

Apple stores each page and event as a record in the user's private iCloud
database and tells the other device at once.

- **For.** The quickest and most robust sync; Apple finds conflicts per record.
- **Against.** The most work, and nearly all of it on the Mac. A Rust app cannot
  call CloudKit directly, so the Mac needs a Swift helper process (like
  `helper/rune-listen.swift`) or Objective-C bindings. The Mac app also needs the
  iCloud entitlement and an embedded provisioning profile, which changes
  `scripts/bundle.sh` and the self-updater.
- **Same page changed on both.** The same rule as option A (newer wins, the other
  goes to history), with Apple telling us that there was a conflict.

### Option C: straight over the home Wi-Fi

The Mac app listens on the local network; the iPhone finds it (Bonjour), the two
are paired once with a code, and they exchange changes directly.

- **For.** No cloud at all.
- **Against.** It only syncs when both devices are on the same network and the Mac
  app is open and awake. Away from home the phone has its last state and its own
  edits wait. It puts a network listener in the desktop app (firewall prompt on the
  Mac, local-network prompt on the iPhone), which is new attack surface.
- **As decided on 2026-11-10** (`docs/sync-wifi.md`): the Mac is not searched for
  (no Bonjour); the QR code carries the Mac's name and address, the fingerprint of
  a certificate that only the Mac has, and a code that works once. The phone then
  holds a token in its keychain. HTTPS, with the certificate checked against the
  fingerprint. What the desktop app needs:
  1. Its own sync folder, `~/Library/Application Support/Rune/Sync/sync-v1/`,
     written by the same writer as option A (and copied into the iCloud folder
     when that exists).
  2. The certificate, made once; a listener for five requests, off the UI thread.
  3. In Settings, under Sync: "Sync over Wi-Fi" on/off, "Connect an iPhone…"
     (shows the QR code), the paired iPhones with a way to remove one.

### Option D: copy by hand (what milestone 1 does)

Send a backup file to the iPhone (AirDrop, or the Files app). The phone shows that
copy, read-only. Nothing to build on the desktop: `Backups/` already has the files.
One way only; edits on the phone could not come back.

**Recommendation.** Use D now (it works today), build A next. Move to B only if
iCloud Drive proves too slow in daily use; the change format and the conflict rule
stay the same, only the transport changes.

## Decision 3: how the app gets onto the iPhone without the App Store

**Recommended: install straight from Xcode with the user's own team; add TestFlight
later only if updates away from the Mac are wanted.**

| Way | Setup | Stays valid | Updates |
| --- | --- | --- | --- |
| **Direct install from Xcode** (or `mobile/scripts/install-on-iphone.sh`) | Turn on Developer Mode on the iPhone once, connect it by cable once | 1 year with the paid developer account (7 days with a free Apple ID) | Build and install again from the Mac; cable, or the same Wi-Fi after the first time. Data in the app stays |
| TestFlight, internal testing | One-time: an app record in App Store Connect, upload a build. No App Review for internal testers; nobody else sees the app | Each build runs 90 days, so a new build at least every 90 days | Over the air, in the TestFlight app, automatic if wanted |

The project is set to the team of the "Developer ID Application" certificate on
this Mac, with the bundle id `com.jackhagan.notion2.mobile` and automatic signing.
A Developer ID certificate only exists with a paid membership, so the 1-year
validity should apply; Xcode confirms it the first time it makes the profile.

## What milestone 1 is made of

```
mobile/
  Core/                Swift package "RuneCore": no UI, tested with `swift test`
    Sources/RuneCore
      Model.swift          pages, blocks, events, spaces (as in src/model.rs)
      WorkspaceDatabase    read-only SQLite reader (as in src/store.rs)
      Inline.swift         inline text form (as in src/inline.rs)
      Repeat.swift, When   repeat rules and dates (as in src/repeat.rs, src/when.rs)
      Outline.swift        toggles, list numbers, page tree (src/model.rs, state.rs)
      Search.swift         titles and text (as in the desktop quick find)
      Icons*.swift         the icon set, generated from src/ui/icons.rs
  Rune/                the SwiftUI app (Home, Pages, Page, Search, Calendar, Settings)
  Rune.xcodeproj
  scripts/             sample data, load a copy into the Simulator, install on iPhone
```

The app opens its own copy of the workspace file, read-only. It never opens a file
in place and never writes to it.

Requires iOS 26 or later (the system glass API). How to run it, install it and
load notes: `mobile/README.md`.

### Checked

- `swift test` in `mobile/Core`: 38 tests (database reader, inline text, repeat
  rules and dates with the clocks changing, page outline, page tree, search, icons).
- In the iOS 27 Simulator (iPhone 18 Pro and Pro Max), light and dark: Home, Pages,
  a long page with every block kind, Search, Calendar (week, month, an event),
  Settings, with sample notes and with a copy of the real backup.
- A release build for a real iPhone compiles (3.4 MB). Memory in use with the
  sample notes: about 26 MB; no processor use at rest.
- Sync, iPhone side (`swift test`: 55 tests in all, 17 for sync): starts from the
  newest snapshot, applies change files in order, waits at a gap or for a file
  that iCloud has not brought, starts again from a newer snapshot when old files
  are gone, removals, and a page changed in two places (the newer wins, the other
  is in the page's history in the form the desktop reads).
- Sync in the Simulator, with a folder in place of iCloud Drive and
  `mobile/scripts/make-sync-folder.py` in place of the Mac: the app started with
  no notes and took the snapshot; a rename, a new to-do, a new event and a page
  put in the trash showed within about ten seconds, also on a page that was open;
  Settings showed "Up to date" and both devices.
- Covers, pictures and tables (64 tests in all): read from the database, drawn
  in the Simulator in both themes (a table that fits, one that moves sideways, a
  picture, a missing picture, a cover of each kind), and carried by sync (a
  cover and a picture added by the stand-in Mac showed on the phone).
- Sync over Wi-Fi, iPhone side (85 tests in all, 21 for this): the link of the
  QR code, the names of the files that may travel, which files the phone
  fetches, and whole runs against `mobile/scripts/wifi-mac.py` in place of the
  Mac (pairing, the first notes, changes, a picture, a code used twice, another
  certificate at the Mac's address, a token the Mac forgot, a file of another
  device, a second address when the first does not answer).
- Sync over Wi-Fi in the Simulator, both themes: the link opened the app, which
  asked "Connect to MacBook Pro?"; the notes and a picture came; a rename and a
  new to-do showed within about ten seconds; with the stand-in Mac stopped,
  Settings said that it waits for the Mac; a used code gave a plain message;
  "Disconnect" made the Mac forget the phone and left the notes and pictures.
  With the stand-in folder for iCloud Drive on as well, a new event and a cover
  came once (no conflict, nothing in the page history).
- Not checked: a real iPhone (none was connected), so also not AirDrop of a
  backup file, not the signing profile, and not iCloud Drive itself: that the
  folder appears on the Mac, how long a file takes, and that iOS reports files
  that are still on their way as the app expects.
- On the user's iPhone (iPhone 16 Pro Max, iOS 27), 2026-11-10: installed with
  `mobile/scripts/install-on-iphone.sh --no-icloud` (Apple would not make a
  profile with iCloud until a new developer agreement is accepted, so this build
  has no iCloud capability and no iCloud container exists yet). Sync over Wi-Fi
  against `wifi-mac.py --lan --qr` with a copy of the newest backup: the camera
  read the QR code and opened the app, the app paired and took the snapshot, and
  the calendar showed the real events. Not looked at on the phone: every screen
  in both themes, a change arriving, and "Disconnect".
- 86 tests since then: a phone that is paired with a Mac it has no notes from
  starts again from that Mac's snapshot.

### Learned while building, for the next milestones

- A sub-page is not always a link block in its parent (the real workspace has
  one without). The phone lists such pages under "Pages inside" at the end of the
  parent page. Sync must carry `parent_id`, not rely on link blocks.
- Search on the phone also ignores accents; the desktop quick find does not.
- The designs' "+" button (new page, new event) and the quick note on Home are
  left out until editing exists, so that no control does nothing.
- The week starts on the day the iPhone's region uses; the desktop always starts
  on Sunday.
- The Mac's data format grows while the phone is built (2026-11-09: page covers,
  `image` and `table` blocks, pictures as files in `Files`). The phone reads a
  file without the new columns and with them; a block kind it does not know
  shows as text and passes through sync unchanged; pictures travel in the sync
  folder (`files/`), so a backup file loaded by hand has none.
- The Mac's notes file gained more on 2026-11-10 (schema mark 4; Mac branch
  `pictures-tables-covers`): `blocks.place` (where a picture sits and how wide
  it is: JSON such as `{"free":true,"right":true,"x":42,"y":0,"w":300}`; a free
  picture sits at one side of the text column and the lines after it run beside
  it), `blocks.due` (the day a to-do is due, `YYYY-MM-DD`), `pages.font` (`""`,
  `serif` or `mono` for the title and headings), and a picture's caption in the
  `text` of its `image` block.
  - Done on the phone the same day: sync carries all of them through unchanged
    (`docs/sync-format.md`; tests in `SyncTests` and `WifiRunTests`), adds the
    columns to an older database, and writes them in a page's history in the
    desktop's form.
  - Not done on the phone: it does not show them. A to-do has no due chip
    ("Today", "Fri", red when past) and the calendar does not list the to-dos
    due on a day; a free picture shows in line at its own size (a fine
    fallback); every page is drawn in sans; a caption is not shown. In Markdown
    a to-do with a day ends `📅 2026-11-12` and a picture with a caption is
    `[Image: the caption]`.
  - Before the phone can edit (milestone 3): it must read these columns into
    its own model so that a rewrite of a page's blocks keeps them, keep the
    schema mark it was built for, and refuse to write a database with a greater
    mark (`docs/sync-format.md`, "The schema mark"). A snapshot must then be
    taken record by record, not whole, so that the phone's own changes stay.

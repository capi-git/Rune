# Rune for iPhone

The iPhone app for Rune (the Mac app first called Notion 2.0): the same spaces,
pages and calendar as the Mac app. Only what a person sees is called Rune; the
names inside (`Notion2`, `com.jackhagan.notion2.mobile`, `notion2://`) stay, as
on the Mac. It reads notes files up to form 6 (Rune 0.1.3 and later).
The plan, and the decisions still open (sync), are in
[`docs/mobile-plan.md`](../docs/mobile-plan.md).

**Today: the iPhone reads, it does not write.** It shows the notes of the Mac
and cannot change them. The notes arrive in one of two ways:

- **Sync**, in two ways that work alone or together (the files are described in
  [`docs/sync-format.md`](../docs/sync-format.md)):
  - through your own iCloud Drive. It reaches the iPhone anywhere, also while
    the Mac sleeps;
  - straight from the Mac over your Wi-Fi
    ([`docs/sync-wifi.md`](../docs/sync-wifi.md)). The Mac shows a QR code once,
    the iPhone's camera reads it, and from then on the two sync while they are
    on the same network and the Mac's app is open. No iCloud, no internet.

  The iPhone side of both is built. The Mac app does not write the files or
  show the QR code yet, so on a real iPhone the app says "Ready. No notes from
  your Mac yet" until it does.
- **A copy loaded by hand** (a backup file of the Mac app). This works today.

What it shows:

- Home: the pages changed last, the next events, the open to-dos from all pages
  (as on the Mac's Home; "All" opens the whole list, by page), favourites.
- Pages: spaces, favourites, the tree of pages inside pages, the trash.
- A page: its cover (one of the app's own, or a picture), and every block kind
  (text, headings, bulleted, numbered, to-do, toggle, quote, callout, code,
  divider, page link, table, chart, image), and a code block of the language
  `mermaid` as a diagram (with the Mac's own copy of Mermaid,
  `assets/mermaid`, linked in as `Notion2/Vendor/mermaid.min.js`). Tables show
  as on the Mac: column kinds (money, dates, tags, status, checkboxes,
  formulas), totals at the foot, the table's sort, filters and colour rules;
  the page of a row opens from the row. A drawing shows a placeholder. A picture
  whose file is not on the iPhone shows a stand-in: pictures are files beside
  the Mac's database, so they come with sync, not with a backup file.
- Calendar: a week strip or the whole month, the events of a day, repeating
  events on every day their rule falls on, reminders, the notes page of an event.
- Search: page names, the text inside pages, events.
- Light and dark mode (Settings: System, Light, Dark).

Editing (notes that came from sync only; a copy loaded by hand is read-only,
and so are notes from a newer Rune): events (the "+" in the calendar, "Edit"
on an event), to-do ticks, typing in a line (Return makes the next line, as on
the Mac), renaming a page, a new line at the end of a page, a new page (the
"+" on Pages). Each change gets a version (`NotesWriter`), goes into the
phone's outbox (`SyncEngine.push`) and from there into iCloud Drive and to the
Mac over Wi-Fi (`SyncEngine.publish`, then `WifiSync` uploads it).

Not built yet: the Insert sheet (other kinds of line, pictures), moving or
nesting lines, deleting pages, table cells, reminders on the phone, Ask AI,
page history, drawings, text that follows the iPhone's text-size setting.

Requires iOS 26 or later (the system glass API) and Xcode 26 or later.

## Try it in the Simulator

```bash
mobile/scripts/run-in-simulator.sh            # with the newest backup of the Mac app
mobile/scripts/run-in-simulator.sh --sample   # with sample notes: every kind of block
```

Or open `mobile/Notion2.xcodeproj` in Xcode, pick a simulator and press Run, then
`mobile/scripts/sim-load.sh` to put a copy of the notes into the running app.

To see sync at work (the Simulator has no iCloud account, so a folder stands in
for iCloud Drive and a small tool for the Mac):

```bash
mobile/scripts/run-in-simulator.sh --sync
python3 mobile/scripts/make-sync-folder.py rename mobile/build/sync-demo "A page title" "A new title"
```

The app shows the change in about ten seconds. `make-sync-folder.py` can also add
a to-do or an event, put a page in the trash and write a snapshot.

To see sync over Wi-Fi (a small tool stands in for the Mac, and its link is
opened in the app as the camera does with the QR code):

```bash
mobile/scripts/run-in-simulator.sh --wifi
python3 mobile/scripts/make-sync-folder.py rename mobile/build/wifi-demo "A page title" "A new title"
```

In the Simulator tap **Open** (if asked), then **Connect**.

The scripts never open the live `workspace.db`. They copy a backup: a complete,
closed copy that the Mac app makes once a day in
`~/Library/Application Support/Notion 2.0/Backups/` (and on "Back up now").

## Put it on your iPhone

No App Store. The project signs with your own developer team.

Once:

1. Connect the iPhone to the Mac with a cable and unlock it. The script below
   asks the iPhone to trust the Mac the first time: tap **Trust** and type the
   passcode.
2. On the iPhone: **Settings › Privacy & Security › Developer Mode**, turn it on,
   restart. (The switch shows after the iPhone was connected to a Mac with Xcode.)
3. In Xcode: **Settings › Accounts** must show your Apple ID.

Keep the iPhone unlocked until the app opens: Xcode cannot fill a locked iPhone
(the script waits for it).

Then, each time you want to install or update:

```bash
mobile/scripts/install-on-iphone.sh
```

or open `mobile/Notion2.xcodeproj` in Xcode, choose the iPhone at the top of the
window and press Run. With the paid developer account the app runs for one year;
install again to renew it. The notes on the iPhone stay.

The first install makes two things in your Apple developer account: the app id
`com.jackhagan.notion2.mobile` and the iCloud container
`iCloud.com.jackhagan.notion2` (the folder that sync through iCloud Drive uses).
Apple does not let an iCloud container be deleted later.

If Apple will not make the signing profile (for example "PLA Update available":
a new developer agreement waits for you at <https://developer.apple.com/account>),
the app can still be installed without sync through iCloud Drive:

```bash
mobile/scripts/install-on-iphone.sh --no-icloud
```

That build asks for no iCloud capability, so the team's general profile on this
Mac is enough and no iCloud container is made. Sync over Wi-Fi and a copy loaded
by hand work as always. Install again without the option to add iCloud Drive;
the notes stay. (Installed this way on 2026-11-10.)

The first time the app connects to a Mac over Wi-Fi, iOS asks whether Rune
may find devices on your local network. Allow it (it can be changed in
**Settings › Privacy & Security › Local Network**).

## Put your notes on the iPhone

Until the Mac app syncs, load a copy by hand:

1. On the Mac, open the folder `~/Library/Application Support/Notion 2.0/Backups`
   (Finder › Go › Go to Folder).
2. Send the newest `workspace-….db` file to the iPhone with AirDrop and choose
   **Rune**. Or save it to Files and, in the app, use
   **Settings › Load another copy…**.

The app keeps its own copy of that file and replaces it only when the new file
reads as a workspace.

To try the QR code with a real iPhone before the Mac app can show one, let the
stand-in Mac serve a copy of a backup (a fixed copy: it does not follow the Mac
app). macOS may ask whether Python may accept incoming connections: allow it.

```bash
python3 mobile/scripts/make-sync-folder.py init "BACKUP.db" "$TMPDIR/notion2-wifi"
python3 mobile/scripts/wifi-mac.py "$TMPDIR/notion2-wifi" --lan --qr
```

Point the iPhone's camera at the code and tap the banner. This works for an
iPhone app with no notes yet (a synced app takes changes, not a new start).

## Tests

```bash
cd mobile/Core && swift test
```

The rules live in the package `Core` (no UI code): the database reader, inline
text, repeat rules and dates, the shape of a page, tables, covers, the open
to-dos, search, the icons, and sync
(the file format, and the rule for a page changed in two places at once). The
tests use the same cases as the Rust tests of the desktop app; when a rule changes
in `src/inline.rs`, `src/repeat.rs`, `src/model.rs` or `src/table.rs`, change the
matching case in `Core/Tests`. The list of covers in `Cover.swift` is a copy of
`covers::PRESETS` in `src/model.rs`. A change to the sync rules changes `docs/sync-format.md`, `SyncTests`
and `scripts/make-sync-folder.py` together; a change to sync over Wi-Fi changes
`docs/sync-wifi.md`, `WifiTests` and `scripts/wifi-mac.py` together.

`WifiRunTests` starts `scripts/wifi-mac.py` (Python 3 and `openssl`, both part
of macOS with the developer tools) on a free port of this computer and runs the
iPhone's code against it.

## Layout

| Path | What |
| --- | --- |
| `Core/` | Swift package `Notion2Core`: model, `WorkspaceDatabase`, `Inline`, `Repeat`, `When`, `Outline`, `Search`, `Icons`, `SyncFormat`, `SyncEngine`, and for sync over Wi-Fi `WifiLink` (the QR code's link, file names), `WifiClient` (HTTPS to the Mac's own certificate), `WifiMirror` (which files to fetch), `WifiSync` (one run) |
| `Notion2/` | The SwiftUI app: `RootView` (tabs and the glass bars), `Views/` (Home, Pages, Page, Search, Calendar, Settings), `Theme` (colours from the designs), `SyncCoordinator` (follows the iCloud Drive folder and the paired Mac while the app is in front; keeps the Mac's token in the keychain) |
| `Notion2.xcodeproj` | The Xcode project. It picks up every file in `Notion2/` by itself |
| `Support/Info.plist`, `Support/Notion2.entitlements` | Let the app open `.db` files sent to the iPhone, use its folder in iCloud Drive, open `notion2://pair?…` links (the QR code) and reach the Mac on the local network |
| `scripts/gen-icons.py` | Writes `Core/Sources/Notion2Core/IconData.swift` from `src/ui/icons.rs`. Run it after an icon changes on the desktop (last run 2026-11-11, from Rune 0.1.7) |
| `scripts/gen-app-icon.sh` | Makes the app icon from `Support/AppIcon.svg` (Rune's logo, `assets/logo.svg`, as a full square) with `scripts/make-icon.swift` |
| `scripts/make-sample-db.py` | Makes sample notes with every kind of block |
| `scripts/make-sync-folder.py` | Plays the part of the Mac in sync: writes a snapshot and change files. The reference for a writer |
| `scripts/wifi-mac.py`, `scripts/qr.swift` | Plays the part of the Mac in sync over Wi-Fi: serves a sync folder, prints the pairing link, and can draw it as a QR code. The reference for the Mac's listener |
| `scripts/run-in-simulator.sh`, `scripts/sim-load.sh`, `scripts/install-on-iphone.sh` | Run and install |

Three things to know before changing the app code:

- The app target treats code as main-actor code by default. A closure that the
  system may call from another thread (a dynamic `UIColor`, a `Canvas` drawing)
  must be marked `@Sendable` or be in a `nonisolated` function, or the app stops
  with a queue assertion. See `Theme.swift` and `IconView.swift`.
- The tabs are a `TabView` with the system bar hidden, and each first screen
  carries the glass bar (`TabBar`, `SearchBar`). Do not stack the tabs in a
  `ZStack`: hidden tabs then take touches.
- Everything that touches the app's database file (a read, a sync run, a copy
  loaded by hand) goes through `StoreGate`, one at a time and off the main thread.

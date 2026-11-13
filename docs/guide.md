# Rune: the guide

This is the long form: a walk through, every part of the app, the AI, and how
to build it. The short page is the [README](../README.md).

(The app was called Notion 2.0 at first. The program, the notes folder and
the page links keep that name inside: `notion2`.)

A local-first workspace of notes and a calendar for macOS, written in Rust with
[Dioxus](https://dioxuslabs.com) (desktop). Pages, nested sub-pages and a block
editor, saved to a SQLite file as you type. AI features run through an AI tool
already on your Mac (Claude Code or Codex).

Your notes stay on your Mac. There is no account and no server.

## Install

1. Download `Rune.zip` from the
   [latest release](https://github.com/capi-git/Rune/releases/latest).
2. Unzip it and move **Rune** into your Applications folder.
3. Open it. The first time, macOS asks if you want to open an app from the
   internet: press **Open**. Apple has checked this build for harmful
   software.

It needs a Mac with Apple silicon and macOS 14.2 or later.

Rune looks for a newer version one time a day and offers it at the bottom of
the sidebar. It asks the Rune page on GitHub and sends nothing of yours. You
can turn this off in Settings → Data.

**Updates.** Rune downloads a newer version by itself. Click **Update ready**
to restart into it, or just open Rune next time: with "Update automatically"
on, it updates as it opens. Before it installs anything, Rune checks that the
download is complete, signed by the same developer, and checked by Apple. It
keeps your current version until the new one has opened. If the new one does
not open, Rune goes back to the version you had and tells you. Your notes are
not touched by an update.

## A first walk through (five minutes)

1. **Home.** The app opens on Home: today's date, the line of your day, a box
   for a quick note, the pages you were on last, and your to-dos.
2. **Make a page.** Press ⌘N (or the + at the top of the sidebar). Type a
   name, press Return and write. The page saves as you type.
3. **Add blocks.** Type `/` on an empty line for the list: headings, to-dos,
   tables, charts, pictures, drawings, a sub-page. Shortcuts work too: `#` and
   a space makes a heading, `[]` a to-do, `-` a bullet. A column of a table
   can have a kind (date, number, time, pace, checkbox, choice) and a total at
   its foot, and **New row for today** adds a row with today's date. A chart
   (bars, a line, a ring, or days) draws from a table.
4. **Link pages.** Type `@` and the start of a page's name, then Return. The
   graph tile in the sidebar shows how your pages connect.
5. **Two pages side by side.** Hold ⌥ and click a page in the sidebar. Drag
   the line between the two to resize them.
6. **Calendar.** Press the calendar tile. Press **New event** and type it in
   words, for example `Lunch with Sam Friday 1pm`, then Return. Drag an event
   to move it. The **Day** view shows the notes of an event beside the day.
   An event has a field for its meeting link (Teams, Zoom, Meet); **Join**
   beside it opens the call and the notes of the event. The calendar shows
   the events of every space, each with a small mark for its space; the
   control at its top shows this space only.
7. **To-dos with a day, and reminders.** On a to-do, press **Give it a day**.
   It then shows on Home and in the calendar. Drag it onto a time to plan it.
   Set **Remind me at** in the same menu, and Rune sends a notification at
   that time (a click on it opens the page). You can also ask the AI:
   "remind me to call the dentist tomorrow at 9".
8. **Meetings.** When a call starts, Rune makes a notes page for it: in Teams,
   Zoom, Webex, Slack, Discord or FaceTime, or a call in Chrome, Arc, Edge,
   Brave or Firefox (you can turn this off in Settings → Notifications). For
   a meeting in a room, use **File → New Meeting Notes**. Rune also shows a
   small card at the corner of the screen, over the call: the **meeting
   pad**. It asks whether to listen. **Listen** writes down what is said and,
   with an AI connected, writes the notes. Rune never listens by itself; with
   no answer the card becomes a small pill and does nothing. The pad is also
   a notepad: what you type goes under "Notes" of the page. You can drag it
   anywhere, make it small, and open the page from it. When the meeting is
   over, the pad asks which page to keep the notes in, then goes away. (With
   the pad turned off in Settings, the page itself asks these things.) Rune
   learns from your answer: the next meeting
   with the same calendar event name goes there by itself. A meeting with no
   calendar event is known by its day and its start time: the second time you
   get the same page as a one-click proposal, and after that the notes go
   there by themselves.
9. **Spaces.** A space is a separate set of pages (work, home, a project).
   The dots at the bottom of the sidebar are your spaces: click one, or move
   two fingers sideways on the sidebar. Click the space name at the top to
   make a new one.
10. **Find and settings.** ⌘K finds any page. It also runs commands: type
    "dark", "focus" or "lock", and Return runs the command. A name that no
    page has gives "New page": `Work/Plan` makes the page Plan inside the
    page Work. Settings is the button at the bottom left of the sidebar (⌘,).

## Connect your AI (optional)

Rune works with no AI. There are two things you can connect, and each is
optional.

**1. AI inside Rune** (Ask AI with ⌘J or the small round button at the foot of
a page, the `/` menu, meeting notes). Rune has no AI of its own. It uses an AI tool that is on your Mac and signed in with
your own account:

- [Claude Code](https://claude.com/claude-code): install it, then run `claude`
  in Terminal one time to sign in.
- or [Codex](https://github.com/openai/codex): install it, then run
  `codex login` one time.

Then open **Settings → AI** in Rune and choose the tool and the model. The AI
gets only the page you ask about.

**2. Give your notes to your AI** (so Claude, Codex or Cursor can read and
write your pages and your calendar). Open **Settings → AI → Connect AI
tools** and press the button for your tool:

| Tool | What to do with what the button copies |
| --- | --- |
| Claude Code | Paste it in Terminal and press Return, one time. |
| Codex | Add the lines to `~/.codex/config.toml`. |
| Claude app, Cursor and others | Add the JSON to the MCP settings file of the tool. |

After that, ask your AI things like "what is on my calendar tomorrow?" or "add
these action items to my page Website plan". Pages are never deleted for good
this way: what an AI removes goes to the Trash of the space.

## Build it from the code

```bash
scripts/bundle.sh            # builds target/release/Rune.app
open "target/release/Rune.app"
```

Or, while developing: `cargo run`. Tests: `cargo test`.

The app can also test itself: it opens a throwaway workspace, clicks and types
through the interface (menus, editing, undo, tabs, sidebar, spaces, settings),
writes a report and quits. Always point it at a scratch database:

```bash
NOTION2_DB=/tmp/notion2-test.db NOTION2_SELFTEST=/tmp/notion2-report.json cargo run
```

## What's in it

- **Look:** Aven-style window. The top row and sidebar sit on see-through glass
  (blurred desktop) and the page sits on it in one rounded card. Light and dark
  themes (⌘⇧L, the moon/sun button, or Settings). Settings ▸ Appearance ▸
  Accent gives links, checkboxes and switches another colour (blue, grey,
  green, orange, pink or purple). Original icon set and logo
  (`assets/logo.svg`; `scripts/make-icon.sh` rebuilds the app icon).
- **Focus mode:** ⌘. (or View → Focus Mode) shows only the page. The other
  lines go pale, and the line you write stays near the middle of the window.
  The tabs come back while the pointer is at the top. Escape or ⌘. ends it.
- **Locked pages:** the page menu (⋯) → "Lock with password…" keeps the lines
  of a page scrambled on disk, in sync, backups and search; the AI tools never
  read them. The name of the page stays visible. Type the password on the page
  to open it; it locks again when no tab shows it, when you quit, or with
  "Lock now". "Remove password" makes it a plain page again. A lost password
  cannot be got back, and the earlier versions of the page are removed when
  you lock it. The iPhone app shows a locked page as locked (`src/lock.rs`).
- **Sidebar:** resize by dragging its right edge (double-click resets). ⌘\
  unpins it; it then slides out as a floating card when the pointer rests on
  the window's left edge. Click that edge to pin it again.
- **Rich text:** select text for the format bar, or ⌘B bold, ⌘I italic,
  ⌘⇧S strikethrough, ⌘E code, ⌘K link. Typing `**bold**`, `_italic_`,
  `~strike~` or `` `code` `` formats as you go. Pasting a link over selected
  text links it; pasting several lines makes blocks. Text is stored as Markdown
  (`src/inline.rs`).
- **Undo:** ⌘Z / ⌘⇧Z undo and redo any change on the page (typing, splitting,
  moving, deleting, AI inserts), not just the block you're in.
- **Drawings:** type `/drawing` (or `/excalidraw`) for an
  [Excalidraw](https://excalidraw.com) canvas in the page. Click it to start
  drawing (until then the page scrolls past it); the button at its top right
  expands it to fill the page. It saves as you draw, keeps its own undo, works
  offline (the library is built into the app, see `assets/excalidraw/NOTICE.md`)
  and shows up in other windows. Search, AI and Markdown see it as "[Drawing]".
- **Spaces:** the dots at the bottom of the sidebar are the spaces: click one,
  or move two fingers sideways on the sidebar, to go to it. Click the space
  name at the top of the sidebar to make a new one; Trash is in the same menu.
  Each space has its own pages, favorites, Home, search and Trash; rename,
  recolour or delete one in Settings (the button at the bottom left).
- **Glass:** Settings → Appearance sets how see-through the window is, whether
  what's behind it is blurred, and its tint (any colour).
- **Calendar:** the calendar tile (⇧⌘C) shows your events by week or month,
  from every space (each with the mark of its space) or from this space only.
  Double-click an empty spot to add an event, click one for its panel: quiet
  rows for the day, the times, repeat, reminder, colour, space, meeting link
  and notes, each with Rune's own picker; or turn its notes into a page. Home
  lists what's coming up.
- **Tables and charts:** a table column has a kind (text, number, choice,
  several choices, date, time, pace, checkbox, link, email, phone, picture,
  formula, row of another table), a format (money, percent), a unit and a
  total at the foot. Type `$3,500` in a cell and its column becomes US
  Dollar. A click on a choice cell opens its list: pick an option, or type
  a new one and press Return; the dots beside an option change its name
  and colour. A click on a date cell opens a month: pick a day, or type one
  ("12 Dec", "tomorrow") in the box at its top and press Return; ↓ goes to
  the month, where the arrows move the day; **Clear** empties the cell.
  Under the month, **End date** gives it an end (click the first day, then
  the last), **Include time** gives it a time of day, and **Remind** sends a
  Mac notification before it (a day with no time reminds at 9 AM). The cell
  says it in words, as `Tue 3 Nov 2:30 PM → 4 PM ⏰ 15 min before`, and a
  small bell shows a reminder.
  ⌘ click (or the small button at its end) opens a link, writes to an email
  address or calls a phone number. A cell that its kind cannot read (words in
  a number column) is grey, and its tip says why; a click on a formula or a
  pace says what it is worked out from. A table can have a name and an icon, stand beside another table,
  and show its rows as a board, a list, a gallery or a calendar. Its menu
  (the ⋯ at its top right) also filters, sorts, groups and colours its rows,
  and locks its columns; a filter or a sort shows as a small chip above the
  table. Each row is a page: **Open** on a row shows it at the side, with
  the values of the row at the top and room to write below. The handle at
  the left of a row adds a row, moves it, nests it under the row above, or
  takes it out; it shows when the pointer is on the row or beside it at the
  left of the table. Its menu has a box at its top that finds a command as
  you type (**Edit property** changes one value of the row, **Open in** opens
  it at the side, as a full page or in a new tab), and at its foot how long
  the row's page is. To take several out at once, pick them: the box at the
  left of each row picks it (⇧ click picks all the rows up to it, and the box
  in the headings picks all or none), ⌘ click the handle of a row does the
  same, and ⌘ click and ⇧ click pick the headings of columns. ⌘D copies the
  picked rows. A small bar over the table says how many are
  picked; **Delete** there, or the Delete key, takes them out, and one Undo
  brings them all back. New rows can start from a template; in a table with a date
  column, **New row for today** starts a row. A chart block (type `/chart`)
  draws bars, a line, a ring or days from the table above it or from a table
  of another page. With an AI connected, "I ran 6 km in 34 minutes today"
  adds the row.
- **Notifications:** each event has a reminder (new events default to 10
  minutes before; change it in Settings). Reminders for the next two weeks are
  handed to macOS, so they arrive even when the app is closed, and clicking one
  opens the event. You're also told when an AI reply finishes while you're in
  another app. macOS asks for permission the first time.
- **Pages side by side and stacked (as in Aven):** a tab can show any number
  of pages, in rows and columns. Drag a tab, a sidebar page, or one page of a
  tab onto any page: it lands on the nearest edge (left, right, top or bottom),
  and a grey preview shows the half it will take. Drag a page by its bar to
  move it within the tab. Drag the line between two pages to resize them
  (double-click shares the space evenly). Each page has its own back and
  forward; ✕ in its bar (or View → Close Pane) closes it and its neighbours
  take the space. ⌥-click a sidebar page, or "Open in split view", to open it
  to the right of the page you're in. Layouts are kept between launches.
- **Home:** today's date, a jot box that saves a line as a page, recently
  visited and edited pages, your to-dos and upcoming events.
- **Tabs:** clicking a page (sidebar, Home, search, links) opens it in a tab of
  its own, or brings forward the tab already showing it. Drag a tab onto
  another tab's edge to reorder it, or onto its middle to bring its pages into
  that tab. Sidebar pages can be dragged onto the tab bar the same way. A tab
  showing several pages has one part per page: click one to work there, drag
  it along the bar to move the whole tab, or pull it down out of the bar to
  give that page its own tab (let go on the bar to choose where, on another
  tab's middle to join that tab, or on a page to put it there). "Separate
  Pages" in the tab's right-click menu gives every page its own tab. A tab
  that just moved glows briefly.
- **Windows:** ⌘⇧N, or "Move to New Window" on
  a tab. Windows stay in step with each other and with agents using the MCP
  server.

## AI

The app uses an AI tool that's already installed and signed in, so there is
nothing to set up if you use Claude Code or Codex. It picks Claude Code first,
then Codex; choose the tool and model in Settings (⌘,) or from the picker in the
Ask AI panel. Claude Code offers Fable, Opus, Sonnet and Haiku; Codex's list is
read from the models it keeps for your account, so it stays current.

- **Claude Code** runs as `claude -p` with every tool switched off (`--tools ""`),
  `--safe-mode` (no CLAUDE.md, hooks, plugins or MCP servers) and no saved session,
  in an empty temporary folder. Not signed in? Run `claude` in Terminal once.
- **Codex** runs as `codex exec` in a read-only sandbox, in an empty temporary
  folder, with no saved session. Not signed in? Run `codex login` once.

The AI only sees the page you ask about. If a reply pretends to run commands or
read files (a model with no tools sometimes does), those parts are removed and
the panel warns that the reply may be made up.

An app opened from Finder doesn't get your shell's PATH, so the app asks your
login shell for it once and also checks the usual install folders.

Use ⌘J or the round button at the foot of a page (it opens into a bar over the
page), the `/` menu (Ask AI, Continue writing, Summarize, Action items, Improve,
Fix grammar, Translate) or a block's ⋮⋮ menu. Ask AI can change the page, and
it can make sub-pages and link them ("give each workout its own page"): it
shows what it will do first, and one Undo takes it all back.

Ask AI can also do things in the rest of Rune. For a request in your own words
it runs with Rune's own tools (the same ones as the MCP server below, and no
others): it can search and read your notes, make, change, name, move and remove
pages, and add, change and remove calendar events ("make a page named Fitness
and move my run logs into it", "add a run tomorrow at 7"). While it works the
card shows the step it is at. At the end it lists what it did, with Undo: one
press takes all of it back. A page it removes goes to the Trash. With no AI
tool connected, none of the AI shows in the app; Settings ▸ AI is where you
connect one.

You can give the AI a picture with your words: paste a screenshot (⌘V) or drop
a picture file into the AI bar, up to four. It shows small in the bar; a press
on it takes it out again. While the AI writes a long page, the card says what
it writes and how far it is, with a clock.

To check the installed tools for real (sends one tiny prompt to each):
`cargo test live_installed_tools -- --ignored --nocapture`. To see the real
tool use Rune's tools on a throwaway workspace (build the app first):
`cargo test live_agent_tools -- --ignored --nocapture`.

## Use it from Aven, Claude Code or Codex (MCP)

The app doubles as an MCP server: `notion2 --mcp` serves the workspace over
stdio, with no window. Agents can search, list, read, create, append to,
replace, rename/move, trash and restore pages, and list, add, change and
delete calendar events (times are local, like `2026-11-10T14:00`). Content goes
in and out as Markdown, and sub-page links look like
`[Title](rune://page/<id>)`. Pages are never deleted permanently. Build the app first (`scripts/bundle.sh`).

- **Aven:** Settings → MCP → Add server. Name `notion2`, Type *Command on this
  Mac*, Command
  `/Applications/Rune.app/Contents/MacOS/notion2`,
  Arguments `--mcp`. Pick Claude and/or Codex (for Claude, *All my projects*).
- **Claude Code:**
  ```bash
  claude mcp add --scope user rune -- "/Applications/Rune.app/Contents/MacOS/notion2" --mcp
  ```
- **Codex:**
  ```bash
  codex mcp add rune -- "/Applications/Rune.app/Contents/MacOS/notion2" --mcp
  ```
- **Claude Desktop** (`~/Library/Application Support/Claude/claude_desktop_config.json`):
  ```json
  { "mcpServers": { "rune": {
      "command": "/Applications/Rune.app/Contents/MacOS/notion2",
      "args": ["--mcp"] } } }
  ```

Set `NOTION2_DB=/some/test.db` to point the app or the server at a different
database file.

## Data

`~/Library/Application Support/Notion 2.0/workspace.db` (override with `NOTION2_DB`)

## Layout

| Path | What it holds |
| --- | --- |
| `src/model.rs` | Pages, blocks and the editing rules (Enter, Backspace, nesting, moving) |
| `src/markdown.rs` | Typing shortcuts, Markdown → blocks for AI replies, page → Markdown |
| `src/lock.rs` | Locked pages: the sealed block, the keys of open pages, lock and open |
| `src/store.rs` | SQLite storage |
| `src/ai.rs` | Picks the AI and model, prompts, cleans replies |
| `src/glass.rs` | The blur behind the window |
| `src/ai/local.rs` | Finds and runs installed AI tools (Claude Code, Codex) |
| `src/mcp.rs` | MCP server (`notion2 --mcp`) so agents can use the workspace |
| `src/when.rs` | Dates and times for the calendar (local time) |
| `src/ui/calendar.rs` | Calendar week/month views and the event editor |
| `src/inline.rs` | Inline formatting: Markdown ↔ formatted runs ↔ editor HTML |
| `src/history.rs` | Undo and redo |
| `src/sync.rs` | Keeps windows (and outside edits) in step |
| `src/state.rs` | App state, tabs/history, every user action |
| `src/ui/` | Top row, sidebar, page editor, menus, modals, icons |
| `assets/bridge.js` | The rich-text editor (keys, formatting, paste, links), sidebar resize/peek, drag and drop |
| `assets/style.css` | Styling, light and dark themes |

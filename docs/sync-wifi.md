# Sync straight between the Mac and the iPhone (Wi-Fi, version 1)

A second way for the sync files of `docs/sync-format.md` to travel: directly
from the Mac to the iPhone over the local network, with no cloud between.
Decided on 2026-11-10 (the user wants both this and iCloud Drive; see
`docs/mobile-plan.md`, decision 2). Both apps must follow this file exactly.

State on 2026-11-11: the iPhone side is built and tested
(`mobile/Core/Sources/Notion2Core/Wifi*.swift`, `mobile/Notion2/SyncCoordinator.swift`).
The Mac's side is `src/devsync/wifi.rs`, on from Settings ▸ Sync ▸ Over
Wi-Fi. `mobile/scripts/wifi-mac.py` plays the Mac's part in tests. On 2026-11-10 it also worked on a real
iPhone (iOS 27) against `wifi-mac.py --lan --qr`: the camera read the QR code
and opened the app, the app paired, and the first notes came.

## The idea

- Nothing about the notes changes: the same snapshots, change files, device
  files and pictures, the same versions and the same rules to apply a change.
  Only the way a file gets from one device to the other is new.
- The Mac keeps its sync files in a folder of its own and serves that folder.
  The iPhone asks "which files do you have?", fetches the ones it still needs
  into its own copy of the folder, and applies them as it applies the folder in
  iCloud Drive.
- The two are paired once: the Mac shows a QR code, the iPhone's camera reads
  it. After that no code is needed.
- It works only while both are on the same network and Notion 2.0 is open on
  the Mac and in front on the iPhone. Away from the Mac the iPhone has its last
  state; with iCloud Drive also on, it goes on through that.
- Both ways can be on at once. A device may get the same file by both: a file
  with a number at or below the last one applied is passed over, and a change
  whose version is already here does nothing (rule 2 of "Applying a change").

## The Mac's own sync folder

Sync over Wi-Fi must work when the folder in iCloud Drive does not exist. So
the Mac writes its files in a folder of its own, always:

    ~/Library/Application Support/Notion 2.0/Sync/sync-v1/

with the layout of `docs/sync-format.md` (`devices/`, `snapshots/`, `changes/`,
`files/`). A live database is never in it.

- This folder is what the Mac serves over Wi-Fi, and where the files that an
  iPhone sends are put.
- When the folder in iCloud Drive exists, the Mac also copies each new file of
  its own into it (written elsewhere first, then moved in, as always), and
  reads the other devices' files from both folders.
- When the Mac removes old files of its own (see "Snapshots"), it removes them
  in both places.

## The Mac's certificate

The connection is HTTPS (TLS 1.2 or later). There is no certificate authority:

- The Mac makes one certificate, once: self-signed, ECDSA P-256, signed with
  SHA-256. It keeps the private key in its keychain (or in a file only the
  user can read) and never sends it anywhere.
- The fingerprint of the certificate is the SHA-256 of its DER form, as 64
  lower-case hex characters. The QR code carries it.
- The iPhone accepts exactly the certificate with that fingerprint and no
  other, whatever name or dates are in it. So nobody on the network can read
  the notes or stand in for the Mac.
- A new certificate means every iPhone must pair again. Do not make a new one
  when the app is updated.

## Pairing

The QR code holds one link:

    notion2://pair?v=1&host=Jacks-MacBook-Pro.local&host=192.168.1.20&port=47815&cert=<64 hex>&code=<code>&name=MacBook%20Pro

| Part | Meaning |
| --- | --- |
| `v` | The version of this file: `1`. A greater number: the iPhone says "update the app". |
| `host` | Where the Mac listens. One to four times: first the Mac's local network name (`scutil --get LocalHostName` + `.local`), then its address on the network now, for a network where the name is not found. Letters, digits, dots and hyphens only (a name or an IPv4 address). |
| `port` | The port, 1 to 65535. The Mac picks one once (47815 when it is free) and keeps it, so that a paired iPhone finds it again. |
| `cert` | The fingerprint of the Mac's certificate. |
| `code` | A code for this one pairing: at least 16 characters of `A-Z a-z 0-9 - _`, made from at least 16 random bytes. It works once and for at most 10 minutes. |
| `name` | The Mac's name, for the question "Connect to MacBook Pro?". |

- The iPhone's camera reads the code and opens the iPhone app with the link
  (the app registers the scheme `notion2`). The app asks "Connect to …?".
- A new code is made each time the Mac shows the QR code. When it is used, or
  the window that shows it is closed, or 10 minutes passed, it is dead.
- The iPhone first finds the address at which the Mac answers (any answer
  counts, also 401), then sends:

      POST /v1/pair
      {"code": "<code>", "device": "<the iPhone's device id>", "name": "Jack's iPhone", "kind": "iphone"}

  A good code: `200` with

      {"token": "<token>", "device": "<the Mac's device id>", "name": "MacBook Pro"}

  A wrong, used or old code, or a `device` that is not 32 lower-case hex
  characters: `403`, and nothing else is said.
- The token is 32 random bytes as 43 characters of `A-Z a-z 0-9 - _`. The Mac
  keeps only its SHA-256, with the iPhone's device id and name. The iPhone
  keeps the token in its keychain, on that device only.
- An iPhone that pairs again (same device id) replaces its earlier entry.
- The iPhone remembers the Mac: device id, name, hosts, port, fingerprint. When
  the Mac's local name changes and its address too, the iPhone cannot find it:
  pair again.

## Requests

Every request except `POST /v1/pair` carries `Authorization: Bearer <token>`.
A token the Mac does not know: `401` (the iPhone then says "this Mac no longer
knows this iPhone"). HTTP/1.1 with keep-alive; every answer has a
`Content-Length`.

### `GET /v1/files`

    {
      "format": 1,
      "device": "<the Mac's device id>",
      "name": "MacBook Pro",
      "devices": [ {a device file, whole}, … ],
      "files": [ {"path": "snapshots/<device>/0000000012.db", "size": 5234688},
                 {"path": "changes/<device>/0000000013.json", "size": 2110},
                 {"path": "files/5a3b6b0a1e9b3281740d.png", "size": 614545} ]
    }

- `format` is the format of the sync files (`docs/sync-format.md`). Greater
  than 1: the iPhone stops and says "update the app".
- `devices` holds every device file of the Mac's folder as it is (they are
  small and change often, so they are not fetched one by one).
- `files` lists every snapshot, change file and picture in the Mac's folder,
  of every device, with its length in bytes.
- The iPhone asks this about every ten seconds while it is in front. Keep it
  cheap.

### `GET /v1/file/<path>`

The bytes of one file of the list. `404` when there is no such file or the
name is not one of the four kinds below.

### `PUT /v1/file/<path>`

The iPhone sends one of its own files. The Mac puts it in its folder (written
outside the folder first, then moved in).

- Allowed: `devices/<the asking iPhone>.json`, `changes/<the asking iPhone>/…`,
  `snapshots/<the asking iPhone>/…`, and `files/<name>`. Anything else, above
  all a file of another device: `403`.
- A device file replaces the one before. Any other file is written once: the
  same name again with the same length is fine (`204`, nothing written), with
  another length it is `409`.
- Good: `204`. Larger than the Mac takes (256 MB in the stand-in): `413`.

Today the iPhone only sends its device file (it cannot edit yet), which tells
the Mac how far the iPhone is. The rest is for milestone 3.

### `DELETE /v1/pair`

The iPhone was told to disconnect: the Mac forgets its token. `204`.

### File names

Only these names are ever taken, on both sides. A name is checked before it
touches the file system, so it can never point outside the folder:

| Kind | Name |
| --- | --- |
| Device file | `devices/<device>.json` |
| Snapshot | `snapshots/<device>/<seq>.db` |
| Change file | `changes/<device>/<seq>.json` |
| Picture | `files/<name>` with `<name>` as `src/files.rs` makes it: 1 to 40 letters and digits, a dot, and one of `png jpg gif webp heic tiff bmp` |

`<device>` is 32 lower-case hex characters, `<seq>` ten digits, not all zero.

## What the iPhone does in one run

About every ten seconds while the app is in front (twice a minute while the
Mac does not answer):

1. `GET /v1/files`.
2. Decide what to fetch (`WifiMirror.plan`):
   - With no synced notes yet: the newest snapshot of each other device, every
     change file, every picture.
   - Otherwise: each device's change files after the last one applied, and
     every picture it does not have. A snapshot only when a file between the
     last one applied and that snapshot is gone.
   - A snapshot that is listed and not fetched is marked "listed, not here" in
     the copy, so that the engine sees its number (a snapshot uses a number of
     the device's counter; it is not a gap).
3. Fetch (snapshots, then change files, then pictures), each into a place
   beside the copy and then moved in. A file whose length is not the listed
   one is thrown away.
4. Apply with the engine of `docs/sync-format.md`. A snapshot that was just
   taken says how far the other devices are, so steps 2 to 4 run again when
   there is more to fetch.
5. `PUT` its own device file when it is not the one in the Mac's list.

Applied change files and snapshots leave the copy at the next run: the copy
does not keep a second set of the notes. Pictures stay (also after
"Disconnect"; the notes still name them).

## What the Mac app must build

1. The own sync folder above, and the writer of `docs/sync-format.md` that
   fills it (the same work that sync through iCloud Drive needs).
2. The certificate, made once.
3. A listener for the requests above, off the UI thread, on IPv4 and IPv6. It
   is new attack surface, so: start it only when sync over Wi-Fi is on in
   Settings; answer nothing without a known token except `POST /v1/pair`;
   compare codes and tokens in constant time; check every file name as above.
4. In Settings, under Sync: a switch "Sync over Wi-Fi", a button "Connect an
   iPhone…" that shows the QR code (and a new code each time), and the list of
   paired iPhones with a way to remove one. The iPhone app's texts say "show
   the code for an iPhone in Notion 2.0 on your Mac": keep that findable.
5. macOS may ask "Do you want Notion 2.0 to accept incoming network
   connections?" the first time. The app is signed, so the default firewall
   setting lets it through.
6. The listener must not keep the Mac awake.

`mobile/scripts/wifi-mac.py` does 2, 3 and the code of 4 in about 250 lines of
Python; a Rust version needs a TLS library (`rustls` with a certificate from
`rcgen`, for example) and a small HTTP server.

## Not in version 1

- The Mac is not found by itself (Bonjour). The iPhone uses the name and the
  address from the QR code.
- Nothing is sent while the iPhone app is not in front: iOS does not let it.
- An iPhone is paired with one Mac. Two Macs sync with each other through
  iCloud Drive.
- An iPhone that is paired with a Mac it has no notes from (it never applied
  a file of that Mac) starts again from that Mac's newest snapshot: the Mac's
  notes take the place of the ones on the iPhone. This is right while the
  iPhone cannot edit. When it can, its own changes must be kept (take the
  snapshot record by record, as `docs/sync-format.md` says for a gap).
- A device does not pass on files that another device wrote (the Mac does not
  hand an iPhone's files to a second iPhone, except through its list).

## How to try it

- `cd mobile/Core && swift test`: the suite `WifiRunTests` starts
  `wifi-mac.py` on a free port and runs the iPhone's code against it (pairing,
  the first notes, changes, a picture, a used code, a wrong certificate, a
  token the Mac forgot, a file of another device).
- `mobile/scripts/run-in-simulator.sh --wifi`: the same in the iOS Simulator,
  with the link opened in the app as the camera does.
- With a real iPhone, before the Mac app can do it:
  `mobile/scripts/wifi-mac.py DIR --lan --qr` serves the folder that
  `make-sync-folder.py init BACKUP.db DIR` made and shows the QR code.

#!/usr/bin/env bash
# Build the iPhone app, start it in the iOS Simulator and load a copy of your notes.
#
#   mobile/scripts/run-in-simulator.sh            with the newest backup of the Mac app
#   mobile/scripts/run-in-simulator.sh FILE.db    with that file
#   mobile/scripts/run-in-simulator.sh --sample   with sample notes (every kind of block)
#   mobile/scripts/run-in-simulator.sh --sync     with sync: a folder stands in for iCloud Drive
#                                                 and make-sync-folder.py for the Mac
#   mobile/scripts/run-in-simulator.sh --wifi     with sync over the local network: wifi-mac.py
#                                                 stands in for the Mac, and its link is opened
#                                                 in the app as the camera does with the QR code
#
# SIM_NAME picks the simulator (default "iPhone 18 Pro Max").
# NO_WINDOW=1 does not bring the Simulator window to the front.
set -euo pipefail
cd "$(dirname "$0")/.."
SIM_NAME="${SIM_NAME:-iPhone 18 Pro Max}"
BUNDLE_ID="com.jackhagan.notion2.mobile"
SOURCE="${1:-}"

UDID="$(xcrun simctl list devices available | grep -F "    $SIM_NAME (" | head -1 | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/')"
[ -n "$UDID" ] || { echo "No simulator named \"$SIM_NAME\". See: xcrun simctl list devices available" >&2; exit 1; }
xcrun simctl boot "$UDID" >/dev/null 2>&1 || true
xcrun simctl bootstatus "$UDID" >/dev/null

echo "Building…"
xcodebuild -project Notion2.xcodeproj -scheme Notion2 -configuration Debug \
  -destination "platform=iOS Simulator,id=$UDID" -derivedDataPath build build \
  | grep -E "error:|BUILD" || true
APP="build/Build/Products/Debug-iphonesimulator/Notion2.app"
[ -d "$APP" ] || { echo "The build did not make the app." >&2; exit 1; }
xcrun simctl install "$UDID" "$APP"
xcrun simctl launch "$UDID" "$BUNDLE_ID" >/dev/null

if [ "$SOURCE" = "--wifi" ]; then
  # The stand-in Mac serves a sync folder on this computer; the Simulator
  # reaches it at "localhost". The app starts with no notes of its own.
  BACKUP="$(ls -t "$HOME/Library/Application Support/Notion 2.0/Backups"/workspace-*.db 2>/dev/null | head -1 || true)"
  if [ -z "$BACKUP" ]; then
    BACKUP="$(mktemp -d)/sample-backup.db"
    python3 scripts/make-sample-db.py "$BACKUP" >/dev/null
  fi
  FOLDER="$PWD/build/wifi-demo"
  rm -rf "$FOLDER" "$FOLDER.mac"
  python3 scripts/make-sync-folder.py init "$BACKUP" "$FOLDER" "$(scutil --get ComputerName 2>/dev/null || echo Mac)"
  xcrun simctl terminate "$UDID" "$BUNDLE_ID" >/dev/null 2>&1 || true
  DATA="$(xcrun simctl get_app_container "$UDID" "$BUNDLE_ID" data)"
  rm -rf "$DATA/Library/Application Support/Workspace"
  xcrun simctl launch "$UDID" "$BUNDLE_ID" >/dev/null
  [ -n "${NO_WINDOW:-}" ] || open -a Simulator
  LOG="$FOLDER.mac/wifi-mac.log"
  python3 -u scripts/wifi-mac.py "$FOLDER" >"$LOG" 2>&1 &
  SERVER=$!
  trap 'kill "$SERVER" 2>/dev/null || true' EXIT
  for _ in 1 2 3 4 5 6 7 8 9 10; do grep -q '^LINK ' "$LOG" 2>/dev/null && break; sleep 0.5; done
  LINK="$(grep -m1 '^LINK ' "$LOG" | cut -c6-)"
  [ -n "$LINK" ] || { echo "The stand-in Mac did not start. See $LOG" >&2; exit 1; }
  xcrun simctl openurl "$UDID" "$LINK"
  echo "In the Simulator: tap Open (if asked), then Connect. The notes then come from the stand-in Mac."
  echo "Change a page as the Mac would, and watch the app (about 10 seconds):"
  echo "  python3 mobile/scripts/make-sync-folder.py rename \"$FOLDER\" \"A page title\" \"A new title\""
  echo "The stand-in Mac runs until you press Ctrl-C."
  wait "$SERVER"
  exit 0
fi
if [ "$SOURCE" = "--sync" ]; then
  # The Simulator has no iCloud account. A plain folder takes the place of
  # the iCloud Drive folder, and the app starts with no notes of its own.
  BACKUP="$(ls -t "$HOME/Library/Application Support/Notion 2.0/Backups"/workspace-*.db 2>/dev/null | head -1 || true)"
  if [ -z "$BACKUP" ]; then
    BACKUP="$(mktemp -d)/sample-backup.db"
    python3 scripts/make-sample-db.py "$BACKUP" >/dev/null
  fi
  FOLDER="$PWD/build/sync-demo"
  rm -rf "$FOLDER" "$FOLDER.mac"
  python3 scripts/make-sync-folder.py init "$BACKUP" "$FOLDER" "$(scutil --get ComputerName 2>/dev/null || echo Mac)"
  xcrun simctl terminate "$UDID" "$BUNDLE_ID" >/dev/null 2>&1 || true
  DATA="$(xcrun simctl get_app_container "$UDID" "$BUNDLE_ID" data)"
  rm -rf "$DATA/Library/Application Support/Workspace"
  xcrun simctl launch "$UDID" "$BUNDLE_ID" -syncFolder "$FOLDER" >/dev/null
  echo "The app follows $FOLDER. Change a page as the Mac would, and watch the app (about 10 seconds):"
  echo "  python3 mobile/scripts/make-sync-folder.py rename \"$FOLDER\" \"A page title\" \"A new title\""
  [ -n "${NO_WINDOW:-}" ] || open -a Simulator
  exit 0
fi
if [ "$SOURCE" = "--sample" ]; then
  SOURCE="$(mktemp -d)/sample.db"
  python3 scripts/make-sample-db.py "$SOURCE" >/dev/null
fi
if [ -n "$SOURCE" ]; then
  SIM="$UDID" scripts/sim-load.sh "$SOURCE"
else
  SIM="$UDID" scripts/sim-load.sh || echo "The app is running without notes. Load a copy in the app, or run this with --sample."
fi
[ -n "${NO_WINDOW:-}" ] || open -a Simulator

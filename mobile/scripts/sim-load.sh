#!/usr/bin/env bash
# Put a copy of a workspace file into the app in the booted iPhone simulator.
#
#   mobile/scripts/sim-load.sh            the newest backup made by the Mac app
#   mobile/scripts/sim-load.sh FILE.db    that file
#
# SIM=<simulator name or id> picks a simulator when more than one is booted.
#
# The live workspace.db is never opened or copied: backups are complete,
# closed copies that the Mac app makes in "Backups" beside it.
set -euo pipefail
BUNDLE_ID="com.jackhagan.notion2.mobile"
SIM="${SIM:-booted}"
SOURCE="${1:-}"
if [ -z "$SOURCE" ]; then
  # (The Mac app's folder. It was "Notion 2.0" at first: the Mac app moves it
  # at its first start, and a link of that name still leads here.)
  BACKUPS="$HOME/Library/Application Support/Rune/Backups"
  SOURCE="$(ls -t "$BACKUPS"/workspace-*.db 2>/dev/null | head -1 || true)"
  [ -n "$SOURCE" ] || { echo "No backup in $BACKUPS. In the Mac app: Settings, Back up now." >&2; exit 1; }
fi
[ -f "$SOURCE" ] || { echo "No file at $SOURCE" >&2; exit 1; }
case "$(basename "$SOURCE")" in
  workspace.db) echo "That is the name of the live file. Use a backup copy." >&2; exit 1 ;;
esac

DATA="$(xcrun simctl get_app_container "$SIM" "$BUNDLE_ID" data 2>/dev/null)" \
  || { echo "The app is not in a booted simulator. Build and run it first." >&2; exit 1; }
xcrun simctl terminate "$SIM" "$BUNDLE_ID" >/dev/null 2>&1 || true
mkdir -p "$DATA/Library/Application Support/Workspace"
cp "$SOURCE" "$DATA/Library/Application Support/Workspace/workspace.db"
# Pictures are files beside the database, not rows in it: "Files" beside the
# Mac's workspace.db (one folder up from its backups), or "<file>.files".
rm -rf "$DATA/Library/Application Support/Workspace/Files"
for PICTURES in "$SOURCE.files" "$(dirname "$SOURCE")/../Files"; do
  if [ -d "$PICTURES" ]; then
    cp -R "$PICTURES" "$DATA/Library/Application Support/Workspace/Files"
    break
  fi
done
xcrun simctl launch "$SIM" "$BUNDLE_ID" >/dev/null
echo "Loaded $(basename "$SOURCE") into the simulator."

#!/usr/bin/env bash
# Build the app and put it on your iPhone, without the App Store.
#
#   mobile/scripts/install-on-iphone.sh                the iPhone that is connected
#   mobile/scripts/install-on-iphone.sh "Jack's iPhone" that one, by name
#   mobile/scripts/install-on-iphone.sh --no-icloud    without sync through iCloud Drive
#
# --no-icloud leaves the iCloud capability out of this one build. The team's
# general signing profile is then enough, so nothing is asked of the Apple
# developer account (for when it cannot be reached, or an agreement there
# waits for you). Sync over Wi-Fi and a copy loaded by hand work as always.
# Install again without the option to add iCloud Drive; the notes stay.
#
# Before the first time:
#   1. Connect the iPhone to this Mac with a cable and unlock it. This script
#      asks the iPhone to trust this Mac: tap Trust on the iPhone.
#   2. On the iPhone: Settings > Privacy & Security > Developer Mode > on, then restart.
#   3. Xcode must be signed in with your Apple ID (Xcode > Settings > Accounts).
# Xcode makes the signing profile itself (-allowProvisioningUpdates). With the
# paid developer account the app then runs for one year; run this again to
# renew it or to install a newer build. The notes on the iPhone stay.
set -euo pipefail
cd "$(dirname "$0")/.."
BUNDLE_ID="com.jackhagan.notion2.mobile"
NO_ICLOUD=""
if [ "${1:-}" = "--no-icloud" ]; then
  NO_ICLOUD=1
  shift
fi
WANTED="${1:-}"

LIST="$(mktemp)"
trap 'rm -f "$LIST"' EXIT

# The connected iPhones (not simulators): "udid<TAB>name<TAB>paired or not<TAB>developer mode" per line.
find_iphones() {
  xcrun devicectl list devices --json-output "$LIST" >/dev/null 2>&1 || true
  python3 - "$LIST" "$WANTED" <<'PY'
import json, sys
try:
    devices = json.load(open(sys.argv[1])).get("result", {}).get("devices", [])
except Exception:
    devices = []
wanted = sys.argv[2]
for d in devices:
    hardware = d.get("hardwareProperties", {})
    connection = d.get("connectionProperties", {})
    properties = d.get("deviceProperties", {})
    name = properties.get("name", "")
    # An iPhone that is not paired yet does not say that it is a real one,
    # so everything that is not a simulator counts.
    if hardware.get("reality") == "simulated" or hardware.get("platform") != "iOS":
        continue
    if wanted and wanted not in (name, hardware.get("udid"), d.get("identifier")):
        continue
    if not wanted and connection.get("tunnelState") == "unavailable":
        continue
    print("\t".join([hardware.get("udid") or d.get("identifier", ""), name, connection.get("pairingState") or "", properties.get("developerModeStatus") or ""]))
PY
}

FOUND="$(find_iphones)"
COUNT="$(printf '%s' "$FOUND" | grep -c . || true)"
if [ "$COUNT" -eq 0 ]; then
  echo "No iPhone found. Connect it to this Mac with a cable and unlock it." >&2
  exit 1
fi
if [ "$COUNT" -gt 1 ]; then
  echo "More than one iPhone found. Give the name of one:" >&2
  printf '%s\n' "$FOUND" | cut -f2 >&2
  exit 1
fi
UDID="$(printf '%s' "$FOUND" | cut -f1)"
NAME="$(printf '%s' "$FOUND" | cut -f2)"

# The first time: this Mac and the iPhone must trust each other.
if [ "$(printf '%s' "$FOUND" | cut -f3)" = "unpaired" ]; then
  echo "$NAME is connected, but it does not trust this Mac yet."
  echo "Unlock the iPhone now. Tap Trust when it asks, and type the passcode of the iPhone."
  if ! xcrun devicectl manage pair --device "$UDID" --timeout 120 >/dev/null 2>&1; then
    echo "The iPhone did not answer. Unlock it, connect the cable again, tap Trust, and run this again." >&2
    exit 1
  fi
  FOUND="$(find_iphones)"
fi
if [ "$(printf '%s' "$FOUND" | cut -f4)" = "disabled" ]; then
  echo "Developer Mode is off on $NAME. On the iPhone: Settings > Privacy & Security > Developer Mode > on." >&2
  echo "The iPhone restarts. Unlock it, tap Turn On, and run this again." >&2
  exit 1
fi

# Xcode can only prepare and fill an iPhone that is unlocked.
is_locked() {
  xcrun devicectl device info lockState --device "$UDID" --json-output "$LIST" >/dev/null 2>&1 || return 1
  python3 -c 'import json, sys; sys.exit(0 if json.load(open(sys.argv[1])).get("result", {}).get("passcodeRequired") else 1)' "$LIST" 2>/dev/null
}
if is_locked; then
  echo "$NAME is locked. Unlock it now, and keep it unlocked until the app opens."
  for _ in $(seq 1 90); do
    is_locked || break
    sleep 2
  done
  if is_locked; then
    echo "$NAME is still locked. Unlock it and run this again." >&2
    exit 1
  fi
fi

if [ -n "$NO_ICLOUD" ]; then
  # An entitlements file with nothing in it (an empty setting is not enough:
  # Xcode then still asks for iCloud), and no iCloud folder in the app's
  # settings: the app asks for nothing that needs a signing profile of its own.
  mkdir -p build
  printf '%s\n' '<?xml version="1.0" encoding="UTF-8"?>' \
    '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' \
    '<plist version="1.0"><dict/></plist>' > build/none.entitlements
  cp Support/Info.plist build/Info-no-icloud.plist
  /usr/libexec/PlistBuddy -c "Delete :NSUbiquitousContainers" build/Info-no-icloud.plist
  SIGNING=(CODE_SIGN_ENTITLEMENTS=build/none.entitlements INFOPLIST_FILE=build/Info-no-icloud.plist)
else
  SIGNING=(-allowProvisioningUpdates)
fi
APP="build/Build/Products/Release-iphoneos/Notion2.app"
rm -rf "$APP"
echo "Building for $NAME…"
xcodebuild -project Notion2.xcodeproj -scheme Notion2 -configuration Release \
  -destination "id=$UDID" -derivedDataPath build "${SIGNING[@]}" build >"$LIST" 2>&1 || true
grep -E "error:|warning: .*provision|BUILD" "$LIST" || true
if [ ! -d "$APP" ]; then
  if grep -q "PLA Update available" "$LIST"; then
    echo "Apple asks you to agree to the newest developer agreement before it makes a signing profile." >&2
    echo "Sign in at https://developer.apple.com/account and accept it, then run this again." >&2
    echo "Or run this with --no-icloud: the app is then installed now, without sync through iCloud Drive." >&2
  else
    echo "The build did not make the app. Keep the iPhone unlocked and run this again, or open mobile/Notion2.xcodeproj in Xcode to see why." >&2
  fi
  exit 1
fi

echo "Installing on $NAME…"
xcrun devicectl device install app --device "$UDID" "$APP" >/dev/null
xcrun devicectl device process launch --device "$UDID" "$BUNDLE_ID" >/dev/null 2>&1 || true
echo "Rune is on $NAME."

#!/usr/bin/env bash
# Build "Rune.app" in target/<profile>/ so the app can be opened from Finder.
# Usage: scripts/bundle.sh [debug|release]   (default: release)
set -euo pipefail
cd "$(dirname "$0")/.."
PROFILE="${1:-release}"
# Homebrew's rustup keeps cargo outside the default PATH; find it if needed.
if ! command -v cargo >/dev/null 2>&1; then
  for dir in "$HOME/.cargo/bin" /opt/homebrew/opt/rustup/bin /usr/local/opt/rustup/bin; do
    if [ -x "$dir/cargo" ]; then PATH="$dir:$PATH"; break; fi
  done
  if ! command -v cargo >/dev/null 2>&1 && command -v rustup >/dev/null 2>&1; then
    PATH="$(dirname "$(rustup which cargo)"):$PATH"
  fi
  command -v cargo >/dev/null 2>&1 || { echo "cargo not found. Install Rust: brew install rustup && rustup default stable" >&2; exit 1; }
fi
# Every build made here has a build number (when it was built), compiled into
# the program and written as the app's version, so an installed copy can tell
# that this one is newer and update itself (see src/update.rs).
export RUNE_BUILD_STAMP="$(date +%Y%m%d%H%M%S)"
export RUNE_BUILD_COMMIT="$(git rev-parse --short HEAD 2>/dev/null || true)"
if [ "$PROFILE" = "release" ]; then cargo build --release; else cargo build; fi
APP="target/$PROFILE/Rune.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/$PROFILE/rune" "$APP/Contents/MacOS/Rune"
# The program was named notion2 before the app was Rune. A link of that name
# stays beside it: a Rune built before the rename takes an update only when
# it finds Contents/MacOS/notion2 (src/update.rs), and the AI tools people
# connected start the connector as ".../Contents/MacOS/notion2 --mcp".
ln -s Rune "$APP/Contents/MacOS/notion2"
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
# The listener (helper/rune-listen.swift): hears the microphone and a meeting's
# sound and turns speech into text. Without the Swift compiler the app is
# built without it, and says so when asked to listen.
if xcrun --find swiftc >/dev/null 2>&1; then
  # Built for macOS 14.2 and later (what it uses to hear the Mac's sound came
  # with 14.2), not only for the macOS of the Mac that builds it.
  xcrun swiftc -swift-version 5 -O -suppress-warnings -target arm64-apple-macos14.2 -o "$APP/Contents/MacOS/rune-listen" helper/rune-listen.swift
  # The calendar reader (helper/rune-cal.swift): reads the events of the Mac's
  # calendars, only when the person turns that on in Settings.
  xcrun swiftc -swift-version 5 -O -suppress-warnings -target arm64-apple-macos13.0 -o "$APP/Contents/MacOS/rune-cal" helper/rune-cal.swift
else
  echo "No Swift compiler: built without the meeting listener and the calendar reader." >&2
fi
# (The bundle id and the helpers' signing identifiers keep the app's first
# name, notion2: macOS keeps what the person allowed, the microphone, the
# calendars and the notifications, by them, and a Rune that people have
# takes an update only with the same bundle id.)
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Rune</string>
  <key>CFBundleDisplayName</key><string>Rune</string>
  <key>CFBundleIdentifier</key><string>com.jackhagan.notion2</string>
  <key>CFBundleExecutable</key><string>Rune</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>${RUNE_BUILD_STAMP}</string>
  <key>CFBundleShortVersionString</key><string>0.1.21</string>
  <key>LSMinimumSystemVersion</key><string>14.2</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Rune listens to your microphone only when you ask it to: to write what you say, or the notes of a meeting. The sound is not kept.</string>
  <key>NSCalendarsFullAccessUsageDescription</key><string>Rune shows the events of your calendars beside its own, when you turn that on in Settings. It only reads them, and they stay on this Mac.</string>
  <key>NSCalendarsUsageDescription</key><string>Rune shows the events of your calendars beside its own, when you turn that on in Settings. It only reads them, and they stay on this Mac.</string>
  <key>NSSpeechRecognitionUsageDescription</key><string>Rune turns what it hears into text when you ask it to listen. Apple's speech recognition does this on this Mac when it can for your language.</string>
  <key>NSAudioCaptureUsageDescription</key><string>Rune hears the other people in a meeting through the sound this Mac plays, only while you have asked it to listen. The sound is not kept.</string>
</dict>
</plist>
PLIST
# Sign it with your Developer ID if this Mac has one (RUNE_SIGN_IDENTITY picks
# another, or "-" for none): macOS then knows each new build as the same
# app, so what you allowed (microphone, speech) stays allowed after updates.
IDENTITY="${RUNE_SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | awk -F'"' '/Developer ID Application/ {print $2; exit}')}"
if [ -n "$IDENTITY" ] && [ "$IDENTITY" != "-" ]; then
  [ -f "$APP/Contents/MacOS/rune-listen" ] && codesign --force -s "$IDENTITY" --identifier com.jackhagan.notion2.listen "$APP/Contents/MacOS/rune-listen" 2>/dev/null
  [ -f "$APP/Contents/MacOS/rune-cal" ] && codesign --force -s "$IDENTITY" --identifier com.jackhagan.notion2.cal "$APP/Contents/MacOS/rune-cal" 2>/dev/null
  if codesign --force -s "$IDENTITY" "$APP" 2>/dev/null; then
    echo "Signed as: $IDENTITY"
  else
    echo "Couldn't sign with \"$IDENTITY\"; the app is left as built." >&2
  fi
fi
echo "Built $APP (build $RUNE_BUILD_STAMP)"

#!/bin/bash
# Get Apple to check a release build (notarization), so that it opens on
# another Mac with no warning.
#
#   scripts/bundle.sh release     # first: builds target/release/Rune.app
#   scripts/notarize.sh sign      # a copy, signed for the check
#   scripts/notarize.sh submit    # sends that copy to Apple, attaches the answer, zips it
#
# One time before "submit", store your Apple sign-in for the check tool:
#   xcrun notarytool store-credentials rune-notary --team-id <your team id>
# (N2_NOTARY_PROFILE names another profile.)
#
# The app in target/release stays as built: your own copy updates from it.
set -euo pipefail
cd "$(dirname "$0")/.."

APP="target/release/Rune.app"
[ -d "$APP" ] || { echo "Build first: scripts/bundle.sh release" >&2; exit 1; }
STAMP="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$APP/Contents/Info.plist")"
OUT="target/release/notarized/$STAMP"
COPY="$OUT/Rune.app"
PROFILE_NAME="${N2_NOTARY_PROFILE:-rune-notary}"

case "${1:-}" in
  sign)
    IDENTITY="${N2_SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | awk -F'"' '/Developer ID Application/ {print $2; exit}')}"
    [ -n "$IDENTITY" ] || { echo "No Developer ID Application certificate on this Mac." >&2; exit 1; }
    [ -e "$COPY" ] && { echo "$COPY is there already (this build was signed for the check before)." >&2; exit 1; }
    mkdir -p "$OUT"
    ditto "$APP" "$COPY"
    # Each program inside, then the app: with the hardened runtime, a time
    # stamp from Apple, and what the app may ask the person for.
    for helper in n2listen:listen n2cal:cal; do
      name="${helper%%:*}"
      [ -f "$COPY/Contents/MacOS/$name" ] || continue
      codesign --force --options runtime --timestamp --entitlements scripts/entitlements.plist \
        -s "$IDENTITY" --identifier "com.jackhagan.notion2.${helper##*:}" "$COPY/Contents/MacOS/$name"
    done
    codesign --force --options runtime --timestamp --entitlements scripts/entitlements.plist -s "$IDENTITY" "$COPY"
    codesign --verify --deep --strict "$COPY"
    echo "Signed for the check: $COPY"
    ;;
  submit)
    [ -d "$COPY" ] || { echo "Sign first: scripts/notarize.sh sign" >&2; exit 1; }
    ditto -c -k --sequesterRsrc --keepParent "$COPY" "$OUT/to-apple.zip"
    xcrun notarytool submit "$OUT/to-apple.zip" --keychain-profile "$PROFILE_NAME" --wait
    xcrun stapler staple "$COPY"
    # The zip to give to people holds the app with Apple's answer attached.
    ditto -c -k --sequesterRsrc --keepParent "$COPY" "$OUT/Rune.zip"
    spctl -a -vv "$COPY" 2>&1 | head -3
    echo "Ready: $OUT/Rune.zip"
    ;;
  *)
    echo "Usage: scripts/notarize.sh sign | submit" >&2
    exit 2
    ;;
esac

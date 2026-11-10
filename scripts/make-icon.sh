#!/usr/bin/env bash
# Regenerate assets/AppIcon.icns and assets/logo-256.png from assets/logo.svg.
set -euo pipefail
cd "$(dirname "$0")/.."
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
swift scripts/make-icon.swift assets/logo.svg "$WORK/AppIcon.iconset" assets/logo-256.png
iconutil -c icns "$WORK/AppIcon.iconset" -o assets/AppIcon.icns
echo "Wrote assets/AppIcon.icns and assets/logo-256.png"

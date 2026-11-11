#!/usr/bin/env bash
# Make the iPhone app icon from Rune's logo. The Mac icon (assets/logo.svg)
# is a rounded tile with a shadow; an iPhone icon is a full square (iOS
# rounds the corners), so mobile/Support/AppIcon.svg is that tile to its
# edges. Change both together.
set -euo pipefail
cd "$(dirname "$0")/../.."
swift mobile/scripts/make-icon.swift

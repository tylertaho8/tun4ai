#!/bin/bash
# Build the menu bar app from source and wrap it in a .app bundle.
#   scripts/bundle-app.sh OUT.app
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:?usage: bundle-app.sh OUT.app}"
if ! command -v cargo >/dev/null 2>&1; then
  echo "Building the menu bar app needs Rust: https://rustup.rs" >&2
  echo "Or download a prebuilt release and run its install.sh." >&2
  exit 1
fi
version="$(awk -F'"' '/^version/{print $2; exit}' "$ROOT/tray/Cargo.toml")"
cargo build --release --manifest-path "$ROOT/tray/Cargo.toml"
rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS"
install -m 755 "$ROOT/tray/target/release/tun4ai" "$OUT/Contents/MacOS/tun4ai"
cat >"$OUT/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>tun4ai</string>
  <key>CFBundleIdentifier</key>
  <string>io.github.tylertaho8.tun4ai</string>
  <key>CFBundleName</key>
  <string>tun4ai</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF
plutil -lint "$OUT/Contents/Info.plist" >/dev/null
codesign --force --sign - "$OUT" >/dev/null 2>&1 || true
echo "built $OUT ($version)"

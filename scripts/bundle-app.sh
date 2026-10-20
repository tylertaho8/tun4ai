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
rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Resources/helper" "$OUT/Contents/Resources/examples"
if [ "${UNIVERSAL:-}" = 1 ]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --release --target aarch64-apple-darwin --manifest-path "$ROOT/tray/Cargo.toml"
  cargo build --release --target x86_64-apple-darwin --manifest-path "$ROOT/tray/Cargo.toml"
  lipo -create -output "$OUT/Contents/MacOS/tun4ai" \
    "$ROOT/tray/target/aarch64-apple-darwin/release/tun4ai" \
    "$ROOT/tray/target/x86_64-apple-darwin/release/tun4ai"
  chmod 755 "$OUT/Contents/MacOS/tun4ai"
else
  cargo build --release --manifest-path "$ROOT/tray/Cargo.toml"
  install -m 755 "$ROOT/tray/target/release/tun4ai" "$OUT/Contents/MacOS/tun4ai"
fi
# The running app installs from these copies, so a downloaded zip needs nothing else.
for f in supervise.sh enable.sh disable.sh firewall-on.sh firewall-off.sh panic-open.sh uninstall-root.sh install-root.sh; do
  install -m 755 "$ROOT/helper/$f" "$OUT/Contents/Resources/helper/$f"
done
install -m 644 "$ROOT/helper/common.sh" "$OUT/Contents/Resources/helper/common.sh"
install -m 644 "$ROOT/helper/render.py" "$OUT/Contents/Resources/helper/render.py"
install -m 644 "$ROOT/examples/vless.txt" "$OUT/Contents/Resources/examples/vless.txt"
install -m 644 "$ROOT/examples/direct.txt" "$OUT/Contents/Resources/examples/direct.txt"
install -m 755 "$ROOT/scripts/fetch-sing-box.sh" "$OUT/Contents/Resources/fetch-sing-box.sh"
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

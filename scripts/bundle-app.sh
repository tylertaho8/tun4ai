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
  lipo -create -output "$OUT/Contents/MacOS/Tun4aiPanel" \
    "$ROOT/tray/target/aarch64-apple-darwin/release/Tun4aiPanel" \
    "$ROOT/tray/target/x86_64-apple-darwin/release/Tun4aiPanel"
  chmod 755 "$OUT/Contents/MacOS/tun4ai" "$OUT/Contents/MacOS/Tun4aiPanel"
else
  cargo build --release --manifest-path "$ROOT/tray/Cargo.toml"
  install -m 755 "$ROOT/tray/target/release/tun4ai" "$OUT/Contents/MacOS/tun4ai"
  install -m 755 "$ROOT/tray/target/release/Tun4aiPanel" "$OUT/Contents/MacOS/Tun4aiPanel"
fi
codesign --force --sign - "$OUT/Contents/MacOS/Tun4aiPanel" >/dev/null 2>&1 || true
# The running app installs from these copies, so a downloaded zip needs nothing else.
for f in supervise.sh enable.sh disable.sh firewall-on.sh firewall-off.sh panic-open.sh uninstall-root.sh install-root.sh; do
  install -m 755 "$ROOT/helper/$f" "$OUT/Contents/Resources/helper/$f"
done
install -m 644 "$ROOT/helper/common.sh" "$OUT/Contents/Resources/helper/common.sh"
install -m 644 "$ROOT/helper/render.py" "$OUT/Contents/Resources/helper/render.py"
install -m 644 "$ROOT/helper/nodes.py" "$OUT/Contents/Resources/helper/nodes.py"
install -m 755 "$ROOT/helper/tun4ai" "$OUT/Contents/Resources/helper/tun4ai"
install -m 644 "$ROOT/examples/nodes.txt" "$OUT/Contents/Resources/examples/nodes.txt"
install -m 644 "$ROOT/examples/direct.txt" "$OUT/Contents/Resources/examples/direct.txt"
install -m 755 "$ROOT/scripts/fetch-sing-box.sh" "$OUT/Contents/Resources/fetch-sing-box.sh"
swift "$ROOT/scripts/render-icon.swift" "$OUT/AppIcon-1024.png"
iconset="$OUT/AppIcon.iconset"
mkdir -p "$iconset"
for spec in "16:icon_16x16.png" "32:icon_16x16@2x.png" "32:icon_32x32.png" "64:icon_32x32@2x.png" \
            "128:icon_128x128.png" "256:icon_128x128@2x.png" "256:icon_256x256.png" "512:icon_256x256@2x.png" \
            "512:icon_512x512.png" "1024:icon_512x512@2x.png"; do
  px="${spec%%:*}"
  name="${spec#*:}"
  sips -z "$px" "$px" "$OUT/AppIcon-1024.png" --out "$iconset/$name" >/dev/null
done
iconutil -c icns "$iconset" -o "$OUT/Contents/Resources/AppIcon.icns"
rm -rf "$iconset" "$OUT/AppIcon-1024.png"
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
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
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

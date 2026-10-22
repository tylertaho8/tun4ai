#!/bin/bash
# Root half of install.sh:
#   sudo helper/install-root.sh OWNER CONFIG_DIR SING_BOX [zh|en]
# With DESTDIR set, everything is staged under that prefix and launchd is left alone (tests).
set -euo pipefail
OWNER="${1:-}"
CONFIG_DIR="${2:-}"
SING_BOX="${3:-}"
UI_LANG="${4:-en}"
DESTDIR="${DESTDIR:-}"
SRC="$(cd "$(dirname "$0")" && pwd)"
LABEL=io.github.tylertaho8.tun4ai
HELPER="$DESTDIR/Library/PrivilegedHelperTools/tun4ai"
STATE="$DESTDIR/Library/Application Support/tun4ai"
PLIST="$DESTDIR/Library/LaunchDaemons/$LABEL.plist"
SUDOERS="$DESTDIR/etc/sudoers.d/tun4ai"
ACTIONS="enable disable firewall-on firewall-off"

die() {
  echo "install-root: $*" >&2
  exit 1
}

if [ -z "$DESTDIR" ] && [ "$(id -u)" -ne 0 ]; then
  die "must run as root"
fi
printf '%s' "$OWNER" | grep -Eq '^[A-Za-z0-9._-]+$' || die "invalid user name: $OWNER"
id -u "$OWNER" >/dev/null 2>&1 || die "no such user: $OWNER"
case "$CONFIG_DIR" in
  /*) ;;
  *) die "CONFIG_DIR must be an absolute path" ;;
esac
[ -x "$SING_BOX" ] || die "sing-box not found at $SING_BOX"
"$SING_BOX" version >/dev/null 2>&1 || die "$SING_BOX does not run"
case "$UI_LANG" in zh | en) ;; *) UI_LANG=en ;; esac

own() {
  if [ "$(id -u)" -eq 0 ]; then chown "$@"; fi
}

if [ -z "$DESTDIR" ] && launchctl print "system/$LABEL" >/dev/null 2>&1; then
  launchctl bootout "system/$LABEL" 2>/dev/null || true
fi

fresh=0
[ -d "$STATE" ] || fresh=1
mkdir -p "$HELPER/bin" "$STATE/run" "$(dirname "$PLIST")" "$(dirname "$SUDOERS")"
for f in supervise.sh enable.sh disable.sh firewall-on.sh firewall-off.sh panic-open.sh uninstall-root.sh; do
  install -m 755 "$SRC/$f" "$HELPER/$f"
done
install -m 644 "$SRC/common.sh" "$HELPER/common.sh"
install -m 644 "$SRC/render.py" "$HELPER/render.py"
install -m 644 "$SRC/nodes.py" "$HELPER/nodes.py"
# The CLI runs as the user; root-owned here so nothing else can swap it. Not in sudoers.
install -m 755 "$SRC/tun4ai" "$HELPER/tun4ai"
install -m 755 "$SING_BOX" "$HELPER/bin/sing-box"
printf 'OWNER=%q\nCONFIG_DIR=%q\nSTATE_DIR=%q\nUI_LANG=%q\n' "$OWNER" "$CONFIG_DIR" "$STATE" "$UI_LANG" >"$HELPER/env"
chmod 644 "$HELPER/env"
# A fresh install touches nothing: tunnel off, firewall off. The user turns them on from the tray.
if [ "$fresh" -eq 1 ]; then
  touch "$STATE/DISABLED" "$STATE/FIREWALL_OFF"
  chmod 644 "$STATE/DISABLED" "$STATE/FIREWALL_OFF"
fi
chmod 755 "$HELPER" "$HELPER/bin" "$STATE" "$STATE/run"
own -R root:wheel "$HELPER" "$STATE"

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
for action in $ACTIONS; do
  printf '%s ALL=(root) NOPASSWD: %s/%s.sh\n' "$OWNER" "$HELPER" "$action"
done >"$tmp"
visudo -cf "$tmp" >/dev/null || die "sudoers check failed, nothing installed"
install -m 440 "$tmp" "$SUDOERS"
own root:wheel "$SUDOERS"

cat >"$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/bash</string>
    <string>$HELPER/supervise.sh</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>$STATE/run/launchd.log</string>
  <key>StandardErrorPath</key>
  <string>$STATE/run/launchd.log</string>
</dict>
</plist>
EOF
plutil -lint "$PLIST" >/dev/null || die "generated launchd plist is invalid"
chmod 644 "$PLIST"
own root:wheel "$PLIST"

if [ -z "$DESTDIR" ]; then
  launchctl bootstrap system "$PLIST"
fi
echo "tun4ai helper installed in $HELPER"

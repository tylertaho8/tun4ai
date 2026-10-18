#!/bin/bash
# Root half of uninstall.sh. Gives the network back and removes everything outside the user's home.
# With DESTDIR set, only files under that prefix are removed (tests).
set -uo pipefail
DESTDIR="${DESTDIR:-}"
LABEL=io.github.tylertaho8.tun4ai
HELPER="$DESTDIR/Library/PrivilegedHelperTools/tun4ai"
STATE="$DESTDIR/Library/Application Support/tun4ai"
PLIST="$DESTDIR/Library/LaunchDaemons/$LABEL.plist"
SUDOERS="$DESTDIR/etc/sudoers.d/tun4ai"

if [ -z "$DESTDIR" ] && [ "$(id -u)" -ne 0 ]; then
  echo "uninstall-root: must run as root" >&2
  exit 1
fi

if [ -z "$DESTDIR" ]; then
  launchctl bootout "system/$LABEL" 2>/dev/null || true
  pkill -f "$HELPER/bin/sing-box run" 2>/dev/null || true
  # pf.loaded exists only while our rules are the ones in the kernel.
  if [ -f "$STATE/run/pf.loaded" ]; then
    /sbin/pfctl -f /etc/pf.conf >/dev/null 2>&1 || true
    /sbin/pfctl -d >/dev/null 2>&1 || true
  fi
fi
rm -rf "$HELPER" "$STATE"
rm -f "$PLIST" "$SUDOERS"
echo "tun4ai helper removed"

#!/bin/bash
# Emergency only: sudo /Library/PrivilegedHelperTools/tun4ai/panic-open.sh
# Stops the tunnel, turns pf off and gives the network back.
set -u
# shellcheck source=helper/common.sh
. "$(cd "$(dirname "$0")" && pwd)/common.sh"
require_root
mkdir -p "$RUN"
touch "$DISABLED" "$FIREWALL_OFF"
chmod 644 "$FIREWALL_OFF"
if [ -f "$RUN/sing-box.pid" ]; then
  kill "$(cat "$RUN/sing-box.pid")" 2>/dev/null || true
fi
pkill -f "$BOX run" 2>/dev/null || true
"$PFCTL" -f /etc/pf.conf >/dev/null 2>&1 || true
"$PFCTL" -d >/dev/null 2>&1 || true
rm -f "$ENABLED" "$RUN/sing-box.pid"
msg="$(t "隧道和防火墙都已关闭，网络已直连。" "Tunnel and firewall are off. Traffic goes direct.")"
result "$msg"
echo "$msg"

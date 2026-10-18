#!/bin/bash
# User asked for a direct path. Leave the tunnel running if it is up.
# FIREWALL_OFF tells the supervisor not to load the kill switch again.
set -u
# shellcheck source=helper/common.sh
. "$(cd "$(dirname "$0")" && pwd)/common.sh"
require_root
mkdir -p "$RUN"
touch "$FIREWALL_OFF"
chmod 644 "$FIREWALL_OFF"
# With the tunnel up, keep only the DNS redirect. Otherwise DNS would go out in the clear.
if ifconfig "$TUN_IF" >/dev/null 2>&1 && [ -f "$PF_DNS_CONF" ]; then
  "$PFCTL" -f "$PF_DNS_CONF" >>"$RUN/pfctl.log" 2>&1 || true
  "$PFCTL" -e >>"$RUN/pfctl.log" 2>&1 || true
  msg="$(t "防火墙已关掉。隧道还在，DNS 仍走隧道。隧道一断，外网就会从网卡漏出去。" \
    "Firewall off. The tunnel and its DNS stay up, but if the tunnel drops, traffic leaks out directly.")"
  result "$msg"
  echo "$msg"
  exit 0
fi
"$PFCTL" -f /etc/pf.conf >>"$RUN/pfctl.log" 2>&1 || true
"$PFCTL" -d >>"$RUN/pfctl.log" 2>&1 || true
if "$PFCTL" -s info 2>/dev/null | grep -q "Status: Enabled"; then
  msg="FAIL $(t "防火墙没有关掉" "pf is still enabled")"
  result "$msg"
  echo "$msg" >&2
  exit 1
fi
msg="$(t "防火墙已关掉。隧道也没开，网络已直连。" "Firewall off. The tunnel is off too, traffic goes direct.")"
result "$msg"
echo "$msg"

#!/bin/bash
# Stop the tunnel. With the firewall on, public traffic stays blocked.
set -u
# shellcheck source=helper/common.sh
. "$(cd "$(dirname "$0")" && pwd)/common.sh"
require_root
mkdir -p "$RUN"
touch "$DISABLED"
pid="$(cat "$RUN/sing-box.pid" 2>/dev/null)"
if [ -n "$pid" ] && [ "$(ps -ww -p "$pid" -o command= 2>/dev/null)" = "$BOX run -c $CONFIG_JSON" ]; then
  kill "$pid" 2>/dev/null || true
  sleep 0.3
  kill -9 "$pid" 2>/dev/null || true
fi
pkill -f "$BOX run" 2>/dev/null || true
rm -f "$RUN/sing-box.pid" "$ENABLED"
if [ -f "$FIREWALL_OFF" ]; then
  # The DNS redirect points at the tunnel that just went away.
  "$PFCTL" -f /etc/pf.conf >>"$RUN/pfctl.log" 2>&1 || true
  "$PFCTL" -d >>"$RUN/pfctl.log" 2>&1 || true
  msg="$(t "已关闭。防火墙也是关的，网络已直连。" "Tunnel stopped. Firewall is off, traffic goes direct.")"
else
  msg="$(t "已关闭。外网已切断，防火墙仍开着。" "Tunnel stopped. Firewall is on, internet is blocked.")"
fi
result "$msg"
echo "$msg"

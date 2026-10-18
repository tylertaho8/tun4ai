#!/bin/bash
# Turn the kill switch on. Without a tunnel, public traffic is blocked from now on.
set -u
# shellcheck source=helper/common.sh
. "$(cd "$(dirname "$0")" && pwd)/common.sh"
require_root
mkdir -p "$RUN"
rm -f "$FIREWALL_OFF"
fail() {
  result "FAIL $1"
  echo "FAIL $1" >&2
  exit 1
}
if [ ! -f "$PF_CONF" ]; then
  iface="$(default_iface)"
  [ -n "$iface" ] || fail "$(t "没有网络，防火墙规则还没生成" "no network, rules not rendered yet")"
  render_config "$iface" >>"$RUN/supervise.log" 2>&1 \
    || fail "$(t "防火墙规则没有生成，看 supervise.log" "could not render rules, see supervise.log")"
fi
"$PFCTL" -f "$PF_CONF" >>"$RUN/pfctl.log" 2>&1 || true
"$PFCTL" -e >>"$RUN/pfctl.log" 2>&1 || true
"$PFCTL" -s info 2>/dev/null | grep -q "Status: Enabled" || fail "$(t "防火墙没有打开" "pf did not turn on")"
msg="$(t "防火墙已打开。外网只走隧道。" "Firewall on. Internet only goes through the tunnel.")"
result "$msg"
echo "$msg"

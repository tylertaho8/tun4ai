#!/bin/bash
# Ask the supervisor to start the tunnel. The firewall setting is left as it is.
set -u
# shellcheck source=helper/common.sh
. "$(cd "$(dirname "$0")" && pwd)/common.sh"
require_root
mkdir -p "$RUN"
: >"$RUN/last-result.txt"
rm -f "$DISABLED"
for _ in $(seq 1 20); do
  if [ -f "$ENABLED" ] && [ -f "$RUN/sing-box.pid" ] && kill -0 "$(cat "$RUN/sing-box.pid")" 2>/dev/null \
    && ifconfig "$TUN_IF" >/dev/null 2>&1; then
    result "OK"
    t "已开启。" "Tunnel is up."
    echo
    exit 0
  fi
  sleep 1
done
msg="$(cat "$RUN/last-result.txt" 2>/dev/null)"
if [ -z "$msg" ] || [ "$msg" = "OK" ]; then
  msg="$(t "隧道没有起来" "The tunnel did not come up")"
fi
result "$msg"
echo "$msg" >&2
exit 1

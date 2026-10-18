# shellcheck shell=bash disable=SC2034
# SC2034: the variables below are used by the scripts that source this file.
# Sourced by every root helper. Paths live here and in env, nowhere else.
# Everything this file points at is root-owned; the only user-owned input is CONFIG_DIR,
# which render.py reads but never executes.

HELPER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ ! -f "$HELPER_DIR/env" ]; then
  echo "tun4ai: $HELPER_DIR/env is missing. Run install.sh." >&2
  exit 1
fi
# shellcheck source=/dev/null
. "$HELPER_DIR/env"
: "${OWNER:?}" "${CONFIG_DIR:?}" "${STATE_DIR:?}"
UI_LANG="${UI_LANG:-en}"
export TUN4AI_LANG="$UI_LANG"

PYTHON=/usr/bin/python3
PFCTL=/sbin/pfctl
# Must match TUN_IF in render.py.
TUN_IF=utun15
BOX="$HELPER_DIR/bin/sing-box"
RENDER="$HELPER_DIR/render.py"
RUN="$STATE_DIR/run"
CONFIG_JSON="$STATE_DIR/config.json"
PF_CONF="$STATE_DIR/pf.killswitch.conf"
PF_DNS_CONF="$STATE_DIR/pf.dns.conf"
ENABLED="$STATE_DIR/ENABLED"
DISABLED="$STATE_DIR/DISABLED"
FIREWALL_OFF="$STATE_DIR/FIREWALL_OFF"

# t "中文" "English"
t() {
  if [ "$UI_LANG" = zh ]; then printf '%s' "$1"; else printf '%s' "$2"; fi
}

require_root() {
  if [ "$(id -u)" -ne 0 ]; then
    { t "需要管理员权限" "Must run as root"; echo; } >&2
    exit 1
  fi
}

# The one line the tray shows after an action.
result() {
  mkdir -p "$RUN"
  printf '%s\n' "$1" >"$RUN/last-result.txt"
  chmod 644 "$RUN/last-result.txt"
}

render_config() {
  "$PYTHON" "$RENDER" --if "$1" --config-dir "$CONFIG_DIR" --state-dir "$STATE_DIR" --owner "$OWNER"
}

default_iface() {
  route -n get default 2>/dev/null | awk '/interface:/{print $2}'
}

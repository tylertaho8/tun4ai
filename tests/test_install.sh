#!/bin/bash
# Stages the root install under a temp DESTDIR, exercises the installed helper, then uninstalls.
# Touches nothing outside the temp directory: no launchd, no pf, no sudoers.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export DESTDIR="$tmp/root"
HELPER="$DESTDIR/Library/PrivilegedHelperTools/tun4ai"
STATE="$DESTDIR/Library/Application Support/tun4ai"
PLIST="$DESTDIR/Library/LaunchDaemons/io.github.tylertaho8.tun4ai.plist"
SUDOERS="$DESTDIR/etc/sudoers.d/tun4ai"
CONFIG="$tmp/config dir"
fails=0

ok() { echo "ok   $1"; }
bad() { echo "FAIL $1" >&2; fails=$((fails + 1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1"; fi; }

# Off macOS, stand in for the two Apple tools install-root.sh calls. The plutil shim parses the plist
# with Python's plistlib, so a malformed plist still fails; visudo is the real one from sudo.
if [ "$(uname -s)" != Darwin ]; then
  mkdir -p "$tmp/shims"
  cat >"$tmp/shims/plutil" <<'SHIM'
#!/usr/bin/python3
import plistlib, sys
args = sys.argv[1:]
with open(args[-1], "rb") as f:
    data = plistlib.load(f)
if args[0] == "-extract":
    for key in args[1].split("."):
        data = data[int(key)] if isinstance(data, list) else data[key]
    print(data)
SHIM
  chmod 755 "$tmp/shims/plutil"
  export PATH="$tmp/shims:$PATH:/usr/sbin:/sbin"
fi
mode_of() {
  if [ "$(uname -s)" = Darwin ]; then stat -f %Lp "$1"; else stat -c %a "$1"; fi
}

box="${SING_BOX:-$ROOT/build/sing-box}"
if [ ! -x "$box" ]; then
  box="$tmp/fake-sing-box"
  printf '#!/bin/sh\necho "sing-box version test"\n' >"$box"
  chmod 755 "$box"
fi

mkdir -p "$CONFIG"
printf 'vless://11111111-1111-4111-8111-111111111111@203.0.113.7:443?pbk=Vdp5mc4z6trF6_wM_BjEK2FeiZts-oO4wPUSPcdf2jc&sid=abcd&sni=node.example.com\n' >"$CONFIG/vless.txt"
cp "$ROOT/examples/direct.txt" "$CONFIG/direct.txt"

for f in "$ROOT"/install.sh "$ROOT"/uninstall.sh "$ROOT"/helper/*.sh "$ROOT"/scripts/*.sh; do
  check "bash -n $(basename "$f")" "bash -n '$f'"
done
check "no personal home path in shipped files" \
  "! grep -rIl '/Users/' '$ROOT/helper' '$ROOT/scripts' '$ROOT/tray/src' '$ROOT/install.sh' '$ROOT/uninstall.sh' '$ROOT/examples'"

bash "$ROOT/helper/install-root.sh" "$(id -un)" "$CONFIG" "$box" zh >/dev/null
for f in common.sh supervise.sh render.py nodes.py tun4ai enable.sh disable.sh firewall-on.sh firewall-off.sh panic-open.sh uninstall-root.sh env bin/sing-box; do
  check "installed $f" "[ -f '$HELPER/$f' ]"
done
check "scripts are executable, libraries are not" \
  "[ -x '$HELPER/supervise.sh' ] && [ -x '$HELPER/tun4ai' ] && [ ! -x '$HELPER/common.sh' ] && [ ! -x '$HELPER/nodes.py' ]"
# shellcheck disable=SC2034  # read inside check's eval
listed="$(TUN4AI_CONFIG_DIR="$CONFIG" TUN4AI_STATE_DIR="$STATE" TUN4AI_HELPER_DIR="$HELPER" "$HELPER/tun4ai" node list --json)"
check "installed CLI finds its modules and reads the legacy vless.txt" \
  "[[ \"\$listed\" == *'\"file\": \"vless.txt\"'* ]] && [[ \"\$listed\" == *'\"protocol\": \"vless\"'* ]]"
check "installed CLI does not leave bytecode in the root folder" "[ ! -e '$HELPER/__pycache__' ]"
check "fresh install leaves tunnel and firewall off" "[ -f '$STATE/DISABLED' ] && [ -f '$STATE/FIREWALL_OFF' ]"

# shellcheck source=/dev/null
if ( . "$HELPER/env"; [ "$OWNER" = "$(id -un)" ] && [ "$CONFIG_DIR" = "$CONFIG" ] && [ "$STATE_DIR" = "$STATE" ] && [ "$UI_LANG" = zh ] ); then
  ok "env round-trips paths with spaces"
else
  bad "env round-trips paths with spaces"
fi

check "sudoers grants exactly the four tray actions" "[ \"\$(wc -l <'$SUDOERS' | tr -d ' ')\" = 4 ] && ! grep -v 'NOPASSWD: $HELPER/' '$SUDOERS' >/dev/null"
check "sudoers parses" "visudo -cf '$SUDOERS' >/dev/null 2>&1"
check "sudoers is 0440" "[ \"\$(mode_of '$SUDOERS')\" = 440 ]"
check "launchd plist is valid" "plutil -lint '$PLIST' >/dev/null"
check "launchd runs the installed supervisor" \
  "[ \"\$(plutil -extract ProgramArguments.1 raw '$PLIST')\" = '$HELPER/supervise.sh' ]"

check "helper refuses to run without root" "! bash '$HELPER/enable.sh' 2>/dev/null"
# shellcheck disable=SC2034  # read inside check's eval
refusal="$(bash "$HELPER/enable.sh" 2>&1 || true)"
check "refusal speaks the installed language" "[[ \"\$refusal\" == *管理员* ]]"

iface="$(route -n get default 2>/dev/null | awk '/interface:/{print $2}' || true)"
if [ -n "$iface" ]; then
  # shellcheck disable=SC2016
  check "installed render writes into the state dir" \
    "bash -c '. \"\$1/common.sh\"; render_config \"\$2\"' _ '$HELPER' '$iface' >/dev/null 2>&1 && [ -f '$STATE/config.json' ] && [ -f '$STATE/pf.killswitch.conf' ]"
  if "$box" version 2>/dev/null | grep -q '^sing-box version [0-9]'; then
    check "sing-box accepts the rendered config" "'$HELPER/bin/sing-box' check -c '$STATE/config.json'"
  fi
fi

rm -f "$STATE/FIREWALL_OFF"
bash "$ROOT/helper/install-root.sh" "$(id -un)" "$CONFIG" "$box" en >/dev/null
check "reinstall keeps the user's firewall choice" "[ ! -f '$STATE/FIREWALL_OFF' ]"
check "reinstall switches language" "grep -q '^UI_LANG=en$' '$HELPER/env'"

check "bad user name is refused" "! bash '$ROOT/helper/install-root.sh' 'x;y' '$CONFIG' '$box' 2>/dev/null"
check "relative config dir is refused" "! bash '$ROOT/helper/install-root.sh' '$(id -un)' relative '$box' 2>/dev/null"

bash "$HELPER/uninstall-root.sh" >/dev/null
check "uninstall removes helper, state, plist and sudoers" \
  "[ ! -e '$HELPER' ] && [ ! -e '$STATE' ] && [ ! -e '$PLIST' ] && [ ! -e '$SUDOERS' ]"
check "uninstall leaves the user's config" "[ -f '$CONFIG/vless.txt' ]"

if [ "$fails" -gt 0 ]; then
  echo "$fails check(s) failed" >&2
  exit 1
fi
echo "all install checks passed"

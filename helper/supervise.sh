#!/bin/bash
# Keep the firewall up unless the user turned it off from the tray.
# If the tunnel is down and the firewall is up, public traffic stays blocked.
set -u
# shellcheck source=helper/common.sh
. "$(cd "$(dirname "$0")" && pwd)/common.sh"
require_root
# "<conf path>|<conf checksum>|<loaded rules checksum>" from the last load.
PF_STAMP="$RUN/pf.loaded"
mkdir -p "$RUN"
chmod 755 "$STATE_DIR" "$RUN"
for name in pfctl.log supervise.log sing-box.log; do
  if [ -f "$RUN/$name" ] && [ "$(stat -f %z "$RUN/$name")" -gt 1048576 ]; then
    mv -f "$RUN/$name" "$RUN/$name.old"
  fi
done

log() {
  printf '%s %s\n' "$(date '+%F %T')" "$*" >>"$RUN/supervise.log"
}

conf_sum() {
  cksum <"$1" 2>/dev/null
}

rules_sum() {
  "$PFCTL" -sr 2>/dev/null | cksum
}

pf_name() {
  if [ "$1" = "$PF_CONF" ]; then t "防火墙规则" "kill switch rules"; else t "DNS 转发规则" "DNS redirect rules"; fi
}

load_pf() {
  if [ ! -f "$1" ]; then
    return 0
  fi
  printf '%s load %s: %s\n' "$(date '+%F %T')" "$1" "$2" >>"$RUN/pfctl.log"
  log "$(t "加载" "loading ")$(pf_name "$1")$(t "：" ": ")$2"
  "$PFCTL" -f "$1" >>"$RUN/pfctl.log" 2>&1 || true
  "$PFCTL" -e >>"$RUN/pfctl.log" 2>&1 || true
  printf '%s|%s|%s\n' "$1" "$(conf_sum "$1")" "$(rules_sum)" >"$PF_STAMP"
}

pf_enabled() {
  "$PFCTL" -s info 2>/dev/null | grep -q "Status: Enabled"
}

# Prints why $1 must be loaded. Silent when it is already in place.
pf_drift() {
  local stamp="" path sum rules
  if ! pf_enabled; then
    t "pf 没开" "pf is disabled"
    return
  fi
  [ -f "$PF_STAMP" ] && stamp="$(cat "$PF_STAMP")"
  IFS='|' read -r path sum rules <<<"$stamp"
  if [ "$path" != "$1" ]; then
    t "防火墙开关变了" "firewall switch changed"
  elif [ "$sum" != "$(conf_sum "$1")" ]; then
    t "规则文件变了" "rules file changed"
  elif [ "$rules" != "$(rules_sum)" ]; then
    t "内核里的规则被改过" "rules in the kernel were changed"
  fi
}

# Firewall on: the kill switch. Firewall off with the tunnel up: only the DNS redirect,
# or DNS would leave in the clear. Neither: pf off.
pf_target() {
  if [ ! -f "$FIREWALL_OFF" ]; then
    echo "$PF_CONF"
  elif box_alive; then
    echo "$PF_DNS_CONF"
  fi
}

ensure_firewall() {
  local conf why
  conf="$(pf_target)"
  if [ -z "$conf" ]; then
    rm -f "$PF_STAMP"
    if pf_enabled; then
      log "$(t "防火墙按你的设置关闭" "firewall turned off as requested")"
      "$PFCTL" -f /etc/pf.conf >>"$RUN/pfctl.log" 2>&1 || true
      "$PFCTL" -d >>"$RUN/pfctl.log" 2>&1 || true
    fi
    return 0
  fi
  why="$(pf_drift "$conf")"
  if [ -n "$why" ]; then
    load_pf "$conf" "$why"
  fi
}

# A pid only counts if it is still our sing-box. Pids get reused.
box_pid() {
  local pid
  pid="$(cat "$RUN/sing-box.pid" 2>/dev/null)"
  [ -n "$pid" ] || return 1
  [ "$(ps -ww -p "$pid" -o command= 2>/dev/null)" = "$BOX run -c $CONFIG_JSON" ] || return 1
  echo "$pid"
}

box_alive() {
  box_pid >/dev/null
}

stop_box() {
  local pid
  if pid="$(box_pid)"; then
    kill "$pid" 2>/dev/null || true
    sleep 0.3
    kill -9 "$pid" 2>/dev/null || true
  fi
  pkill -f "$BOX run" 2>/dev/null || true
  rm -f "$RUN/sing-box.pid" "$ENABLED"
}

# On failure START_ERROR says why, in one line.
START_ERROR=""
start_box() {
  local route iface out
  route="$(default_route)"
  iface="${route%% *}"
  if [ -z "$route" ] || [ "$iface" = "$route" ]; then
    START_ERROR="$(t "没有默认网络" "no default network")"
    return 1
  fi
  if ! ensure_direct_group; then
    START_ERROR="$(t "建不了用户组 $DIRECT_GROUP" "could not create group $DIRECT_GROUP")"
    return 1
  fi
  if ! out="$(render_config "$iface" 2>&1)"; then
    START_ERROR="$(printf '%s\n' "$out" | tail -1)"
    return 1
  fi
  log "$(t "配置已生成：" "config rendered: ")$out"
  ensure_firewall
  # Own session, so restarting this supervisor does not kill the tunnel.
  if ! "$PYTHON" - "$BOX" "$CONFIG_JSON" "$RUN/sing-box.log" "$RUN/sing-box.pid" <<'PY'
import grp
import os
import select
import sys

box, cfg, log, pidfile = sys.argv[1:]
read_fd, write_fd = os.pipe()
if os.fork():
    os.close(write_fd)
    ready, _, _ = select.select([read_fd], [], [], 5)
    got = os.read(read_fd, 1) if ready else b""
    os._exit(0 if got == b"1" else 1)
os.setsid()
os.close(read_fd)
# Effective group is fixed before any socket exists. pf stores that group
# on the socket. uid stays 0 so the utun can still be created.
gid = grp.getgrnam("_tun4ai").gr_gid
os.setgroups([gid])
os.setgid(gid)
with open(pidfile, "w") as handle:
    handle.write(f"{os.getpid()}\n")
os.chmod(pidfile, 0o644)
fd = os.open(log, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o600)
os.dup2(fd, 1)
os.dup2(fd, 2)
if fd > 2:
    os.close(fd)
null = os.open("/dev/null", os.O_RDONLY)
os.dup2(null, 0)
if null > 2:
    os.close(null)
os.write(write_fd, b"1")
os.close(write_fd)
os.execv(box, [box, "run", "-c", cfg])
PY
  then
    START_ERROR="$(t "sing-box 没能启动，看 sing-box.log" "sing-box did not start, see sing-box.log")"
    return 1
  fi
  local tries=0
  while [ "$tries" -lt 20 ] && ! ifconfig "$TUN_IF" >/dev/null 2>&1; do
    tries=$((tries + 1))
    sleep 0.3
  done
  if ! ifconfig "$TUN_IF" >/dev/null 2>&1; then
    stop_box
    START_ERROR="$(t "6 秒内没有建出 $TUN_IF，看 sing-box.log" "$TUN_IF did not appear within 6 seconds, see sing-box.log")"
    return 1
  fi
  printf '%s\n' "$route" >"$RUN/bound-iface"
  touch "$ENABLED"
  ensure_firewall
  result "OK"
  log "$(t "隧道已连上：sing-box 进程 " "tunnel up: sing-box pid ")$(cat "$RUN/sing-box.pid" 2>/dev/null)$(t "，网卡 " ", interface ")$iface"
  return 0
}

# "<interface> <gateway>". Joining another Wi-Fi keeps en0 but changes the gateway.
default_route() {
  route -n get default 2>/dev/null | awk '/interface:/{i=$2} /gateway:/{g=$2} END{if (i != "" && g != "") print i, g}'
}

log "$(t "看门狗启动" "supervisor started")"
# A failing start is retried every few seconds. Log each distinct error once, not every retry.
retries=0
last_error=""
was_disabled=""
while true; do
  ensure_firewall
  if [ -f "$DISABLED" ]; then
    if box_alive; then
      log "$(t "按你的操作关闭隧道" "tunnel stopped as requested")"
      stop_box
    fi
    was_disabled=1
    sleep 2
    continue
  fi
  route="$(default_route)"
  bound="$(cat "$RUN/bound-iface" 2>/dev/null || true)"
  if [ -z "$route" ]; then
    sleep 2
    continue
  fi
  why=""
  if ! box_alive; then
    if [ -n "$was_disabled" ]; then
      why="$(t "按你的操作开启隧道" "tunnel started as requested")"
    else
      why="$(t "sing-box 进程不在了" "sing-box exited")"
    fi
  elif [ "$route" != "$bound" ]; then
    why="$(t "默认网络从 ${bound:-无} 换成 $route" "default network changed from ${bound:-none} to $route")"
  fi
  was_disabled=""
  if [ -n "$why" ]; then
    [ "$retries" -gt 0 ] || log "$(t "重建隧道：" "rebuilding tunnel: ")$why"
    stop_box
    if ! start_box; then
      retries=$((retries + 1))
      if [ "$START_ERROR" != "$last_error" ]; then
        log "$(t "隧道没起来：$START_ERROR（每 7 秒重试，同样的错误不再重复记录）" "tunnel failed: $START_ERROR (retrying every 7 seconds; repeats are not logged)")"
        last_error="$START_ERROR"
      fi
      if [ -f "$FIREWALL_OFF" ]; then
        result "$(t "隧道没起来：$START_ERROR" "Tunnel failed: $START_ERROR")"
      else
        result "$(t "隧道没起来：$START_ERROR。外网保持切断" "Tunnel failed: $START_ERROR. Internet stays blocked")"
      fi
      sleep 5
      continue
    fi
    [ "$retries" -eq 0 ] || log "$(t "重试 $retries 次后恢复" "recovered after $retries retries")"
    retries=0
    last_error=""
  fi
  sleep 3
done

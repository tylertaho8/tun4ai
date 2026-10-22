#!/bin/bash
# Install tun4ai for the current user. Asks for the admin password once.
#   ./install.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
APP_DEST="$HOME/Applications/tun4ai.app"
CONFIG_DIR="$HOME/Library/Application Support/tun4ai"
BUILD="$ROOT/build"

first_lang="$(defaults read -g AppleLanguages 2>/dev/null | sed -n '2p' | tr -d ' ",')"
case "$first_lang" in zh*) UI_LANG=zh ;; *) UI_LANG=en ;; esac
t() { if [ "$UI_LANG" = zh ]; then printf '%s\n' "$1"; else printf '%s\n' "$2"; fi; }
die() { t "错误：$1" "Error: $2" >&2; exit 1; }

[ "$(uname -s)" = Darwin ] || die "tun4ai 只支持 macOS" "tun4ai only runs on macOS"
[ "$(id -u)" -ne 0 ] || die "请用普通用户运行，不要加 sudo" "run this as your normal user, not with sudo"
xcode-select -p >/dev/null 2>&1 \
  || die "需要 Xcode 命令行工具，先运行：xcode-select --install" \
         "Xcode Command Line Tools are required: xcode-select --install"

t "1/4 下载并校验 sing-box" "1/4 Downloading and verifying sing-box"
"$ROOT/scripts/fetch-sing-box.sh" "$BUILD"

t "2/4 准备菜单栏程序" "2/4 Preparing the menu bar app"
if [ -d "$ROOT/tun4ai.app" ]; then
  app="$ROOT/tun4ai.app"
else
  "$ROOT/scripts/bundle-app.sh" "$BUILD/tun4ai.app"
  app="$BUILD/tun4ai.app"
fi
pkill -x tun4ai 2>/dev/null || true
mkdir -p "$HOME/Applications"
rm -rf "$APP_DEST"
ditto "$app" "$APP_DEST"
xattr -dr com.apple.quarantine "$APP_DEST" 2>/dev/null || true

t "3/4 准备配置目录 $CONFIG_DIR" "3/4 Preparing $CONFIG_DIR"
mkdir -p "$CONFIG_DIR"
chmod 700 "$CONFIG_DIR"
# An existing vless.txt is the node list until the first `tun4ai node` edit renames it to nodes.txt.
# A template nodes.txt next to it would hide it, so only a brand-new folder gets one.
if [ ! -e "$CONFIG_DIR/nodes.txt" ] && [ ! -e "$CONFIG_DIR/vless.txt" ]; then
  install -m 600 "$ROOT/examples/nodes.txt" "$CONFIG_DIR/nodes.txt"
fi
if [ ! -f "$CONFIG_DIR/direct.txt" ]; then
  install -m 644 "$ROOT/examples/direct.txt" "$CONFIG_DIR/direct.txt"
fi

t "4/4 安装后台服务（需要管理员密码）" "4/4 Installing the background service (admin password needed)"
sudo /bin/bash "$ROOT/helper/install-root.sh" "$(id -un)" "$CONFIG_DIR" "$BUILD/sing-box" "$UI_LANG"

open "$APP_DEST"
t "
安装完成。隧道和防火墙目前都是关的，网络没有任何变化。
下一步：
  1. 菜单栏图标 → 节点管理，每行粘贴一个分享链接（vless/vmess/trojan/ss/hysteria2/tuic）
  2. 菜单栏图标 → 开启隧道
  3. 确认能上网后，再勾选「防火墙」
卸载：./uninstall.sh" "
Installed. The tunnel and the firewall are both off; nothing about your network has changed.
Next:
  1. Menu bar icon → Node, paste one share link per line (vless/vmess/trojan/ss/hysteria2/tuic)
  2. Menu bar icon → Start tunnel
  3. Once it works, tick \"Firewall\"
Uninstall: ./uninstall.sh"

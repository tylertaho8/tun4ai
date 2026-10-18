#!/bin/bash
# Remove tun4ai and give the network back.
#   ./uninstall.sh           keep your node and direct list
#   ./uninstall.sh --purge   delete them too
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
CONFIG_DIR="$HOME/Library/Application Support/tun4ai"
INSTALLED="/Library/PrivilegedHelperTools/tun4ai/uninstall-root.sh"

pkill -x tun4ai 2>/dev/null || true
if [ -f "$INSTALLED" ]; then
  sudo /bin/bash "$INSTALLED"
else
  sudo /bin/bash "$ROOT/helper/uninstall-root.sh"
fi
rm -rf "$HOME/Applications/tun4ai.app"
if [ "${1:-}" = "--purge" ]; then
  rm -rf "$CONFIG_DIR"
  echo "Removed $CONFIG_DIR"
else
  echo "Kept $CONFIG_DIR (run with --purge to delete it)"
fi
echo "tun4ai uninstalled"

#!/bin/bash
# Download the pinned sing-box release for this Mac into DIR and verify its SHA-256.
#   scripts/fetch-sing-box.sh DIR
set -euo pipefail
VERSION=1.14.3
# From the release's asset digests on GitHub.
SHA256_ARM64=0360ce634a04c26a6b4fd957517987bb4f6c8950f63435ce3c5af2fcae5860c5
SHA256_AMD64=6d002b74d03b478547349cc10af89e007868596e5aa931816d45da9f544715a9
DEST="${1:?usage: fetch-sing-box.sh DIR}"

case "$(uname -m)" in
  arm64) arch=arm64 sum=$SHA256_ARM64 ;;
  x86_64) arch=amd64 sum=$SHA256_AMD64 ;;
  *) echo "unsupported CPU: $(uname -m)" >&2; exit 1 ;;
esac
mkdir -p "$DEST"
if [ -x "$DEST/sing-box" ] && [ "$("$DEST/sing-box" version 2>/dev/null | head -1)" = "sing-box version $VERSION" ]; then
  echo "sing-box $VERSION already in $DEST"
  exit 0
fi
name="sing-box-$VERSION-darwin-$arch"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
echo "downloading $name"
curl -fsSL --retry 3 -o "$tmp/$name.tar.gz" \
  "https://github.com/SagerNet/sing-box/releases/download/v$VERSION/$name.tar.gz"
if ! echo "$sum  $tmp/$name.tar.gz" | shasum -a 256 -c - >/dev/null 2>&1; then
  echo "checksum mismatch for $name.tar.gz, refusing to install it" >&2
  exit 1
fi
tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
install -m 755 "$tmp/$name/sing-box" "$DEST/sing-box"
echo "sing-box $VERSION verified and placed in $DEST"

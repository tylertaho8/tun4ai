#!/bin/bash
# Download the pinned sing-box release for this Mac into DIR and verify its SHA-256.
# Linux builds are pinned too, only so `make check` can run `sing-box check` on a Linux dev box.
#   scripts/fetch-sing-box.sh DIR
set -euo pipefail
VERSION=1.14.3
# From the release's asset digests on GitHub.
SHA256_ARM64=0360ce634a04c26a6b4fd957517987bb4f6c8950f63435ce3c5af2fcae5860c5
SHA256_AMD64=6d002b74d03b478547349cc10af89e007868596e5aa931816d45da9f544715a9
SHA256_LINUX_ARM64=29dae24d76bea3bc62d07dd3e60cfc12003de7debbd6b768df9be9c68360174e
SHA256_LINUX_AMD64=e2bdf179c15a3652955dc44867e33ca0965c9c9b91b34267dcc5a5d639a5feee
DEST="${1:?usage: fetch-sing-box.sh DIR}"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) os=darwin arch=arm64 sum=$SHA256_ARM64 ;;
  Darwin-x86_64) os=darwin arch=amd64 sum=$SHA256_AMD64 ;;
  Linux-aarch64) os=linux arch=arm64 sum=$SHA256_LINUX_ARM64 ;;
  Linux-x86_64) os=linux arch=amd64 sum=$SHA256_LINUX_AMD64 ;;
  *) echo "unsupported system: $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac
mkdir -p "$DEST"
if [ -x "$DEST/sing-box" ] && [ "$("$DEST/sing-box" version 2>/dev/null | head -1)" = "sing-box version $VERSION" ]; then
  echo "sing-box $VERSION already in $DEST"
  exit 0
fi
name="sing-box-$VERSION-$os-$arch"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
echo "downloading $name"
curl -fsSL --retry 3 -o "$tmp/$name.tar.gz" \
  "https://github.com/SagerNet/sing-box/releases/download/v$VERSION/$name.tar.gz"
if command -v shasum >/dev/null 2>&1; then sha256() { shasum -a 256 "$@"; }; else sha256() { sha256sum "$@"; }; fi
if ! echo "$sum  $tmp/$name.tar.gz" | sha256 -c - >/dev/null 2>&1; then
  echo "checksum mismatch for $name.tar.gz, refusing to install it" >&2
  exit 1
fi
tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
install -m 755 "$tmp/$name/sing-box" "$DEST/sing-box"
echo "sing-box $VERSION verified and placed in $DEST"

#!/bin/bash
# Run the tray's platform-independent Rust tests (nodes.rs, panel.rs, setup.rs) on a machine that
# cannot build the macOS app, e.g. Linux. The same source files are compiled into a scratch crate;
# only objc2::MainThreadMarker, which panel.rs names in one signature, is stubbed.
# On macOS use `cargo test` in tray/ instead; it runs these and the rest.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
work="${TMPDIR:-/tmp}/tun4ai-tray-portable"
mkdir -p "$work/src" "$work/objc2stub/src"
cat >"$work/Cargo.toml" <<TOML
[package]
name = "tun4ai-tray-portable"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
serde_json = "1"
objc2 = { path = "objc2stub" }
TOML
printf '[package]\nname = "objc2"\nversion = "0.6.0"\nedition = "2021"\npublish = false\n' >"$work/objc2stub/Cargo.toml"
echo 'pub struct MainThreadMarker;' >"$work/objc2stub/src/lib.rs"
{
  for module in nodes panel setup; do
    printf '#[path = "%s/tray/src/%s.rs"]\npub mod %s;\n' "$ROOT" "$module" "$module"
  done
} >"$work/src/lib.rs"
cargo test --quiet --manifest-path "$work/Cargo.toml"

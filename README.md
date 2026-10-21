# tun4ai

English | [简体中文](README.zh-CN.md)

A whole-machine TUN tunnel with a kill switch for macOS, so AI tools that ignore proxy settings still go through your node and never leak out of the wrong exit.

Claude Code, Codex, Cursor, language servers, `git`, `npm`, Docker builds: many of them ignore `HTTP_PROXY`, open their own sockets, or spawn helpers that do. AI providers also tend to judge your account by the IP they see. tun4ai routes all TCP on the Mac through one VLESS Reality node, and its pf firewall blocks the internet when the tunnel is down instead of quietly falling back to your real IP.

## Features

- **Whole-machine TUN**: every app, every process, no per-app proxy settings.
- **Kill switch**: with the firewall on, only the tunnel and your node's server can reach the internet. If the tunnel drops, traffic stops instead of leaking.
- **DNS that doesn't leak**: lookups are answered over DoH through the tunnel; pf sends queries aimed at your router's DNS into the tunnel too.
- **Direct list**: LAN ranges and domains you choose stay off the tunnel.
- **Menu bar panel**: click the icon for a frosted panel instead of a menu. Status, latency and leak results stay live while it is open. Chinese and English follow your system language.
- **Self-healing**: a root launchd supervisor restarts sing-box and rebuilds rules when Wi-Fi or Ethernet changes.
- **Off by default**: installing changes nothing about your network until you start the tunnel.

## How it works

```
apps ──► utun15 (sing-box TUN) ──► VLESS Reality ──► your server ──► internet
              │
              └── direct list ──► your router
pf kill switch: anything else leaving the Wi-Fi / Ethernet interface is blocked
```

[sing-box](https://github.com/SagerNet/sing-box) does the tunneling. tun4ai is the glue around it: it renders the sing-box config and pf rules from two small text files, runs a root supervisor under launchd, and puts a status icon in the menu bar.

## Requirements

- macOS 12 or later, Apple silicon or Intel
- Xcode Command Line Tools (`xcode-select --install`), for `/usr/bin/python3`
- An administrator account (the installer asks for your password once)
- A VLESS Reality server, as a `vless://` share link
- [Rust](https://rustup.rs) only if you build the menu bar app from source

## Install

Download `tun4ai-macos.zip` from [Releases](https://github.com/tylertaho8/tun4ai/releases), unzip it and open `tun4ai.app`. It runs from any folder.

macOS blocks the first open because the app is not notarized. Right-click the app, choose **Open**, then **Open** again. Or run:

```bash
xattr -dr com.apple.quarantine tun4ai.app
```

Then click the menu bar icon and choose **Install** in the panel. The app downloads sing-box, checks its checksum and asks for your password once. Your network does not change.

1. **Edit VLESS node…**, paste your `vless://` link on its own line, save.
2. **Start tunnel**.
3. Once pages load, tick **Firewall**.

Choose **Reinstall** in the same menu to upgrade. Your node, direct list and firewall choice are kept. Building from source with `./install.sh` still works, and is described under Development.

## Configuration

Both files live in `~/Library/Application Support/tun4ai/` and are read on every start or restart.

**`vless.txt`**: exactly one share link.

```
vless://UUID@host:443?security=reality&pbk=PUBLIC_KEY&sid=SHORT_ID&sni=www.example.com&flow=xtls-rprx-vision
```

Only TCP VLESS Reality is supported. `host` may be an IP or a domain; a domain is resolved through your LAN DNS and the answer is cached.

**`direct.txt`**: one entry per line, `#` starts a comment. CIDRs bypass the tunnel at the routing level; domains bypass it in sing-box and resolve through your LAN DNS.

```
192.168.0.0/16
172.16.0.0/12
corp.example.com
```

After editing either file, choose **Restart tunnel**.

## Menu bar states

| Icon | Meaning |
|------|---------|
| Green | Tunnel up, firewall on |
| Yellow | Tunnel up but firewall off, or starting / stopping |
| Red | Tunnel should be up but isn't. Firewall on: internet blocked. Firewall off: traffic leaks |
| Gray | Tunnel off |

## Uninstall

```bash
./uninstall.sh           # keeps your node and direct list
./uninstall.sh --purge   # deletes them too
```

## If the network is stuck

```bash
sudo /Library/PrivilegedHelperTools/tun4ai/panic-open.sh
```

This stops the tunnel, turns off the firewall and restores the system pf rules. Rebooting does **not** help on its own, because the supervisor brings the firewall back on purpose.

## Security model

- Everything that runs as root lives in `/Library/PrivilegedHelperTools/tun4ai` and `/Library/Application Support/tun4ai`, both owned by root.
- The root helper only *reads* your two config files. It refuses symlinks, non-regular files, files owned by anyone but you, and files over 64 KB. It never executes anything from your home folder.
- `sudo` without a password is granted for exactly four scripts: start, stop, firewall on, firewall off. Nothing else.
- The rendered sing-box config, which contains your UUID, is root-only (`0600`). The pf rules never contain it.

See [SECURITY.md](SECURITY.md) to report a vulnerability.

## Limitations

- **App UDP is dropped**, including QUIC. Browsers fall back to TCP automatically; games and voice calls won't work through the tunnel.
- **IPv4 only.** IPv6 is not tunneled: with the firewall on it is blocked (except IPv6 ranges in the direct list); with the firewall off it goes direct.
- **One node, one protocol**: VLESS Reality over TCP. No subscriptions, no rule sets, no load balancing.
- **macOS only.**
- The TUN interface name `utun15` and the local proxy port `127.0.0.1:17897` are fixed.

## Development

```bash
/usr/bin/python3 -m unittest discover -s tests -v   # config and pf rendering
bash tests/test_install.sh                          # staged install/uninstall, no root needed
(cd tray && cargo test)                             # menu bar app
shellcheck -x install.sh uninstall.sh helper/*.sh scripts/*.sh tests/*.sh
```

`tests/test_install.sh` installs into a temporary `DESTDIR` and never touches launchd, pf or `/etc/sudoers.d`.

## License

[MIT](LICENSE). tun4ai downloads sing-box at install time and does not redistribute it; sing-box is licensed under GPL-3.0-or-later.

# tun4ai

English | [简体中文](README.zh-CN.md)

A whole-machine TUN tunnel with a kill switch for macOS, so AI tools that ignore proxy settings still go through your node and never leak out of the wrong exit.

Claude Code, Codex, Cursor, language servers, `git`, `npm`, Docker builds: many of them ignore `HTTP_PROXY`, open their own sockets, or spawn helpers that do. AI providers also tend to judge your account by the IP they see. tun4ai routes all TCP on the Mac through the one node you selected (VLESS, VMess, Trojan, Shadowsocks, Hysteria2 or TUIC), and its pf firewall blocks the internet when the tunnel is down instead of quietly falling back to your real IP.

## Features

- **Whole-machine TUN**: every app, every process, no per-app proxy settings.
- **Kill switch**: with the firewall on, only the tunnel and the selected node's server (its IPs, its port, its one protocol) can reach the internet. If the tunnel drops, traffic stops instead of leaking.
- **Several nodes, six protocols**: keep a list of share links and switch between them from the panel or the command line.
- **A command line an AI can drive**: `tun4ai node add`, `node use`, `direct add`, `doctor`, each with `--json` output, so Claude Code or Codex can set things up for you and check the result.
- **DNS that doesn't leak**: lookups are answered over DoH through the tunnel; pf sends queries aimed at your router's DNS into the tunnel too.
- **Direct list**: LAN ranges and domains you choose stay off the tunnel.
- **Menu bar panel**: click the icon for a frosted panel instead of a menu. Status, latency and leak results stay live while it is open. Chinese and English follow your system language.
- **Self-healing**: a root launchd supervisor restarts sing-box and rebuilds rules when Wi-Fi or Ethernet changes.
- **Off by default**: installing changes nothing about your network until you start the tunnel.

## How it works

```
apps ──► utun15 (sing-box TUN) ──► selected node ──► your server ──► internet
              │
              └── direct list ──► your router
pf kill switch: anything else leaving the Wi-Fi / Ethernet interface is blocked
```

[sing-box](https://github.com/SagerNet/sing-box) does the tunneling. tun4ai is the glue around it: it renders the sing-box config and pf rules from a few small text files, runs a root supervisor under launchd, and puts a status icon in the menu bar.

## Requirements

- macOS 12 or later, Apple silicon or Intel
- Xcode Command Line Tools (`xcode-select --install`), for `/usr/bin/python3`
- An administrator account (the installer asks for your password once)
- A proxy server and its share link: `vless://`, `vmess://`, `trojan://`, `ss://`, `hysteria2://` / `hy2://` or `tuic://`
- [Rust](https://rustup.rs) only if you build the menu bar app from source

## Install

Download `tun4ai-macos.zip` from [Releases](https://github.com/tylertaho8/tun4ai/releases), unzip it and open `tun4ai.app`. It runs from any folder.

macOS blocks the first open because the app is not notarized. Right-click the app, choose **Open**, then **Open** again. Or run:

```bash
xattr -dr com.apple.quarantine tun4ai.app
```

Then click the menu bar icon and choose **Install** in the panel. The app downloads sing-box, checks its checksum and asks for your password once. Your network does not change.

1. **Node**, paste your share link on its own line (one line per node), save. Or run `tun4ai node add '<link>'` (see [Command line](#command-line)).
2. **Start tunnel**.
3. Once pages load, tick **Firewall**.

Choose **Reinstall** in the same menu to upgrade. Your node, direct list and firewall choice are kept. Building from source with `./install.sh` still works, and is described under Development.

## Configuration

The files live in `~/Library/Application Support/tun4ai/` and are read on every start or restart.

**`nodes.txt`**: one share link per line, `#` starts a comment. The text after `#` at the end of a link is the node's name; names must be unique.

```
vless://UUID@host:443?security=reality&pbk=PUBLIC_KEY&sid=SHORT_ID&sni=www.example.com&flow=xtls-rprx-vision#tokyo
hysteria2://PASSWORD@hk.example.com:8443?sni=hk.example.com#hk-hy2
```

**`selected.txt`**: the name of the node the tunnel uses. Without it, the first node is used. If it names a node that no longer exists, the tunnel does not start (and with the firewall on, the internet stays blocked) rather than quietly moving you to another exit. The panel's **Nodes** tab and `tun4ai node use` write it for you.

| Link | Supported | Refused |
|------|-----------|---------|
| `vless://` | Reality or TLS; TCP (with `xtls-rprx-vision`), WebSocket, HTTPUpgrade, gRPC | no TLS (`security=none`) |
| `vmess://` | v2rayN base64 JSON; TCP, WebSocket, HTTPUpgrade, gRPC; with or without TLS | HTTP header obfuscation, non-JSON formats |
| `trojan://` | TLS; TCP, WebSocket, HTTPUpgrade, gRPC | no TLS |
| `ss://` | SIP002 and the older all-base64 form; AEAD and 2022 ciphers | stream ciphers, plugins |
| `hysteria2://`, `hy2://` | password, `sni`, `insecure`, salamander obfs | port hopping, `pinSHA256` |
| `tuic://` | v5 `UUID:PASSWORD`, congestion control, UDP relay mode, ALPN | |

`host` may be an IPv4 address or a domain; a domain is resolved through your LAN DNS and the answer is cached. IPv6 server addresses and ports 53, 3478, 5349 and 19302 are refused, because the firewall blocks them before the node's rule. A link that skips certificate checks (`insecure=1`, `allowInsecure=1`) is accepted and shown as `"insecure": true`.

The firewall opens exactly one hole: the selected node's IPs and port, over UDP for Hysteria2 and TUIC and TCP for everything else.

**`vless.txt`** (older installs): it is read as the node list for as long as there is no `nodes.txt`. The first `tun4ai node` edit renames it to `nodes.txt`; nothing in it is rewritten.

**`direct.txt`**: one entry per line, `#` starts a comment. CIDRs bypass the tunnel at the routing level; domains bypass it in sing-box and resolve through your LAN DNS.

```
192.168.0.0/16
172.16.0.0/12
corp.example.com
```

After editing a file by hand, choose **Reconnect**.

## Command line

`tun4ai` lives at `/Library/PrivilegedHelperTools/tun4ai/tun4ai` once installed. To call it by name:

```bash
mkdir -p ~/.local/bin && ln -s /Library/PrivilegedHelperTools/tun4ai/tun4ai ~/.local/bin/tun4ai
```

| Command | What it does |
|---------|--------------|
| `tun4ai node list` | Nodes, the selected one marked |
| `tun4ai node add '<link>' [--name NAME]` | Check a share link and append it |
| `tun4ai node remove <name or number> [--force]` | Remove a node; the selected one only with `--force` |
| `tun4ai node use <name or number>` | Select a node (a name wins over a number) |
| `tun4ai node test [name or number]` | TCP connect time to each server (QUIC nodes: not testable) |
| `tun4ai direct list` / `add <entry>` / `remove <entry>` | Edit the direct list |
| `tun4ai status` | Tunnel, firewall, selected and active node |
| `tun4ai doctor` | Checks with a verdict each; exits 1 if any fails |
| `tun4ai start` / `stop` / `restart` / `firewall on\|off` | The same four helper scripts the tray uses |

It runs as you and refuses `sudo`. It edits only `nodes.txt`, `selected.txt` and `direct.txt`, with the same checks the root helper applies; starting, stopping and the firewall go through the four scripts sudoers already allows. Its output never contains UUIDs or passwords, so it is safe to paste into an AI chat.

With `--json`, every command prints exactly one JSON object with `ok`, `schema` (currently `1`) and `command`, and exits `0` on success, `1` on failure and `2` on a usage error:

```console
$ tun4ai node use hk-hy2 --json
{"ok": true, "schema": 1, "command": "node use", "selected": {"index": 2, "name": "hk-hy2", "protocol": "hysteria2", "server": "hk.example.com", "port": 8443, "transport": "udp", "network": "quic", "security": "tls", "insecure": false, "selected": true}, "restart_needed": true}
$ tun4ai node add 'ss://YWVzLTI1Ni1nY206cGFzcw@198.51.100.2:53#x' --json
{"ok": false, "schema": 1, "command": "node add", "error": {"code": "invalid_link", "message": "port 53 is always blocked by the firewall and cannot be used for a node"}}
```

`restart_needed` is true when the running tunnel was built from a different node; run `tun4ai restart`. Error codes: `usage`, `invalid_link`, `invalid_name`, `invalid_node_file`, `invalid_entry`, `duplicate_name`, `not_found`, `selected_node`, `config_unreadable`, `file_too_large`, `not_installed`, `sudo_refused`, `action_failed`, `root`.

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
- The root helper only *reads* your config files (`nodes.txt` or `vless.txt`, `selected.txt`, `direct.txt`). It refuses symlinks, non-regular files, files owned by anyone but you, and files over 64 KB. It never executes anything from your home folder.
- `sudo` without a password is granted for exactly four scripts: start, stop, firewall on, firewall off. Nothing else.
- The rendered sing-box config, which contains your credentials, is root-only (`0600`). The pf rules and `active-node.json` never contain them; rendering stops if any credential would end up there.
- The `tun4ai` command line is root-owned but runs as you, is not in sudoers, and never runs as root.

See [SECURITY.md](SECURITY.md) to report a vulnerability.

## Limitations

- **App UDP is dropped**, including QUIC. Browsers fall back to TCP automatically; games and voice calls won't work through the tunnel.
- **IPv4 only.** IPv6 is not tunneled: with the firewall on it is blocked (except IPv6 ranges in the direct list); with the firewall off it goes direct.
- **One node at a time.** You can keep many and switch, but there is no automatic failover, load balancing or rule-based routing.
- **No subscriptions yet.** Add links one by one (`tun4ai node add`).
- Not supported: Shadowsocks plugins, Hysteria2 port hopping and certificate pinning, VMess HTTP obfuscation, plaintext VLESS, IPv6 server addresses. See the table under Configuration.
- `tun4ai node test` measures a TCP connect, not a working proxy, and cannot test QUIC nodes. With the tunnel and firewall up, other nodes are reached through the current one or not at all.
- **macOS only.**
- The TUN interface name `utun15` and the local proxy port `127.0.0.1:17897` are fixed.

## Development

```bash
make check
```

That runs shellcheck, the Python tests (link parsing, CLI, sing-box config and pf rules, with a pinned sing-box so `sing-box check` validates every protocol's config), the staged install test, and the menu bar app's tests. CI runs the same target.

The kill switch is tested with `tests/pfsim.py`, which evaluates the generated rules the way pf does and asks which packets get out, rather than matching rule text. On a Mac, `pfctl -nf` also parses every generated rule set.

On Linux, `make check` runs everything except what needs macOS (pfctl, `route`/`ipconfig`, launchd, swiftc): those tests skip, the tray's platform-independent modules are tested through `tests/tray_portable.sh`, and the whole tray crate is type-checked with `cargo check --target x86_64-apple-darwin` (`rustup target add x86_64-apple-darwin` first).

`tests/test_install.sh` installs into a temporary `DESTDIR` and never touches launchd, pf or `/etc/sudoers.d`.

## License

[MIT](LICENSE). tun4ai downloads sing-box at install time and does not redistribute it; sing-box is licensed under GPL-3.0-or-later.

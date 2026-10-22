# tun4ai

[English](README.md) | 简体中文

macOS 全局 TUN 隧道 + 防火墙断网保护。不认代理设置的 AI 工具也会走你的节点，并且不会从错误的出口漏出去。

Claude Code、Codex、Cursor、各种语言服务器、`git`、`npm`、Docker 构建，很多都不读 `HTTP_PROXY`，或者自己开 socket、再拉起不认代理的子进程。AI 服务商又常按你出现的 IP 判断账号。tun4ai 把整台 Mac 的 TCP 都送进你选中的那个节点（VLESS、VMess、Trojan、Shadowsocks、Hysteria2 或 TUIC）；打开防火墙后，隧道一断就直接断网，而不是悄悄换回你的真实 IP。

## 功能

- **全局 TUN**：所有应用、所有进程，不用逐个配代理。
- **断网保护**：防火墙打开时，只有隧道和当前节点的服务器（它的 IP、端口和唯一的协议）能上外网。隧道掉了就断网，不会漏。
- **多节点、六种协议**：保存多条分享链接，在面板或命令行里切换。
- **AI 能直接操作的命令行**：`tun4ai node add`、`node use`、`direct add`、`doctor`，都支持 `--json` 输出，Claude Code、Codex 可以替你配置并自己验证结果。
- **DNS 不泄漏**：解析由隧道里的 DoH 应答；发往路由器 DNS 的查询也会被 pf 转进隧道。
- **直连名单**：局域网网段和你指定的域名不走隧道。
- **菜单栏面板**：点图标弹出毛玻璃面板，不是系统菜单。打开期间，状态、延迟和泄露结果会一直刷新。中英文跟随系统语言。
- **自动恢复**：root 权限的 launchd 守护进程会在 sing-box 退出或 Wi-Fi / 有线切换时自动重建。
- **默认关闭**：装完不会改变任何网络行为，直到你手动开启隧道。

## 原理

```
应用 ──► utun15（sing-box TUN）──► 选中的节点 ──► 你的服务器 ──► 外网
              │
              └── 直连名单 ──► 你的路由器
pf 防火墙：其余从 Wi-Fi / 有线网卡出去的流量一律拦截
```

隧道本身由 [sing-box](https://github.com/SagerNet/sing-box) 实现。tun4ai 负责外围：从几个小文本文件生成 sing-box 配置和 pf 规则，用 launchd 跑一个 root 守护进程，并在菜单栏显示状态。

## 环境要求

- macOS 12 或更高，Apple 芯片或 Intel 均可
- Xcode 命令行工具（`xcode-select --install`），用于 `/usr/bin/python3`
- 管理员账号（安装时输入一次密码）
- 一台代理服务器和它的分享链接：`vless://`、`vmess://`、`trojan://`、`ss://`、`hysteria2://` / `hy2://` 或 `tuic://`
- 只有从源码构建菜单栏程序时才需要 [Rust](https://rustup.rs)

## 安装

从 [Releases](https://github.com/tylertaho8/tun4ai/releases) 下载 `tun4ai-macos.zip`，解压后打开 `tun4ai.app`。放在哪个文件夹都能用。

应用没有经过苹果公证，首次打开会被拦截。右键点 **打开**，再点一次 **打开**。或者运行：

```bash
xattr -dr com.apple.quarantine tun4ai.app
```

然后点菜单栏图标，在弹出的面板里选 **安装**。程序会自己下载 sing-box、校验校验和，并询问一次管理员密码。这时网络还没有任何变化。

1. **节点管理**，把分享链接单独粘成一行（一个节点一行），保存。也可以运行 `tun4ai node add '<链接>'`（见[命令行](#命令行)）。
2. **开启隧道**。
3. 确认网页能打开后，再勾选 **防火墙**。

升级时在同一菜单里选 **重新安装**。节点、直连名单和防火墙选择都会保留。从源码用 `./install.sh` 安装仍然可以，写在「开发」一节。

## 配置

文件都在 `~/Library/Application Support/tun4ai/`，每次开启或重启隧道时读取。

**`nodes.txt`**：一行一个分享链接，`#` 开头是注释。链接末尾 `#` 后面的文字是节点名，名字不能重复。

```
vless://UUID@host:443?security=reality&pbk=PUBLIC_KEY&sid=SHORT_ID&sni=www.example.com&flow=xtls-rprx-vision#tokyo
hysteria2://PASSWORD@hk.example.com:8443?sni=hk.example.com#hk-hy2
```

**`selected.txt`**：隧道使用的节点名。没有这个文件就用第一个节点。如果写的节点已经不存在，隧道不会启动（防火墙开着时外网保持切断），而不是悄悄换到别的出口。面板的 **节点** 标签页和 `tun4ai node use` 会替你写好。

| 链接 | 支持 | 拒绝 |
|------|------|------|
| `vless://` | Reality 或 TLS；TCP（可配 `xtls-rprx-vision`）、WebSocket、HTTPUpgrade、gRPC | 不加密（`security=none`） |
| `vmess://` | v2rayN 的 base64 JSON；TCP、WebSocket、HTTPUpgrade、gRPC；有无 TLS 均可 | HTTP 伪装、非 JSON 格式 |
| `trojan://` | TLS；TCP、WebSocket、HTTPUpgrade、gRPC | 不带 TLS |
| `ss://` | SIP002 和旧的整段 base64 格式；AEAD 和 2022 加密 | 流加密、插件 |
| `hysteria2://`、`hy2://` | 密码、`sni`、`insecure`、salamander 混淆 | 端口跳跃、`pinSHA256` |
| `tuic://` | v5 `UUID:密码`、拥塞控制、UDP 转发模式、ALPN | |

`host` 可以是 IPv4 地址或域名；域名用局域网 DNS 解析，结果会缓存。IPv6 服务器地址，以及 53、3478、5349、19302 端口会被拒绝，因为防火墙在节点规则之前就会拦掉它们。跳过证书校验的链接（`insecure=1`、`allowInsecure=1`）可以用，但会显示 `"insecure": true`。

防火墙只开一个口子：当前节点的 IP 和端口，Hysteria2 和 TUIC 走 UDP，其余走 TCP。

**`vless.txt`**（旧版安装）：只要还没有 `nodes.txt`，它就被当作节点列表读取。第一次用 `tun4ai node` 修改节点时，它会被改名为 `nodes.txt`，内容一字不改。

**`direct.txt`**：每行一条，`#` 开头是注释。CIDR 在路由层绕开隧道；域名在 sing-box 里直连，并用局域网 DNS 解析。

```
192.168.0.0/16
172.16.0.0/12
corp.example.com
```

手动改完文件后，选 **重连隧道**。

## 命令行

装好后，`tun4ai` 在 `/Library/PrivilegedHelperTools/tun4ai/tun4ai`。想直接用名字调用：

```bash
mkdir -p ~/.local/bin && ln -s /Library/PrivilegedHelperTools/tun4ai/tun4ai ~/.local/bin/tun4ai
```

| 命令 | 作用 |
|------|------|
| `tun4ai node list` | 列出节点，标出当前选中的 |
| `tun4ai node add '<链接>' [--name 名字]` | 校验分享链接并追加 |
| `tun4ai node remove <名字或序号> [--force]` | 删除节点；删当前选中的要加 `--force` |
| `tun4ai node use <名字或序号>` | 选中节点（名字优先于序号） |
| `tun4ai node test [名字或序号]` | 测到各服务器的 TCP 连接耗时（QUIC 节点测不了） |
| `tun4ai direct list` / `add <条目>` / `remove <条目>` | 修改直连名单 |
| `tun4ai status` | 隧道、防火墙、选中节点和正在用的节点 |
| `tun4ai doctor` | 逐项检查并给出结论；有任何一项失败就返回 1 |
| `tun4ai start` / `stop` / `restart` / `firewall on\|off` | 和托盘调用的是同样四个辅助脚本 |

它以你的身份运行，拒绝 `sudo`。它只改 `nodes.txt`、`selected.txt` 和 `direct.txt`，检查规则和 root 辅助脚本一样；开关隧道和防火墙走 sudoers 里已有的四个脚本。输出里不会出现 UUID 或密码，可以放心贴给 AI。

加 `--json` 时，每条命令只输出一个 JSON 对象，带 `ok`、`schema`（目前是 `1`）和 `command`；成功返回 `0`，失败返回 `1`，用法错误返回 `2`。示例和错误码见[英文文档](README.md#command-line)。`restart_needed` 为 true 表示正在运行的隧道还是用别的节点建的，运行 `tun4ai restart` 生效。

## 菜单栏状态

| 图标 | 含义 |
|------|------|
| 绿色 | 隧道已连接，防火墙开 |
| 黄色 | 隧道已连接但防火墙关；或正在开启 / 关闭 |
| 红色 | 隧道应该开着但断了。防火墙开：外网已切断；防火墙关：流量在漏 |
| 灰色 | 隧道关 |

## 卸载

```bash
./uninstall.sh           # 保留节点和直连名单
./uninstall.sh --purge   # 一并删除
```

## 网络卡死时

```bash
sudo /Library/PrivilegedHelperTools/tun4ai/panic-open.sh
```

它会关掉隧道和防火墙，并恢复系统自带的 pf 规则。单纯重启电脑**没用**，因为守护进程会按设计把防火墙重新打开。

## 安全模型

- 所有以 root 运行的东西都在 `/Library/PrivilegedHelperTools/tun4ai` 和 `/Library/Application Support/tun4ai`，属主都是 root。
- root 辅助脚本只**读取**你的配置文件（`nodes.txt` 或 `vless.txt`、`selected.txt`、`direct.txt`）：拒绝符号链接、非普通文件、不属于你的文件和超过 64 KB 的文件，也从不执行你家目录里的任何东西。
- 免密 `sudo` 只授权四个脚本：开启、关闭、防火墙开、防火墙关，没有别的。
- 生成的 sing-box 配置里有你的凭据，只有 root 可读（`0600`）；pf 规则和 `active-node.json` 里没有凭据，一旦会写进去就停止生成。
- `tun4ai` 命令行属主是 root，但以你的身份运行，不在 sudoers 里，也不会以 root 运行。

发现漏洞请看 [SECURITY.md](SECURITY.md)。

## 限制

- **应用的 UDP 会被丢弃**，包括 QUIC。浏览器会自动退回 TCP；游戏和语音通话无法走隧道。
- **只支持 IPv4。** IPv6 不进隧道：防火墙开时被拦截（直连名单里的 IPv6 网段除外）；防火墙关时直连。
- **同一时间只用一个节点。** 可以存很多个并切换，但没有自动故障切换、负载均衡和分流规则。
- **暂不支持订阅。** 节点要一条条添加（`tun4ai node add`）。
- 不支持：Shadowsocks 插件、Hysteria2 端口跳跃和证书固定、VMess HTTP 伪装、不加密的 VLESS、IPv6 服务器地址。详见「配置」里的表格。
- `tun4ai node test` 测的是 TCP 连接，不代表代理可用，也测不了 QUIC 节点。隧道和防火墙开着时，其他节点要经过当前节点才能连到，或者根本连不到。
- **只支持 macOS。**
- TUN 网卡名 `utun15` 和本地代理端口 `127.0.0.1:17897` 是固定的。

## 开发

```bash
make check
```

它会运行 shellcheck、Python 测试（链接解析、命令行、sing-box 配置和 pf 规则；会下载固定版本的 sing-box，用 `sing-box check` 校验每种协议的配置）、安装演练和菜单栏程序的测试。CI 跑的是同一个目标。

断网保护用 `tests/pfsim.py` 测试：它按 pf 的规则求值生成的规则集，检查哪些包能出去，而不是比对规则文本。在 Mac 上，`pfctl -nf` 还会解析每一套生成的规则。

在 Linux 上，`make check` 会跳过需要 macOS 的部分（pfctl、`route`/`ipconfig`、launchd、swiftc）：菜单栏程序里与平台无关的模块用 `tests/tray_portable.sh` 测试，整个 crate 用 `cargo check --target x86_64-apple-darwin` 做类型检查（先运行 `rustup target add x86_64-apple-darwin`）。

`tests/test_install.sh` 安装到临时 `DESTDIR`，不会碰 launchd、pf 或 `/etc/sudoers.d`。

## 许可证

[MIT](LICENSE)。tun4ai 在安装时下载 sing-box，不重新分发它；sing-box 采用 GPL-3.0-or-later 许可。

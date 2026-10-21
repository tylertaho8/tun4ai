# tun4ai

[English](README.md) | 简体中文

macOS 全局 TUN 隧道 + 防火墙断网保护。不认代理设置的 AI 工具也会走你的节点，并且不会从错误的出口漏出去。

Claude Code、Codex、Cursor、各种语言服务器、`git`、`npm`、Docker 构建，很多都不读 `HTTP_PROXY`，或者自己开 socket、再拉起不认代理的子进程。AI 服务商又常按你出现的 IP 判断账号。tun4ai 把整台 Mac 的 TCP 都送进一个 VLESS Reality 节点；打开防火墙后，隧道一断就直接断网，而不是悄悄换回你的真实 IP。

## 功能

- **全局 TUN**：所有应用、所有进程，不用逐个配代理。
- **断网保护**：防火墙打开时，只有隧道和节点服务器能上外网。隧道掉了就断网，不会漏。
- **DNS 不泄漏**：解析由隧道里的 DoH 应答；发往路由器 DNS 的查询也会被 pf 转进隧道。
- **直连名单**：局域网网段和你指定的域名不走隧道。
- **菜单栏面板**：点图标弹出毛玻璃面板，不是系统菜单。打开期间，状态、延迟和泄露结果会一直刷新。中英文跟随系统语言。
- **自动恢复**：root 权限的 launchd 守护进程会在 sing-box 退出或 Wi-Fi / 有线切换时自动重建。
- **默认关闭**：装完不会改变任何网络行为，直到你手动开启隧道。

## 原理

```
应用 ──► utun15（sing-box TUN）──► VLESS Reality ──► 你的服务器 ──► 外网
              │
              └── 直连名单 ──► 你的路由器
pf 防火墙：其余从 Wi-Fi / 有线网卡出去的流量一律拦截
```

隧道本身由 [sing-box](https://github.com/SagerNet/sing-box) 实现。tun4ai 负责外围：从两个小文本文件生成 sing-box 配置和 pf 规则，用 launchd 跑一个 root 守护进程，并在菜单栏显示状态。

## 环境要求

- macOS 12 或更高，Apple 芯片或 Intel 均可
- Xcode 命令行工具（`xcode-select --install`），用于 `/usr/bin/python3`
- 管理员账号（安装时输入一次密码）
- 一台 VLESS Reality 服务器，以及它的 `vless://` 分享链接
- 只有从源码构建菜单栏程序时才需要 [Rust](https://rustup.rs)

## 安装

从 [Releases](https://github.com/tylertaho8/tun4ai/releases) 下载 `tun4ai-macos.zip`，解压后打开 `tun4ai.app`。放在哪个文件夹都能用。

应用没有经过苹果公证，首次打开会被拦截。右键点 **打开**，再点一次 **打开**。或者运行：

```bash
xattr -dr com.apple.quarantine tun4ai.app
```

然后点菜单栏图标，在弹出的面板里选 **安装**。程序会自己下载 sing-box、校验校验和，并询问一次管理员密码。这时网络还没有任何变化。

1. **编辑 VLESS 节点…**，把 `vless://` 链接单独粘成一行，保存。
2. **开启隧道**。
3. 确认网页能打开后，再勾选 **防火墙**。

升级时在同一菜单里选 **重新安装**。节点、直连名单和防火墙选择都会保留。从源码用 `./install.sh` 安装仍然可以，写在「开发」一节。

## 配置

两个文件都在 `~/Library/Application Support/tun4ai/`，每次开启或重启隧道时读取。

**`vless.txt`**：有且只有一行分享链接。

```
vless://UUID@host:443?security=reality&pbk=PUBLIC_KEY&sid=SHORT_ID&sni=www.example.com&flow=xtls-rprx-vision
```

只支持 TCP 的 VLESS Reality。`host` 可以是 IP 或域名；域名用局域网 DNS 解析，结果会缓存。

**`direct.txt`**：每行一条，`#` 开头是注释。CIDR 在路由层绕开隧道；域名在 sing-box 里直连，并用局域网 DNS 解析。

```
192.168.0.0/16
172.16.0.0/12
corp.example.com
```

改完任一文件后，选 **重启隧道**。

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
- root 辅助脚本只**读取**你的两个配置文件：拒绝符号链接、非普通文件、不属于你的文件和超过 64 KB 的文件，也从不执行你家目录里的任何东西。
- 免密 `sudo` 只授权四个脚本：开启、关闭、防火墙开、防火墙关，没有别的。
- 生成的 sing-box 配置里有你的 UUID，只有 root 可读（`0600`）；pf 规则里不含 UUID。

发现漏洞请看 [SECURITY.md](SECURITY.md)。

## 限制

- **应用的 UDP 会被丢弃**，包括 QUIC。浏览器会自动退回 TCP；游戏和语音通话无法走隧道。
- **只支持 IPv4。** IPv6 不进隧道：防火墙开时被拦截（直连名单里的 IPv6 网段除外）；防火墙关时直连。
- **一个节点、一种协议**：TCP 上的 VLESS Reality。没有订阅、规则集和负载均衡。
- **只支持 macOS。**
- TUN 网卡名 `utun15` 和本地代理端口 `127.0.0.1:17897` 是固定的。

## 开发

```bash
/usr/bin/python3 -m unittest discover -s tests -v   # 配置和 pf 规则生成
bash tests/test_install.sh                          # 安装/卸载演练，无需 root
(cd tray && cargo test)                             # 菜单栏程序
shellcheck -x install.sh uninstall.sh helper/*.sh scripts/*.sh tests/*.sh
```

`tests/test_install.sh` 安装到临时 `DESTDIR`，不会碰 launchd、pf 或 `/etc/sudoers.d`。

## 许可证

[MIT](LICENSE)。tun4ai 在安装时下载 sing-box，不重新分发它；sing-box 采用 GPL-3.0-or-later 许可。

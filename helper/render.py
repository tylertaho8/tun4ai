#!/usr/bin/env python3
"""Write the sing-box config and pf rules. Does not start anything.

Runs as root from the supervisor. The node and direct list come from the owner's
config directory and are only parsed, never executed.
"""

import argparse
import ipaddress
import json
import os
import pwd
import re
import stat
import subprocess
import sys
from pathlib import Path
from urllib.parse import parse_qs, unquote, urlparse

TUN_IF = "utun15"
TUN_NET = ipaddress.ip_network("192.168.165.0/24")
TUN_ADDR = "192.168.165.1/24"
TUN_DNS = "192.168.165.2"
MIXED_PORT = 17897
MAX_USER_FILE = 64 * 1024
LANG = "zh" if os.environ.get("TUN4AI_LANG", "").startswith("zh") else "en"


def t(zh, en):
    return zh if LANG == "zh" else en


# The config directory belongs to the user. A symlink there must not let root read
# some other file, so only plain files owned by the expected user are accepted.
def read_user_file(path: Path, owner_uid=None):
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    except FileNotFoundError:
        return None
    except OSError as err:
        sys.exit(t(f"{path.name} 打不开：{err.strerror}", f"cannot open {path.name}: {err.strerror}"))
    with os.fdopen(fd, "rb") as handle:
        info = os.fstat(handle.fileno())
        if not stat.S_ISREG(info.st_mode):
            sys.exit(t(f"{path.name} 不是普通文件", f"{path.name} is not a regular file"))
        if owner_uid is not None and info.st_uid not in (owner_uid, 0):
            sys.exit(t(f"{path.name} 的属主不对", f"{path.name} has the wrong owner"))
        if info.st_size > MAX_USER_FILE:
            sys.exit(t(f"{path.name} 太大", f"{path.name} is too large"))
        return handle.read().decode("utf-8", errors="replace")


def load_node(path: Path, owner_uid=None):
    text = read_user_file(path, owner_uid)
    if text is None:
        sys.exit(t(f"缺少 {path.name}", f"{path.name} is missing"))
    urls = [l.strip() for l in text.splitlines() if l.strip() and not l.strip().startswith("#")]
    if len(urls) != 1:
        sys.exit(t(f"{path.name} 里要有且只有一行节点", f"{path.name} must contain exactly one node line"))
    try:
        parsed = urlparse(urls[0])
        port = parsed.port or 443
    except ValueError:
        sys.exit(t(f"{path.name} 里的节点地址格式不对", f"the node address in {path.name} is malformed"))
    host = (parsed.hostname or "").lower()
    query = parse_qs(parsed.query)

    def one(name):
        values = query.get(name) or []
        return unquote(values[0]) if values else ""

    sni = (one("sni") or host).lower()
    if parsed.scheme != "vless" or not host or not parsed.username or not one("pbk") or not one("sid"):
        sys.exit(t(
            f"{path.name} 要写成 vless://UUID@地址:端口?pbk=公钥&sid=ShortID&sni=域名",
            f"{path.name} must look like vless://UUID@host:port?pbk=KEY&sid=SHORTID&sni=DOMAIN",
        ))
    try:
        ipaddress.IPv4Address(host)
        address_is_ip = True
    except ipaddress.AddressValueError:
        address_is_ip = False
        if not re.fullmatch(r"[a-z0-9.-]+\.[a-z0-9-]+", host):
            sys.exit(t(f"{path.name} 里的服务器地址不对", f"the server address in {path.name} is invalid"))
    return {
        "host": host,
        "sni": sni,
        "port": port,
        "uuid": unquote(parsed.username),
        "public_key": one("pbk"),
        "short_id": one("sid"),
        "flow": one("flow") or "xtls-rprx-vision",
        "address_is_ip": address_is_ip,
    }


def load_direct(path: Path, owner_uid=None):
    text = read_user_file(path, owner_uid)
    nets = []
    domains = []
    for lineno, raw in enumerate((text or "").splitlines(), 1):
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        try:
            nets.append(ipaddress.ip_network(line, strict=False))
            continue
        except ValueError:
            pass
        domain = line.strip(".").lower()
        if re.fullmatch(r"[a-z0-9.-]+\.[a-z0-9.-]+", domain):
            domains.append(domain)
            continue
        sys.exit(t(
            f"{path.name}:{lineno}: 不是 IP、网段或域名: {line}",
            f"{path.name}:{lineno}: not an IP, network or domain: {line}",
        ))
    return nets, domains


def carve_tun(nets):
    carved = []
    for net in nets:
        if net.version == 4 and TUN_NET.subnet_of(net):
            carved.extend(net.address_exclude(TUN_NET))
        elif net != TUN_NET:
            carved.append(net)
    return carved


def default_gateway():
    out = subprocess.check_output(["route", "-n", "get", "default"], text=True)
    for line in out.splitlines():
        if "gateway:" in line:
            return line.split()[-1]
    sys.exit(t("找不到局域网网关", "no LAN gateway found"))


# Many networks hand out a DNS server that is not the gateway.
def lan_dns_for(iface):
    run = subprocess.run(["ipconfig", "getoption", iface, "domain_name_server"],
                         capture_output=True, text=True)
    value = run.stdout.strip()
    try:
        ipaddress.IPv4Address(value)
        return value
    except ipaddress.AddressValueError:
        return default_gateway()


# On a new network the old kill switch still blocks its DNS, so a lookup can fail
# until the new rules are loaded. The last good answer keeps the tunnel startable.
def resolve_proxy(lan_dns, host, run_dir: Path):
    cache = run_dir / f"proxy-ips.{host}"
    run = subprocess.run(
        ["dig", "+time=2", "+tries=1", "+short", f"@{lan_dns}", "A", host],
        capture_output=True, text=True,
    )
    ips = []
    for line in run.stdout.splitlines():
        line = line.strip().rstrip(".")
        try:
            ipaddress.IPv4Address(line)
        except ipaddress.AddressValueError:
            continue
        ips.append(line)
    if ips:
        write_atomic(cache, "\n".join(ips) + "\n", 0o644)
        return ips, t("刚解析", "resolved")
    if cache.exists():
        cached = [line.strip() for line in cache.read_text().splitlines() if line.strip()]
        if cached:
            return cached, t("缓存", "cached")
    sys.exit(t(f"DNS {lan_dns} 查不到 {host}，也没有上次的缓存",
               f"DNS {lan_dns} cannot resolve {host} and there is no cached answer"))


def config(node, direct, domains, lan_dns, iface, proxy_ips, with_tun, mixed_port=MIXED_PORT):
    routed = carve_tun(direct)
    exclude = ["127.0.0.0/8"] + [f"{ip}/32" for ip in proxy_ips] + [str(net) for net in routed]
    rules = [
        {"action": "sniff"},
        {"protocol": "dns", "action": "hijack-dns"},
        {"domain_suffix": ["local", "lan"], "action": "reject", "method": "drop"},
        {"port": [3478, 5349, 19302], "action": "reject", "method": "drop"},
        {"network": "udp", "action": "reject", "method": "drop"},
    ]
    if domains:
        rules.append({
            "domain_suffix": domains,
            "action": "route",
            "outbound": "direct",
        })
    if routed:
        rules.append({
            "ip_cidr": [str(net) for net in routed],
            "action": "route",
            "outbound": "direct",
        })
    inbounds = [{
        "type": "mixed",
        "tag": "mixed-in",
        "listen": "127.0.0.1",
        "listen_port": mixed_port,
    }]
    if with_tun:
        inbounds.insert(0, {
            "type": "tun",
            "tag": "tun-in",
            "interface_name": TUN_IF,
            "address": [TUN_ADDR],
            "mtu": 1500,
            "stack": "gvisor",
            "auto_route": True,
            "strict_route": False,
            "dns_mode": "hijack",
            "dns_address": [TUN_DNS],
            "route_exclude_address": exclude,
        })
    return {
        "log": {"level": "error", "timestamp": True},
        "dns": {
            "servers": [{
                "type": "udp",
                "tag": "lan",
                "server": lan_dns,
                "server_port": 53,
                "detour": "direct",
            }, {
                "type": "https",
                "tag": "remote",
                "server": "1.1.1.1",
                "server_port": 443,
                "path": "/dns-query",
                "tls": {"enabled": True, "server_name": "1.1.1.1"},
                "detour": "vless-out",
            }],
            "rules": ([{"domain_suffix": domains, "server": "lan"}] if domains else []),
            "final": "remote",
            "strategy": "ipv4_only",
        },
        "inbounds": inbounds,
        "outbounds": [{
            "type": "vless",
            "tag": "vless-out",
            "server": proxy_ips[0],
            "server_port": node["port"],
            "uuid": node["uuid"],
            "flow": node["flow"],
            "bind_interface": iface,
            "tls": {
                "enabled": True,
                "server_name": node["sni"],
                "utls": {"enabled": True, "fingerprint": "chrome"},
                "reality": {
                    "enabled": True,
                    "public_key": node["public_key"],
                    "short_id": node["short_id"],
                },
            },
        }, {
            "type": "direct",
            "tag": "direct",
            "bind_interface": iface,
        }],
        "route": {
            "auto_detect_interface": True,
            "default_domain_resolver": {"server": "remote", "strategy": "ipv4_only"},
            "rules": rules,
            "final": "vless-out",
        },
    }


# macOS sends DNS to the LAN server, and LAN traffic does not enter the TUN.
# pf hands those queries to the TUN instead, so system settings stay untouched.
# sing-box runs as root and must still reach the LAN server itself.
def dns_redirect(lan_dns):
    return f"""pass out quick on $ext_if route-to ($tun_if {TUN_DNS}) inet proto udp to {lan_dns} port 53 user != 0 keep state
pass out quick on $ext_if route-to ($tun_if {TUN_DNS}) inet proto tcp to {lan_dns} port 53 user != 0 flags any keep state"""


# Loaded instead of the kill switch when the firewall is off but the tunnel is up.
def pf_dns_text(iface, lan_dns):
    return f"""# Generated by render.py. DNS redirect only; everything else passes.
ext_if = "{iface}"
tun_if = "{TUN_IF}"

set skip on lo0

{dns_redirect(lan_dns)}
block drop out quick on $ext_if inet6 proto {{ tcp, udp }} to any port 53
"""


def proxy_pass(proxy_port):
    return f"pass out quick inet proto tcp to <proxy4> port {proxy_port} flags any keep state"


def pf_text(iface, direct, lan_dns, proxy_ips, proxy_port):
    v4 = [str(net) for net in direct if net.version == 4]
    v6 = [str(net) for net in direct if net.version == 6]
    v4_table = "table <direct4> persist { " + ", ".join(v4) + " }" if v4 else ""
    v6_table = "table <direct6> persist { " + ", ".join(v6) + " }" if v6 else ""
    v4_rules = ""
    if v4:
        v4_rules = """
pass out quick inet proto tcp to <direct4> flags any keep state
pass in quick inet proto tcp from <direct4> to any flags any keep state
pass out quick inet proto icmp to <direct4> keep state
pass in quick inet proto icmp from <direct4> to any keep state
""".strip("\n")
    v6_rules = ""
    if v6:
        v6_rules = """
pass out quick inet6 proto tcp to <direct6> flags any keep state
pass in quick inet6 proto tcp from <direct6> to any flags any keep state
pass out quick inet6 proto ipv6-icmp to <direct6> keep state
pass in quick inet6 proto ipv6-icmp from <direct6> to any keep state
""".strip("\n")
    return f"""# Generated by render.py. Not loaded here.
# One sing-box process owns both TUN and the VLESS outbound.
# macOS pf cannot tie this hole to a program path, so it is only TCP to the server.
ext_if = "{iface}"
tun_if = "{TUN_IF}"
lan_dns = "{lan_dns}"

set block-policy drop
set skip on lo0

table <proxy4> persist {{ {", ".join(proxy_ips)} }}
{v4_table}
{v6_table}

pass quick on $tun_if all
{dns_redirect(lan_dns)}
pass out quick inet proto udp to $lan_dns port 53 keep state
pass out quick inet proto tcp to $lan_dns port 53 keep state
block drop out quick proto tcp to any port 53
block drop out quick proto udp to any port 53
block drop in quick proto tcp from any port 53
block drop in quick proto udp from any port 53
block drop out quick proto tcp to any port {{ 3478, 5349, 19302 }}
block drop out quick proto udp to any port {{ 3478, 5349, 19302 }}
pass out quick inet proto udp from any port 68 to any port 67 keep state
pass in quick inet proto udp from any port 67 to any port 68 keep state
{proxy_pass(proxy_port)}
pass out quick inet proto icmp to <proxy4> icmp-type echoreq keep state
pass out quick inet proto icmp icmp-type {{ unreach, timex, paramprob }} keep state
{v4_rules}
{v6_rules}

block drop out quick all
block drop in quick all
"""


# The supervisor reloads pf when the file changes. A half-written file must never be visible.
def write_atomic(path: Path, text: str, mode: int):
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(text)
    os.chmod(tmp, mode)
    os.replace(tmp, path)


def write_json(path: Path, payload):
    write_atomic(path, json.dumps(payload, indent=2) + "\n", 0o600)


def parse_args(argv):
    parser = argparse.ArgumentParser(description="Render tun4ai sing-box config and pf rules.")
    parser.add_argument("--if", dest="iface", required=True, help="physical interface, e.g. en0")
    parser.add_argument("--config-dir", required=True, type=Path, help="holds vless.txt and direct.txt")
    parser.add_argument("--state-dir", required=True, type=Path, help="where generated files go")
    parser.add_argument("--owner", help="user who must own the config files")
    return parser.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    iface = args.iface
    if not re.fullmatch(r"[A-Za-z0-9]+", iface):
        sys.exit(t(f"网卡名不对：{iface}", f"invalid interface name: {iface}"))
    owner_uid = pwd.getpwnam(args.owner).pw_uid if args.owner else None
    node = load_node(args.config_dir / "vless.txt", owner_uid)
    direct, domains = load_direct(args.config_dir / "direct.txt", owner_uid)
    state = args.state_dir
    run_dir = state / "run"
    run_dir.mkdir(parents=True, exist_ok=True)
    lan_dns = lan_dns_for(iface)
    if node["address_is_ip"]:
        proxy_ips, source = [node["host"]], t("地址", "literal")
    else:
        proxy_ips, source = resolve_proxy(lan_dns, node["host"], run_dir)
    killswitch = pf_text(iface, direct, lan_dns, proxy_ips, node["port"])
    dns_only = pf_dns_text(iface, lan_dns)
    if node["uuid"] in killswitch or node["uuid"] in dns_only:
        sys.exit(t("节点标识出现在了防火墙规则里，没有写入配置", "the node UUID leaked into pf rules; nothing written"))
    write_json(state / "config.json", config(node, direct, domains, lan_dns, iface, proxy_ips, True))
    write_atomic(state / "pf.killswitch.conf", killswitch, 0o644)
    write_atomic(state / "pf.dns.conf", dns_only, 0o644)
    print(t(
        f"网卡 {iface}，局域网 DNS {lan_dns}，节点 {node['sni']}:{node['port']}，"
        f"服务器 IP {','.join(proxy_ips)}（{source}），直连 {len(direct)} 段 / {len(domains)} 个域名",
        f"interface {iface}, LAN DNS {lan_dns}, node {node['sni']}:{node['port']}, "
        f"server IP {','.join(proxy_ips)} ({source}), direct {len(direct)} networks / {len(domains)} domains",
    ))


if __name__ == "__main__":
    try:
        main()
    except SystemExit:
        raise
    except Exception as err:
        # The supervisor logs only the last line, so keep it to one.
        sys.exit(t(f"生成配置出错：{type(err).__name__}: {err}", f"render failed: {type(err).__name__}: {err}"))

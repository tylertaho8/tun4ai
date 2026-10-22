"""Share links in, sing-box outbounds out. Parsing only, no I/O.

Shared by render.py (root, builds the tunnel) and the tun4ai CLI (the user), so
what the CLI accepts is exactly what the root helper will accept.
Must run on the /usr/bin/python3 that ships with macOS (3.9).
"""

import base64
import binascii
import ipaddress
import json
import os
import re
import unicodedata
from urllib.parse import parse_qs, quote, unquote, urlsplit

LANG = "zh" if os.environ.get("TUN4AI_LANG", "").startswith("zh") else "en"

# pf blocks these ports before it lets the proxy through, so a node there could never connect.
BLOCKED_PORTS = (53, 3478, 5349, 19302)
NAME_MAX = 64

# AEAD only. Stream ciphers are broken and sing-box keeps them for compatibility.
SS_METHODS = (
    "2022-blake3-aes-128-gcm",
    "2022-blake3-aes-256-gcm",
    "2022-blake3-chacha20-poly1305",
    "aes-128-gcm",
    "aes-192-gcm",
    "aes-256-gcm",
    "chacha20-ietf-poly1305",
    "xchacha20-ietf-poly1305",
)
VMESS_SECURITY = ("auto", "none", "zero", "aes-128-gcm", "chacha20-poly1305")
TUIC_CONGESTION = ("cubic", "new_reno", "bbr")
TUIC_RELAY = ("native", "quic")
UTLS_FINGERPRINTS = (
    "chrome", "firefox", "edge", "safari", "360", "qq", "ios", "android", "random", "randomized",
)


def t(zh, en):
    return zh if LANG == "zh" else en


class LinkError(ValueError):
    pass


def fail(zh, en):
    raise LinkError(t(zh, en))


def b64decode(text):
    text = text.strip().replace("-", "+").replace("_", "/")
    text += "=" * (-len(text) % 4)
    try:
        return base64.b64decode(text, validate=True).decode("utf-8")
    except (binascii.Error, UnicodeDecodeError):
        fail("不是有效的 base64", "not valid base64")


def clean_name(raw):
    name = "".join(ch for ch in unquote(raw or "") if unicodedata.category(ch)[0] != "C").strip()
    return name[:NAME_MAX].strip()


def check_host(host):
    host = (host or "").strip().lower().rstrip(".")
    if not host:
        fail("缺少服务器地址", "the server address is missing")
    try:
        ipaddress.IPv4Address(host)
        return host, True
    except ipaddress.AddressValueError:
        pass
    if ":" in host or host.startswith("["):
        fail("不支持 IPv6 服务器地址（隧道只走 IPv4）", "IPv6 server addresses are not supported (the tunnel is IPv4 only)")
    if not re.fullmatch(r"[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+", host):
        fail(f"服务器地址不对：{host}", f"invalid server address: {host}")
    return host, False


def check_port(port):
    try:
        port = int(port)
    except (TypeError, ValueError):
        fail(f"端口不对：{port}", f"invalid port: {port}")
    if not 1 <= port <= 65535:
        fail(f"端口不对：{port}", f"invalid port: {port}")
    if port in BLOCKED_PORTS:
        fail(f"端口 {port} 被防火墙固定拦截，节点不能用这个端口",
             f"port {port} is always blocked by the firewall and cannot be used for a node")
    return port


def split_url(link):
    try:
        parts = urlsplit(link)
        port = parts.port
    except ValueError:
        fail("地址或端口格式不对（不支持端口跳跃）", "malformed address or port (port hopping is not supported)")
    netloc = parts.netloc
    userinfo = unquote(netloc.rpartition("@")[0]) if "@" in netloc else ""
    query = {key: values[0] for key, values in parse_qs(parts.query, keep_blank_values=True).items()}
    return parts, port, userinfo, query


def truthy(value):
    return str(value).strip().lower() in ("1", "true", "yes")


def tls_block(server_name, insecure=False, alpn="", fingerprint="", reality=None):
    tls = {"enabled": True}
    if server_name:
        tls["server_name"] = server_name
    if insecure:
        tls["insecure"] = True
    if alpn:
        tls["alpn"] = [item.strip() for item in alpn.split(",") if item.strip()]
    if fingerprint or reality:
        fingerprint = (fingerprint or "chrome").lower()
        if fingerprint not in UTLS_FINGERPRINTS:
            fail(f"不认识的 TLS 指纹：{fingerprint}", f"unknown TLS fingerprint: {fingerprint}")
        tls["utls"] = {"enabled": True, "fingerprint": fingerprint}
    if reality:
        tls["reality"] = {"enabled": True, **reality}
    return tls


# Xray writes WebSocket early data into the path as "?ed=2048"; sing-box wants it as fields.
def transport_block(kind, host="", path="", service=""):
    if kind in ("", "tcp", "raw", "none"):
        return "tcp", None
    if kind == "ws":
        block = {"type": "ws"}
        path = path or "/"
        base, _, extra = path.partition("?")
        early = parse_qs(extra).get("ed", [""])[0]
        if early.isdigit():
            block["max_early_data"] = int(early)
            block["early_data_header_name"] = "Sec-WebSocket-Protocol"
            path = base or "/"
        block["path"] = path
        if host:
            block["headers"] = {"Host": host}
        return "ws", block
    if kind == "httpupgrade":
        block = {"type": "httpupgrade", "path": path or "/"}
        if host:
            block["host"] = host
        return "httpupgrade", block
    if kind == "grpc":
        block = {"type": "grpc"}
        if service:
            block["service_name"] = service
        return "grpc", block
    fail(f"不支持的传输方式：{kind}（支持 tcp、ws、httpupgrade、grpc）",
         f"unsupported transport: {kind} (tcp, ws, httpupgrade and grpc are supported)")


def node(name, protocol, host, address_is_ip, port, transport, network, security, outbound, secrets, server_name=""):
    return {
        "name": name,
        "protocol": protocol,
        "host": host,
        "address_is_ip": address_is_ip,
        "port": port,
        # What pf has to let through to the server: exactly one of tcp or udp.
        "transport": transport,
        "network": network,
        "security": security,
        "insecure": bool(outbound.get("tls", {}).get("insecure")),
        "server_name": server_name,
        "outbound": outbound,
        "secrets": [s for s in secrets if s],
    }


def parse_vless(link):
    parts, port, uuid, q = split_url(link)
    host, is_ip = check_host(parts.hostname)
    port = check_port(port or 443)
    if not uuid:
        fail("vless 链接缺少 UUID", "the vless link has no UUID")
    security = q.get("security", "").lower() or ("reality" if q.get("pbk") else "none")
    network, transport = transport_block(q.get("type", "").lower(), q.get("host", ""), q.get("path", ""), q.get("serviceName", ""))
    sni = (q.get("sni") or q.get("peer") or q.get("host") or ("" if is_ip else host)).lower()
    out = {"type": "vless", "uuid": uuid}
    if security == "reality":
        if not q.get("pbk"):
            fail("Reality 链接缺少 pbk（公钥）", "the Reality link has no pbk (public key)")
        if not sni:
            fail("Reality 链接缺少 sni", "the Reality link has no sni")
        if not re.fullmatch(r"[0-9a-fA-F]{0,16}", q.get("sid", "")):
            fail("Reality 的 sid 要是最多 16 位的十六进制", "the Reality sid must be up to 16 hex digits")
        reality = {"public_key": q["pbk"], "short_id": q.get("sid", "")}
        out["tls"] = tls_block(sni, False, q.get("alpn", ""), q.get("fp", ""), reality)
    elif security == "tls":
        out["tls"] = tls_block(sni, truthy(q.get("allowInsecure") or q.get("insecure")), q.get("alpn", ""), q.get("fp", ""))
    elif security == "none":
        fail("不接受没有 TLS/Reality 的 VLESS：UUID 和流量都会明文传输",
             "VLESS without TLS or Reality is refused: the UUID and traffic would travel in the clear")
    else:
        fail(f"不支持的 security：{security}", f"unsupported security: {security}")
    flow = q.get("flow", "")
    # Before nodes.txt, a Reality link without flow meant vision. Kept so old vless.txt files still work.
    if not flow and security == "reality" and network == "tcp" and "flow" not in q:
        flow = "xtls-rprx-vision"
    if flow:
        if flow != "xtls-rprx-vision" or network != "tcp":
            fail(f"不支持的 flow：{flow}（只支持 TCP 上的 xtls-rprx-vision）",
                 f"unsupported flow: {flow} (only xtls-rprx-vision over TCP)")
        out["flow"] = flow
    if transport:
        out["transport"] = transport
    name = clean_name(parts.fragment) or f"{host}:{port}"
    return node(name, "vless", host, is_ip, port, "tcp", network, security, out, [uuid], sni)


def parse_trojan(link):
    parts, port, password, q = split_url(link)
    host, is_ip = check_host(parts.hostname)
    port = check_port(port or 443)
    if not password:
        fail("trojan 链接缺少密码", "the trojan link has no password")
    security = q.get("security", "tls").lower() or "tls"
    if security != "tls":
        fail(f"trojan 只支持 TLS，不支持 security={security}", f"trojan is supported over TLS only, not security={security}")
    network, transport = transport_block(q.get("type", "").lower(), q.get("host", ""), q.get("path", ""), q.get("serviceName", ""))
    sni = (q.get("sni") or q.get("peer") or q.get("host") or ("" if is_ip else host)).lower()
    out = {
        "type": "trojan",
        "password": password,
        "tls": tls_block(sni, truthy(q.get("allowInsecure") or q.get("insecure")), q.get("alpn", ""), q.get("fp", "")),
    }
    if transport:
        out["transport"] = transport
    name = clean_name(parts.fragment) or f"{host}:{port}"
    return node(name, "trojan", host, is_ip, port, "tcp", network, "tls", out, [password], sni)


def parse_vmess(link):
    body, _, fragment = link[len("vmess://"):].partition("#")
    try:
        data = json.loads(b64decode(body))
    except ValueError as err:
        if isinstance(err, LinkError):
            raise
        fail("vmess 链接里不是 JSON（只支持 v2rayN 格式）", "the vmess link is not base64 JSON (only the v2rayN format is supported)")
    if not isinstance(data, dict):
        fail("vmess 链接里不是 JSON 对象", "the vmess link does not hold a JSON object")
    text = {key: str(value).strip() for key, value in data.items() if value is not None}
    host, is_ip = check_host(text.get("add"))
    port = check_port(text.get("port") or 443)
    uuid = text.get("id", "")
    if not uuid:
        fail("vmess 链接缺少 id", "the vmess link has no id")
    if text.get("type", "none") not in ("", "none"):
        fail(f"不支持 vmess 伪装类型 {text['type']}", f"vmess header type {text['type']} is not supported")
    network, transport = transport_block(text.get("net", "tcp").lower(), text.get("host", ""), text.get("path", ""), text.get("path", ""))
    security = text.get("scy", "") or "auto"
    if security not in VMESS_SECURITY:
        fail(f"不支持的 vmess 加密：{security}", f"unsupported vmess security: {security}")
    try:
        alter_id = int(text.get("aid") or 0)
    except ValueError:
        fail("vmess 的 aid 不是数字", "vmess aid is not a number")
    out = {"type": "vmess", "uuid": uuid, "security": security, "alter_id": alter_id}
    tls = text.get("tls", "").lower()
    sni = (text.get("sni") or text.get("host") or ("" if is_ip else host)).lower()
    if tls == "tls":
        out["tls"] = tls_block(sni, truthy(text.get("allowInsecure") or text.get("skip-cert-verify")),
                               text.get("alpn", ""), text.get("fp", ""))
    elif tls not in ("", "none"):
        fail(f"不支持的 vmess tls：{tls}", f"unsupported vmess tls: {tls}")
    else:
        sni = ""
    if transport:
        out["transport"] = transport
    name = clean_name(fragment) or clean_name(text.get("ps", "")) or f"{host}:{port}"
    return node(name, "vmess", host, is_ip, port, "tcp", network, "tls" if tls == "tls" else "none", out, [uuid], sni)


def parse_shadowsocks(link):
    rest, _, fragment = link[len("ss://"):].partition("#")
    rest, _, query = rest.partition("?")
    if parse_qs(query).get("plugin"):
        fail("暂不支持 Shadowsocks 插件（plugin=）", "Shadowsocks plugins (plugin=) are not supported yet")
    rest = rest.rstrip("/")
    if "@" not in rest:
        rest = b64decode(rest)
        if "@" not in rest:
            fail("ss 链接格式不对", "malformed ss link")
        userinfo, _, hostport = rest.rpartition("@")
    else:
        userinfo, _, hostport = rest.rpartition("@")
        userinfo = unquote(userinfo)
        if ":" not in userinfo:
            userinfo = b64decode(userinfo)
    method, sep, password = userinfo.partition(":")
    if not sep or not password:
        fail("ss 链接缺少加密方式或密码", "the ss link has no method or password")
    method = method.lower()
    if method not in SS_METHODS:
        fail(f"不支持的 Shadowsocks 加密：{method}（只接受 AEAD 和 2022）",
             f"unsupported Shadowsocks method: {method} (AEAD and 2022 ciphers only)")
    parts, port, _, _ = split_url(f"ss://x@{hostport}")
    host, is_ip = check_host(parts.hostname)
    if port is None:
        fail("ss 链接缺少端口", "the ss link has no port")
    port = check_port(port)
    out = {"type": "shadowsocks", "method": method, "password": password}
    name = clean_name(fragment) or f"{host}:{port}"
    return node(name, "shadowsocks", host, is_ip, port, "tcp", "tcp", "none", out, [password])


def parse_hysteria2(link):
    parts, port, auth, q = split_url(link)
    host, is_ip = check_host(parts.hostname)
    port = check_port(port or 443)
    if not auth:
        fail("hysteria2 链接缺少密码", "the hysteria2 link has no password")
    if q.get("mport"):
        fail("暂不支持端口跳跃（mport）", "port hopping (mport) is not supported")
    if q.get("pinSHA256"):
        fail("暂不支持 pinSHA256 证书固定", "pinSHA256 certificate pinning is not supported")
    sni = (q.get("sni") or ("" if is_ip else host)).lower()
    out = {
        "type": "hysteria2",
        "password": auth,
        "tls": tls_block(sni, truthy(q.get("insecure")), q.get("alpn", "")),
    }
    secrets = [auth]
    obfs = q.get("obfs", "")
    if obfs:
        if obfs != "salamander" or not q.get("obfs-password"):
            fail("hysteria2 混淆只支持 salamander，并且要有 obfs-password",
                 "hysteria2 obfs must be salamander with an obfs-password")
        out["obfs"] = {"type": "salamander", "password": q["obfs-password"]}
        secrets.append(q["obfs-password"])
    name = clean_name(parts.fragment) or f"{host}:{port}"
    return node(name, "hysteria2", host, is_ip, port, "udp", "quic", "tls", out, secrets, sni)


def parse_tuic(link):
    parts, port, userinfo, q = split_url(link)
    host, is_ip = check_host(parts.hostname)
    port = check_port(port or 443)
    uuid, _, password = userinfo.partition(":")
    if not uuid or not password:
        fail("tuic 链接要写成 tuic://UUID:密码@地址:端口", "the tuic link must look like tuic://UUID:PASSWORD@host:port")
    congestion = (q.get("congestion_control") or q.get("congestion") or "cubic").lower()
    if congestion not in TUIC_CONGESTION:
        fail(f"不支持的拥塞控制：{congestion}", f"unsupported congestion control: {congestion}")
    relay = (q.get("udp_relay_mode") or "native").lower()
    if relay not in TUIC_RELAY:
        fail(f"不支持的 udp_relay_mode：{relay}", f"unsupported udp_relay_mode: {relay}")
    sni = (q.get("sni") or ("" if is_ip else host)).lower()
    tls = tls_block(sni, truthy(q.get("allow_insecure") or q.get("insecure") or q.get("allowInsecure")), q.get("alpn", "") or "h3")
    if truthy(q.get("disable_sni")):
        tls["disable_sni"] = True
    out = {
        "type": "tuic",
        "uuid": uuid,
        "password": password,
        "congestion_control": congestion,
        "udp_relay_mode": relay,
        "tls": tls,
    }
    name = clean_name(parts.fragment) or f"{host}:{port}"
    return node(name, "tuic", host, is_ip, port, "udp", "quic", "tls", out, [uuid, password], sni)


PARSERS = {
    "vless": parse_vless,
    "vmess": parse_vmess,
    "trojan": parse_trojan,
    "ss": parse_shadowsocks,
    "hysteria2": parse_hysteria2,
    "hy2": parse_hysteria2,
    "tuic": parse_tuic,
}


def parse_link(link):
    link = link.strip()
    scheme = link.split("://", 1)[0].lower() if "://" in link else ""
    parser = PARSERS.get(scheme)
    if parser is None:
        fail(f"不支持的链接类型：{scheme or link[:16]}（支持 {', '.join(PARSERS)}）",
             f"unsupported link type: {scheme or link[:16]} (supported: {', '.join(PARSERS)})")
    return parser(link)


def with_name(link, name):
    """The link with its display name replaced. The name lives in the URL fragment for every scheme."""
    return link.split("#", 1)[0] + "#" + quote(name, safe="")


def node_lines(text):
    """(file line number, link) for every non-comment line."""
    out = []
    for lineno, raw in enumerate((text or "").splitlines(), 1):
        line = raw.strip()
        if line and not line.startswith("#"):
            out.append((lineno, line))
    return out


def parse_nodes(text, filename):
    nodes = []
    seen = {}
    for lineno, line in node_lines(text):
        try:
            item = parse_link(line)
        except LinkError as err:
            raise LinkError(f"{filename}:{lineno}: {err}") from None
        if item["name"] in seen:
            raise LinkError(t(
                f"{filename}:{lineno}: 节点名 “{item['name']}” 和第 {seen[item['name']]} 行重复，请在链接末尾加 #新名字",
                f"{filename}:{lineno}: node name \"{item['name']}\" repeats line {seen[item['name']]}; add #new-name to the link",
            ))
        seen[item["name"]] = lineno
        item["line"] = lineno
        item["index"] = len(nodes) + 1
        nodes.append(item)
    return nodes


def find(nodes, ref):
    """A name wins over an index, so a node called "2" is still reachable by its name."""
    ref = (ref or "").strip()
    for item in nodes:
        if item["name"] == ref:
            return item
    if ref.isdigit() and 1 <= int(ref) <= len(nodes):
        return nodes[int(ref) - 1]
    return None


def selected(nodes, selected_text):
    """No selection file means the first node. A selection that names nothing is an error, not a fallback:
    silently moving to another exit is exactly what this tool exists to prevent."""
    if not nodes:
        fail("没有可用的节点", "there are no nodes")
    name = (selected_text or "").strip()
    if not name:
        return nodes[0]
    for item in nodes:
        if item["name"] == name:
            return item
    fail(f"选中的节点 “{name}” 不在节点列表里，用 tun4ai node use 重新选择",
         f"the selected node \"{name}\" is not in the node list; pick one with tun4ai node use")


def public(item, is_selected=False):
    """What may be shown to the user, logged, or pasted into an AI chat: everything except credentials."""
    return {
        "index": item.get("index"),
        "name": item["name"],
        "protocol": item["protocol"],
        "server": item["host"],
        "port": item["port"],
        "transport": item["transport"],
        "network": item["network"],
        "security": item["security"],
        "insecure": item["insecure"],
        "selected": is_selected,
    }


def outbound(item, server_ip, iface, tag="proxy"):
    out = dict(item["outbound"])
    proto = out.pop("type")
    return {"type": proto, "tag": tag, "server": server_ip, "server_port": item["port"],
            **out, "bind_interface": iface}

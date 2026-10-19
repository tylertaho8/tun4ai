import ipaddress
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "helper"))

import render  # noqa: E402

LAN = "10.69.237.141"
OTHER = "192.168.0.1"
PROXY = ["203.0.113.7"]
DIRECT = [ipaddress.ip_network("10.0.0.0/8")]
UUID = "11111111-1111-4111-8111-111111111111"
# A throwaway key pair: the format must be valid for `sing-box check`.
PUBKEY = "Vdp5mc4z6trF6_wM_BjEK2FeiZts-oO4wPUSPcdf2jc"
LINK = f"vless://{UUID}@203.0.113.7:443?security=reality&pbk={PUBKEY}&sid=abcd&sni=node.example.com&flow=xtls-rprx-vision"
NODE = {
    "host": "203.0.113.7",
    "sni": "node.example.com",
    "port": 443,
    "uuid": UUID,
    "public_key": PUBKEY,
    "short_id": "abcd",
    "flow": "xtls-rprx-vision",
    "address_is_ip": True,
}


def lines(text):
    return [line for line in text.splitlines() if line.strip() and not line.startswith("#")]


def default_iface():
    out = subprocess.run(["route", "-n", "get", "default"], capture_output=True, text=True).stdout
    for line in out.splitlines():
        if "interface:" in line:
            return line.split()[-1]
    return None


def pfctl_check(text):
    with tempfile.NamedTemporaryFile("w", suffix=".conf", delete=False) as handle:
        handle.write(text)
        path = handle.name
    try:
        run = subprocess.run(["pfctl", "-nf", path], capture_output=True, text=True)
        if run.returncode != 0 and "Operation not permitted" in run.stderr:
            run = subprocess.run(["sudo", "-n", "pfctl", "-nf", path], capture_output=True, text=True)
        return run
    finally:
        os.unlink(path)


class Workdir(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="tun4ai-test-"))
        self.addCleanup(shutil.rmtree, self.dir)


class DnsRedirect(unittest.TestCase):
    def test_both_protocols_go_to_the_tunnel_dns_except_root(self):
        rules = lines(render.dns_redirect(LAN))
        self.assertEqual(len(rules), 2)
        for rule, proto in zip(rules, ("udp", "tcp")):
            self.assertIn(f"proto {proto}", rule)
            self.assertIn(f"to {LAN} port 53", rule)
            self.assertIn(f"route-to ($tun_if {render.TUN_DNS})", rule)
            self.assertIn("user != 0", rule)
            self.assertIn("quick", rule)

    def test_a_different_lan_dns_is_a_different_rule(self):
        self.assertIn(f"to {OTHER} port 53", render.dns_redirect(OTHER))
        self.assertNotIn(LAN, render.dns_redirect(OTHER))

    def test_kill_switch_redirects_before_it_blocks_other_dns(self):
        text = render.pf_text("en0", DIRECT, LAN, PROXY, 443)
        redirect = text.index("user != 0")
        root_pass = text.index("pass out quick inet proto udp to $lan_dns port 53")
        block = text.index("block drop out quick proto udp to any port 53")
        self.assertLess(redirect, root_pass)
        self.assertLess(root_pass, block)
        self.assertIn("block drop out quick all", text)

    def test_root_can_still_query_the_lan_dns_directly(self):
        text = render.pf_text("en0", DIRECT, LAN, PROXY, 443)
        root = [
            line for line in lines(text)
            if "to $lan_dns port 53" in line and "user != 0" not in line
        ]
        self.assertEqual(len(root), 2)
        self.assertTrue(all("route-to" not in line for line in root))

    def test_firewall_off_keeps_only_the_dns_redirect(self):
        text = render.pf_dns_text("en0", LAN)
        self.assertIn("user != 0", text)
        self.assertIn("block drop out quick on $ext_if inet6 proto { tcp, udp } to any port 53", text)
        self.assertNotIn("block drop out quick all", text)
        self.assertNotIn("<proxy4>", text)
        self.assertNotIn(OTHER, text)

    def test_direct_ranges_pass_every_protocol(self):
        # AirDrop and Bonjour need UDP 5353 to ff02::fb / 224.0.0.251 on awdl0 and the LAN.
        direct = DIRECT + [ipaddress.ip_network(n) for n in ("224.0.0.251/32", "fe80::/10", "ff02::fb/128")]
        text = render.pf_text("en0", direct, LAN, PROXY, 443)
        rules = lines(text)
        for rule in (
            "pass out quick inet to <direct4> keep state",
            "pass in quick inet from <direct4> to any keep state",
            "pass out quick inet6 to <direct6> keep state",
            "pass in quick inet6 from <direct6> to any keep state",
        ):
            self.assertIn(rule, rules)
            self.assertLess(text.index("block drop out quick proto udp to any port 53"), text.index(rule))
            self.assertLess(text.index("to any port { 3478, 5349, 19302 }"), text.index(rule))
            self.assertLess(text.index(rule), text.index("block drop out quick all"))

    def test_pfctl_accepts_both_rule_sets(self):
        direct = DIRECT + [ipaddress.ip_network("fe80::/10")]
        for sample in (render.pf_text("en0", direct, LAN, PROXY, 8443), render.pf_dns_text("en1", OTHER)):
            run = pfctl_check(sample)
            if run.returncode != 0 and "password" in run.stderr:
                self.skipTest("pfctl needs root here")
            self.assertEqual(run.returncode, 0, run.stderr)


class VlessOutbound(unittest.TestCase):
    def test_firewall_allows_only_tcp_to_the_server(self):
        text = render.pf_text("en0", DIRECT, LAN, PROXY, 443)
        self.assertIn("proto tcp to <proxy4> port 443", text)
        self.assertNotIn("proto udp to <proxy4>", text)
        self.assertNotIn(UUID, text)

    def test_outbound_is_vless_reality_and_app_udp_stays_dropped(self):
        cfg = render.config(NODE, DIRECT, ["example.com"], LAN, "en0", PROXY, True)
        out = cfg["outbounds"][0]
        self.assertEqual(out["type"], "vless")
        self.assertEqual(out["server"], PROXY[0])
        self.assertEqual(out["flow"], "xtls-rprx-vision")
        self.assertEqual(out["tls"]["server_name"], "node.example.com")
        self.assertTrue(out["tls"]["reality"]["enabled"])
        self.assertEqual(cfg["route"]["final"], "vless-out")
        self.assertIn({"network": "udp", "action": "reject", "method": "drop"}, cfg["route"]["rules"])
        self.assertEqual(cfg["inbounds"][0]["interface_name"], render.TUN_IF)


class NodeFile(Workdir):
    def write(self, text, name="vless.txt"):
        path = self.dir / name
        path.write_text(text)
        return path

    def test_share_link_is_parsed(self):
        node = render.load_node(self.write(f"# comment\n{LINK}\n"))
        self.assertEqual(node, NODE)

    def test_domain_server_and_default_port(self):
        node = render.load_node(self.write(f"vless://{UUID}@Node.Example.com?pbk=k&sid=s"))
        self.assertEqual((node["host"], node["port"], node["sni"]), ("node.example.com", 443, "node.example.com"))
        self.assertFalse(node["address_is_ip"])

    def test_template_without_a_link_is_rejected(self):
        template = (ROOT / "examples" / "vless.txt").read_text()
        with self.assertRaises(SystemExit):
            render.load_node(self.write(template))

    def test_missing_reality_key_is_rejected(self):
        with self.assertRaises(SystemExit):
            render.load_node(self.write(f"vless://{UUID}@203.0.113.7:443?sid=abcd"))

    def test_symlink_is_not_followed(self):
        secret = self.dir / "secret"
        secret.write_text(LINK)
        (self.dir / "vless.txt").symlink_to(secret)
        with self.assertRaises(SystemExit):
            render.load_node(self.dir / "vless.txt")

    def test_file_owned_by_someone_else_is_rejected(self):
        path = self.write(LINK)
        with self.assertRaises(SystemExit):
            render.load_node(path, owner_uid=os.getuid() + 12345)


class DirectFile(Workdir):
    def test_example_parses_to_private_ranges_only(self):
        nets, domains = render.load_direct(ROOT / "examples" / "direct.txt")
        self.assertEqual(domains, [])
        self.assertTrue(nets)
        self.assertTrue(all(net.is_private or net.is_multicast or net.is_link_local for net in nets))

    def test_bad_line_names_the_line(self):
        path = self.dir / "direct.txt"
        path.write_text("10.0.0.0/8\nnot a thing!\n")
        with self.assertRaises(SystemExit) as caught:
            render.load_direct(path)
        self.assertIn("direct.txt:2", str(caught.exception.code))

    def test_missing_direct_list_means_nothing_is_direct(self):
        self.assertEqual(render.load_direct(self.dir / "nope.txt"), ([], []))


class EndToEnd(Workdir):
    def test_render_writes_config_and_rules_without_leaking_the_uuid(self):
        iface = default_iface()
        if not iface:
            self.skipTest("no default route")
        config_dir = self.dir / "config"
        state_dir = self.dir / "state"
        config_dir.mkdir()
        (config_dir / "vless.txt").write_text(LINK + "\n")
        shutil.copy(ROOT / "examples" / "direct.txt", config_dir / "direct.txt")
        run = subprocess.run(
            [sys.executable, str(ROOT / "helper" / "render.py"), "--if", iface,
             "--config-dir", str(config_dir), "--state-dir", str(state_dir)],
            capture_output=True, text=True, env={**os.environ, "TUN4AI_LANG": "en"},
        )
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertIn("node node.example.com:443", run.stdout)
        cfg = json.loads((state_dir / "config.json").read_text())
        self.assertEqual(cfg["outbounds"][0]["uuid"], UUID)
        self.assertEqual(oct((state_dir / "config.json").stat().st_mode & 0o777), "0o600")
        for name in ("pf.killswitch.conf", "pf.dns.conf"):
            self.assertNotIn(UUID, (state_dir / name).read_text())
        box = os.environ.get("SING_BOX") or str(ROOT / "build" / "sing-box")
        if os.access(box, os.X_OK):
            check = subprocess.run([box, "check", "-c", str(state_dir / "config.json")],
                                   capture_output=True, text=True)
            self.assertEqual(check.returncode, 0, check.stderr)

    def test_bad_interface_name_is_refused(self):
        run = subprocess.run(
            [sys.executable, str(ROOT / "helper" / "render.py"), "--if", "en0;rm",
             "--config-dir", str(self.dir), "--state-dir", str(self.dir)],
            capture_output=True, text=True,
        )
        self.assertNotEqual(run.returncode, 0)


if __name__ == "__main__":
    unittest.main()

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
sys.path.insert(0, str(ROOT / "tests"))

import nodes  # noqa: E402
import render  # noqa: E402
from pfsim import Ruleset  # noqa: E402
from samples import EXPECTED, LINKS  # noqa: E402

LAN = "10.69.237.141"
OTHER = "192.168.0.1"
PROXY = ["203.0.113.7"]
DIRECT = [ipaddress.ip_network("10.0.0.0/8")]
UUID = "11111111-1111-4111-8111-111111111111"
LINK = LINKS["vless-reality"]
NODE = nodes.parse_link(LINK)
SING_BOX = os.environ.get("SING_BOX") or str(ROOT / "build" / "sing-box")


def lines(text):
    return [line for line in text.splitlines() if line.strip() and not line.startswith("#")]


def default_iface():
    if not shutil.which("route") or sys.platform != "darwin":
        return None
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
        if not shutil.which("pfctl") and not os.path.exists("/sbin/pfctl"):
            return None
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
        text = render.pf_text("en0", DIRECT, LAN, PROXY, 443, "tcp")
        redirect = text.index("user != 0")
        root_pass = text.index("pass out quick inet proto udp to $lan_dns port 53")
        block = text.index("block drop out quick proto udp to any port 53")
        self.assertLess(redirect, root_pass)
        self.assertLess(root_pass, block)
        self.assertIn("block drop out quick all", text)

    def test_root_can_still_query_the_lan_dns_directly(self):
        text = render.pf_text("en0", DIRECT, LAN, PROXY, 443, "tcp")
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
        text = render.pf_text("en0", direct, LAN, PROXY, 443, "tcp")
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

    def test_pfctl_accepts_every_rule_set(self):
        direct = DIRECT + [ipaddress.ip_network("fe80::/10")]
        for sample in (render.pf_text("en0", direct, LAN, PROXY, 8443, "tcp"),
                       render.pf_text("en0", direct, LAN, PROXY + ["203.0.113.8"], 443, "udp"),
                       render.pf_dns_text("en1", OTHER)):
            run = pfctl_check(sample)
            if run is None:
                self.skipTest("pfctl only exists on macOS")
            if run.returncode != 0 and "password" in run.stderr:
                self.skipTest("pfctl needs root here")
            self.assertEqual(run.returncode, 0, run.stderr)


class KillSwitch(unittest.TestCase):
    """Packets judged by an independent pf evaluator, not by grepping the rule text."""

    OTHER_NODE = "198.51.100.99"

    def rules(self, key, ips=None):
        item = nodes.parse_link(LINKS[key])
        ips = ips or (["203.0.113.50"] if not item["address_is_ip"] else [item["host"]])
        return item, ips, Ruleset(render.pf_text("en0", DIRECT, LAN, ips, item["port"], item["transport"]))

    def test_only_the_selected_server_port_and_protocol_get_out(self):
        for key, (_, _, port, transport) in EXPECTED.items():
            with self.subTest(key):
                item, ips, pf = self.rules(key)
                other = "udp" if transport == "tcp" else "tcp"
                for ip in ips:
                    self.assertEqual(pf.verdict(proto=transport, dst=ip, dport=port), "pass")
                    self.assertEqual(pf.verdict(proto=other, dst=ip, dport=port), "block")
                    self.assertEqual(pf.verdict(proto=transport, dst=ip, dport=port + 1), "block")
                # Root (sing-box) gets no extra room either.
                self.assertEqual(pf.verdict(proto=transport, dst=self.OTHER_NODE, dport=port, user=0), "block")
                self.assertEqual(pf.verdict(proto="tcp", dst="8.8.8.8", dport=443), "block")
                self.assertEqual(pf.verdict(proto="udp", dst="8.8.8.8", dport=443), "block")
                self.assertEqual(pf.verdict(proto="tcp", dst="2001:db8::1", dport=443), "block")

    def test_exactly_one_rule_opens_the_server_and_it_is_the_right_protocol(self):
        for key, (_, _, port, transport) in EXPECTED.items():
            with self.subTest(key):
                _, _, pf = self.rules(key)
                holes = [r for r in pf.rules if r["action"] == "pass" and r["to"] == "<proxy4>"
                         and r["proto"] != ["icmp"]]
                self.assertEqual(len(holes), 1, [r["text"] for r in holes])
                self.assertEqual((holes[0]["proto"], holes[0]["to_port"]), ([transport], [port]))

    def test_every_resolved_address_of_a_domain_node_passes_and_nothing_else(self):
        ips = ["203.0.113.50", "203.0.113.51"]
        _, _, pf = self.rules("hysteria2", ips)
        self.assertEqual([str(n.network_address) for n in pf.tables["proxy4"]], ips)
        for ip in ips:
            self.assertEqual(pf.verdict(proto="udp", dst=ip, dport=8443), "pass")
        self.assertEqual(pf.verdict(proto="udp", dst="203.0.113.52", dport=8443), "block")

    def test_tunnel_dns_and_dhcp_still_work_and_other_dns_does_not(self):
        _, _, pf = self.rules("tuic")
        self.assertEqual(pf.verdict(proto="tcp", dst="8.8.8.8", dport=443, iface=render.TUN_IF), "pass")
        self.assertEqual(pf.verdict(proto="udp", dst=LAN, dport=53), "route-to")
        self.assertEqual(pf.verdict(proto="udp", dst=LAN, dport=53, user=0), "pass")
        self.assertEqual(pf.verdict(proto="udp", dst="1.1.1.1", dport=53, user=0), "block")
        self.assertEqual(pf.verdict(proto="udp", dst="255.255.255.255", dport=67, sport=68), "pass")
        self.assertEqual(pf.verdict(proto="tcp", dst="10.1.2.3", dport=22), "pass")

    def test_stun_stays_blocked_even_towards_the_server(self):
        _, ips, pf = self.rules("hysteria2")
        self.assertEqual(pf.verdict(proto="udp", dst=ips[0], dport=3478), "block")

    def test_firewall_off_rules_do_not_block_ordinary_traffic(self):
        pf = Ruleset(render.pf_dns_text("en0", LAN))
        self.assertEqual(pf.verdict(proto="udp", dst="8.8.8.8", dport=443), "pass")
        self.assertEqual(pf.verdict(proto="udp", dst=LAN, dport=53), "route-to")

    def test_unknown_transport_is_a_bug_not_a_silent_tcp_rule(self):
        with self.assertRaises(ValueError):
            render.pf_text("en0", DIRECT, LAN, PROXY, 443, "sctp")


class SingBoxConfig(Workdir):
    def test_outbound_is_the_selected_node_and_app_udp_stays_dropped(self):
        cfg = render.config(NODE, DIRECT, ["example.com"], LAN, "en0", PROXY, True)
        out = cfg["outbounds"][0]
        self.assertEqual((out["type"], out["tag"], out["server"]), ("vless", render.PROXY_TAG, PROXY[0]))
        self.assertEqual(out["tls"]["server_name"], "node.example.com")
        self.assertEqual(cfg["route"]["final"], render.PROXY_TAG)
        self.assertEqual(cfg["dns"]["servers"][1]["detour"], render.PROXY_TAG)
        self.assertIn({"network": "udp", "action": "reject", "method": "drop"}, cfg["route"]["rules"])
        self.assertIn(f"{PROXY[0]}/32", cfg["inbounds"][0]["route_exclude_address"])

    def test_sing_box_accepts_every_protocol(self):
        if not os.access(SING_BOX, os.X_OK):
            self.skipTest("no sing-box binary; run scripts/fetch-sing-box.sh build")
        for key in LINKS:
            with self.subTest(key):
                item = nodes.parse_link(LINKS[key])
                path = self.dir / f"{key}.json"
                path.write_text(json.dumps(render.config(item, DIRECT, ["corp.example.com"], LAN, "en0", ["203.0.113.50"], True)))
                check = subprocess.run([SING_BOX, "check", "-c", str(path)], capture_output=True, text=True)
                self.assertEqual(check.returncode, 0, check.stderr)


class NodeFiles(Workdir):
    def write(self, name, text):
        path = self.dir / name
        path.write_text(text)
        return path

    def test_legacy_vless_txt_is_read_while_nodes_txt_is_absent(self):
        self.write("vless.txt", f"# comment\n{LINK}\n")
        path, found = render.load_nodes(self.dir)
        self.assertEqual(path.name, "vless.txt")
        self.assertEqual(found[0]["outbound"], NODE["outbound"])

    def test_nodes_txt_wins_over_vless_txt(self):
        self.write("vless.txt", LINK + "\n")
        self.write("nodes.txt", LINKS["tuic"] + "\n")
        path, found = render.load_nodes(self.dir)
        self.assertEqual((path.name, [n["name"] for n in found]), ("nodes.txt", ["tuic"]))

    def test_a_nodes_txt_symlink_is_refused_not_skipped_in_favour_of_vless_txt(self):
        self.write("vless.txt", LINK + "\n")
        secret = self.write("secret", LINKS["tuic"] + "\n")
        (self.dir / "nodes.txt").symlink_to(secret)
        with self.assertRaises(SystemExit):
            render.load_nodes(self.dir)

    def test_the_shipped_template_has_no_nodes_and_is_rejected(self):
        shutil.copy(ROOT / "examples" / "nodes.txt", self.dir / "nodes.txt")
        with self.assertRaises(SystemExit) as caught:
            render.load_nodes(self.dir)
        self.assertIn("nodes.txt", str(caught.exception.code))

    def test_every_example_in_the_template_is_a_supported_scheme(self):
        examples = [line[2:].split()[0] for line in (ROOT / "examples" / "nodes.txt").read_text().splitlines()
                    if "://" in line and line.startswith("# ")]
        self.assertEqual({e.split("://")[0] for e in examples}, {"vless", "vmess", "trojan", "ss", "hysteria2", "tuic"})

    def test_selection_file(self):
        self.write("nodes.txt", LINK + "\n" + LINKS["tuic"] + "\n")
        _, found = render.load_nodes(self.dir)
        self.assertEqual(render.load_selected(self.dir, found)["name"], "reality")
        self.write("selected.txt", "tuic\n")
        self.assertEqual(render.load_selected(self.dir, found)["name"], "tuic")
        self.write("selected.txt", "deleted-node\n")
        with self.assertRaises(SystemExit):
            render.load_selected(self.dir, found)

    def test_selection_file_gets_the_same_checks(self):
        self.write("nodes.txt", LINK + "\n")
        _, found = render.load_nodes(self.dir)
        (self.dir / "selected.txt").symlink_to(self.write("elsewhere", "reality\n"))
        with self.assertRaises(SystemExit):
            render.load_selected(self.dir, found)

    def test_symlink_is_not_followed(self):
        secret = self.write("secret", LINK)
        (self.dir / "vless.txt").symlink_to(secret)
        with self.assertRaises(SystemExit):
            render.load_nodes(self.dir)

    def test_file_owned_by_someone_else_is_rejected(self):
        self.write("nodes.txt", LINK)
        with self.assertRaises(SystemExit):
            render.load_nodes(self.dir, owner_uid=os.getuid() + 12345)

    def test_file_over_64kb_is_rejected(self):
        self.write("nodes.txt", LINK + "\n" + "#" * render.MAX_USER_FILE)
        with self.assertRaises(SystemExit):
            render.load_nodes(self.dir)


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
    def setup_dirs(self, node_text, selected=None):
        config_dir = self.dir / "config"
        state_dir = self.dir / "state"
        config_dir.mkdir()
        (config_dir / "nodes.txt").write_text(node_text)
        if selected:
            (config_dir / "selected.txt").write_text(selected + "\n")
        shutil.copy(ROOT / "examples" / "direct.txt", config_dir / "direct.txt")
        return config_dir, state_dir

    def render_in_process(self, config_dir, state_dir, resolved):
        """Same code path as the root helper, with the two macOS-only lookups replaced."""
        from unittest import mock
        with mock.patch.object(render, "lan_dns_for", return_value=LAN), \
                mock.patch.object(render, "resolve_proxy", return_value=(resolved, "resolved")):
            return render.render("en0", config_dir, state_dir)

    def test_selected_node_ends_up_in_config_pf_and_summary_and_no_secret_leaks(self):
        text = "".join(LINKS[k] + "\n" for k in LINKS)
        config_dir, state_dir = self.setup_dirs(text, "hy2")
        summary = self.render_in_process(config_dir, state_dir, ["203.0.113.60"])
        self.assertIn("hy2 (hysteria2, hy.example.com:8443/udp)", summary)
        cfg = json.loads((state_dir / "config.json").read_text())
        self.assertEqual((cfg["outbounds"][0]["type"], cfg["outbounds"][0]["server"]), ("hysteria2", "203.0.113.60"))
        self.assertEqual(oct((state_dir / "config.json").stat().st_mode & 0o777), "0o600")
        pf = Ruleset((state_dir / "pf.killswitch.conf").read_text())
        self.assertEqual(pf.verdict(proto="udp", dst="203.0.113.60", dport=8443), "pass")
        self.assertEqual(pf.verdict(proto="tcp", dst="203.0.113.7", dport=443), "block")  # the reality node
        active = json.loads((state_dir / "active-node.json").read_text())
        self.assertEqual((active["name"], active["server_ips"], active["transport"]), ("hy2", ["203.0.113.60"], "udp"))
        # tray/src/nodes.rs reads these keys.
        self.assertLessEqual({"name", "protocol", "server", "server_name", "port", "transport"}, set(active))
        self.assertEqual(active["server_name"], "hy.example.com")
        public = "".join((state_dir / n).read_text() for n in ("pf.killswitch.conf", "pf.dns.conf", "active-node.json"))
        for item in nodes.parse_nodes(text, "nodes.txt"):
            for secret in item["secrets"]:
                self.assertNotIn(secret, public)
        if os.access(SING_BOX, os.X_OK):
            check = subprocess.run([SING_BOX, "check", "-c", str(state_dir / "config.json")], capture_output=True, text=True)
            self.assertEqual(check.returncode, 0, check.stderr)

    def test_stale_selection_writes_nothing(self):
        config_dir, state_dir = self.setup_dirs(LINK + "\n", "gone")
        with self.assertRaises(SystemExit):
            self.render_in_process(config_dir, state_dir, ["203.0.113.60"])
        self.assertFalse((state_dir / "pf.killswitch.conf").exists())

    def test_render_script_on_macos(self):
        iface = default_iface()
        if not iface:
            self.skipTest("needs macOS route/ipconfig")
        config_dir, state_dir = self.setup_dirs(LINK + "\n")
        run = subprocess.run(
            [sys.executable, "-E", "-s", str(ROOT / "helper" / "render.py"), "--if", iface,
             "--config-dir", str(config_dir), "--state-dir", str(state_dir)],
            capture_output=True, text=True, env={**os.environ, "TUN4AI_LANG": "en"},
        )
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertIn("node reality (vless, 203.0.113.7:443/tcp)", run.stdout)
        for name in ("pf.killswitch.conf", "pf.dns.conf", "active-node.json"):
            self.assertNotIn(UUID, (state_dir / name).read_text())

    def test_bad_interface_name_is_refused(self):
        run = subprocess.run(
            [sys.executable, str(ROOT / "helper" / "render.py"), "--if", "en0;rm",
             "--config-dir", str(self.dir), "--state-dir", str(self.dir)],
            capture_output=True, text=True,
        )
        self.assertNotEqual(run.returncode, 0)


if __name__ == "__main__":
    unittest.main()

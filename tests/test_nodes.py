import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "helper"))
sys.path.insert(0, str(ROOT / "tests"))

import nodes  # noqa: E402
from samples import EXPECTED, LINKS, PUBKEY, UUID  # noqa: E402


def parse(key):
    return nodes.parse_link(LINKS[key])


class EveryProtocol(unittest.TestCase):
    def test_each_sample_parses_to_its_server_port_and_pf_protocol(self):
        for key, (protocol, server, port, transport) in EXPECTED.items():
            with self.subTest(key):
                item = parse(key)
                self.assertEqual((item["protocol"], item["host"], item["port"], item["transport"]),
                                 (protocol, server, port, transport))

    def test_public_view_never_carries_credentials(self):
        for key in LINKS:
            with self.subTest(key):
                item = parse(key)
                text = repr(nodes.public(item))
                self.assertTrue(item["secrets"])
                for secret in item["secrets"]:
                    self.assertNotIn(secret, text)

    def test_outbound_is_addressed_to_the_resolved_ip_and_bound_to_the_card(self):
        out = nodes.outbound(parse("trojan"), "198.51.100.77", "en0")
        self.assertEqual((out["type"], out["tag"], out["server"], out["server_port"], out["bind_interface"]),
                         ("trojan", "proxy", "198.51.100.77", 443, "en0"))
        # Resolving the server to an IP must not lose the certificate name.
        self.assertEqual(out["tls"]["server_name"], "trojan.example.com")


class Vless(unittest.TestCase):
    def test_reality(self):
        out = parse("vless-reality")["outbound"]
        self.assertEqual(out["uuid"], UUID)
        self.assertEqual(out["flow"], "xtls-rprx-vision")
        self.assertEqual(out["tls"]["reality"], {"enabled": True, "public_key": PUBKEY, "short_id": "abcd"})
        self.assertEqual(out["tls"]["utls"]["fingerprint"], "chrome")
        self.assertNotIn("transport", out)

    def test_old_vless_txt_without_security_or_flow_keeps_meaning_reality_vision(self):
        item = nodes.parse_link(f"vless://{UUID}@Node.Example.com?pbk=k&sid=ab")
        self.assertEqual((item["security"], item["host"], item["port"]), ("reality", "node.example.com", 443))
        self.assertEqual(item["outbound"]["flow"], "xtls-rprx-vision")
        self.assertEqual(item["outbound"]["tls"]["server_name"], "node.example.com")

    def test_websocket_early_data_moves_out_of_the_path(self):
        out = parse("vless-ws-tls")["outbound"]
        self.assertEqual(out["transport"], {"type": "ws", "path": "/ray", "max_early_data": 2048,
                                            "early_data_header_name": "Sec-WebSocket-Protocol",
                                            "headers": {"Host": "cdn.example.com"}})
        self.assertEqual(out["tls"]["server_name"], "ws.example.com")
        self.assertNotIn("flow", out)

    def test_grpc(self):
        out = parse("vless-grpc-tls")["outbound"]
        self.assertEqual(out["transport"], {"type": "grpc", "service_name": "tunnel"})
        self.assertEqual(out["tls"]["alpn"], ["h2"])

    def test_plaintext_vless_is_refused(self):
        with self.assertRaises(nodes.LinkError):
            nodes.parse_link(f"vless://{UUID}@203.0.113.7:443?security=none&type=ws")

    def test_vision_over_websocket_is_refused(self):
        with self.assertRaises(nodes.LinkError):
            nodes.parse_link(f"vless://{UUID}@a.example.com:443?security=tls&type=ws&flow=xtls-rprx-vision")

    def test_reality_needs_a_key_and_a_hex_short_id(self):
        for link in (f"vless://{UUID}@203.0.113.7:443?security=reality&sid=abcd&sni=a.example.com",
                     f"vless://{UUID}@203.0.113.7:443?security=reality&pbk={PUBKEY}&sid=xyz&sni=a.example.com"):
            with self.subTest(link), self.assertRaises(nodes.LinkError):
                nodes.parse_link(link)


class Vmess(unittest.TestCase):
    def test_websocket_over_tls(self):
        item = parse("vmess-ws-tls")
        out = item["outbound"]
        self.assertEqual(item["name"], "vm-ws")
        self.assertEqual((out["uuid"], out["security"], out["alter_id"]),
                         ("22222222-2222-4222-8222-222222222222", "auto", 0))
        self.assertEqual(out["transport"], {"type": "ws", "path": "/ray", "headers": {"Host": "cdn.example.com"}})
        self.assertEqual(out["tls"]["server_name"], "cdn.example.com")

    def test_plain_tcp_has_no_tls_block(self):
        out = parse("vmess-tcp")["outbound"]
        self.assertNotIn("tls", out)
        self.assertNotIn("transport", out)

    def test_fragment_name_beats_ps(self):
        self.assertEqual(nodes.parse_link(LINKS["vmess-tcp"] + "#renamed")["name"], "renamed")

    def test_non_json_vmess_is_refused(self):
        with self.assertRaises(nodes.LinkError):
            nodes.parse_link("vmess://YXV0bzp1dWlkQGhvc3Q6NDQz")

    def test_http_header_obfuscation_is_refused(self):
        import base64
        import json
        body = base64.b64encode(json.dumps({"add": "a.example.com", "port": 80, "id": UUID, "net": "tcp",
                                            "type": "http"}).encode()).decode()
        with self.assertRaises(nodes.LinkError):
            nodes.parse_link("vmess://" + body)


class Trojan(unittest.TestCase):
    def test_password_and_ws(self):
        out = parse("trojan-ws")["outbound"]
        self.assertEqual(out["password"], "trojan-ws-secret")
        self.assertEqual(out["transport"]["path"], "/tj")
        self.assertEqual(out["tls"]["server_name"], "cdn.example.com")

    def test_trojan_without_tls_is_refused(self):
        with self.assertRaises(nodes.LinkError):
            nodes.parse_link("trojan://pw@a.example.com:443?security=none")


class Shadowsocks(unittest.TestCase):
    def test_all_three_userinfo_encodings(self):
        self.assertEqual(parse("ss")["outbound"], {"type": "shadowsocks", "method": "aes-256-gcm", "password": "ss-secret-pass"})
        self.assertEqual(parse("ss-2022")["outbound"]["password"], "AAECAwQFBgcICQoLDA0ODw==")
        legacy = parse("ss-legacy")
        self.assertEqual((legacy["outbound"]["method"], legacy["outbound"]["password"]),
                         ("chacha20-ietf-poly1305", "legacy-secret"))

    def test_stream_ciphers_and_plugins_are_refused(self):
        import base64
        rc4 = base64.b64encode(b"rc4-md5:pw").decode()
        aead = base64.b64encode(b"aes-128-gcm:pw").decode()
        for link in (f"ss://{rc4}@198.51.100.2:8388",
                     f"ss://{aead}@198.51.100.2:8388/?plugin=obfs-local%3Bobfs%3Dhttp"):
            with self.subTest(link), self.assertRaises(nodes.LinkError):
                nodes.parse_link(link)


class Quic(unittest.TestCase):
    def test_hysteria2_with_salamander(self):
        item = parse("hysteria2")
        self.assertEqual(item["outbound"]["obfs"], {"type": "salamander", "password": "obfs-secret-pass"})
        self.assertEqual(item["outbound"]["password"], "hy2-secret-pass")
        self.assertIn("obfs-secret-pass", item["secrets"])

    def test_hy2_short_scheme_defaults_and_insecure_is_reported(self):
        item = parse("hy2-short")
        self.assertEqual(item["port"], 443)
        self.assertTrue(item["insecure"])
        self.assertTrue(nodes.public(item)["insecure"])

    def test_port_hopping_and_pinning_are_refused_not_ignored(self):
        for link in ("hysteria2://pw@hy.example.com:443,20000-30000?sni=hy.example.com",
                     "hysteria2://pw@hy.example.com:443?mport=20000-30000",
                     "hysteria2://pw@hy.example.com:443?pinSHA256=AB:CD"):
            with self.subTest(link), self.assertRaises(nodes.LinkError):
                nodes.parse_link(link)

    def test_tuic(self):
        out = parse("tuic")["outbound"]
        self.assertEqual((out["uuid"], out["password"], out["congestion_control"], out["udp_relay_mode"]),
                         ("66666666-6666-4666-8666-666666666666", "tuic-secret-pass", "bbr", "native"))
        self.assertEqual(out["tls"]["alpn"], ["h3"])

    def test_tuic_needs_uuid_and_password(self):
        with self.assertRaises(nodes.LinkError):
            nodes.parse_link("tuic://66666666-6666-4666-8666-666666666666@198.51.100.9:443")


class Addresses(unittest.TestCase):
    def test_ipv6_servers_are_refused_because_the_tunnel_is_ipv4(self):
        with self.assertRaises(nodes.LinkError):
            nodes.parse_link("trojan://pw@[2001:db8::1]:443")

    def test_ports_the_firewall_always_blocks_are_refused(self):
        for port in nodes.BLOCKED_PORTS:
            with self.subTest(port), self.assertRaises(nodes.LinkError):
                nodes.parse_link(f"trojan://pw@a.example.com:{port}")

    def test_unknown_scheme_lists_what_is_supported(self):
        with self.assertRaises(nodes.LinkError) as caught:
            nodes.parse_link("socks5://a.example.com:1080")
        self.assertIn("hysteria2", str(caught.exception))

    def test_names_are_stripped_of_control_characters_and_capped(self):
        item = nodes.parse_link(LINKS["trojan"].split("#")[0] + "#a%0Ab%1B" + "x" * 100)
        self.assertNotIn("\n", item["name"])
        self.assertNotIn("\x1b", item["name"])
        self.assertLessEqual(len(item["name"]), nodes.NAME_MAX)

    def test_with_name_round_trips_through_the_parser(self):
        for key in LINKS:
            with self.subTest(key):
                self.assertEqual(nodes.parse_link(nodes.with_name(LINKS[key], "日本 #2"))["name"], "日本 #2")


class NodeList(unittest.TestCase):
    TEXT = "# mine\n\n" + LINKS["trojan"] + "\n" + LINKS["tuic"] + "\n"

    def test_lines_indices_and_comments(self):
        found = nodes.parse_nodes(self.TEXT, "nodes.txt")
        self.assertEqual([(n["index"], n["line"], n["name"]) for n in found], [(1, 3, "trojan"), (2, 4, "tuic")])

    def test_a_bad_line_is_named(self):
        with self.assertRaises(nodes.LinkError) as caught:
            nodes.parse_nodes(self.TEXT + "socks5://x\n", "nodes.txt")
        self.assertIn("nodes.txt:5", str(caught.exception))

    def test_duplicate_names_are_an_error_because_selection_is_by_name(self):
        with self.assertRaises(nodes.LinkError) as caught:
            nodes.parse_nodes(self.TEXT + LINKS["trojan-ws"].split("#")[0] + "#trojan\n", "nodes.txt")
        self.assertIn("nodes.txt:5", str(caught.exception))

    def test_selection(self):
        found = nodes.parse_nodes(self.TEXT, "nodes.txt")
        self.assertEqual(nodes.selected(found, None)["name"], "trojan")
        self.assertEqual(nodes.selected(found, "tuic\n")["name"], "tuic")
        # A stale selection must not quietly pick another exit.
        with self.assertRaises(nodes.LinkError):
            nodes.selected(found, "gone")
        with self.assertRaises(nodes.LinkError):
            nodes.selected([], None)

    def test_find_prefers_names_over_indices(self):
        found = nodes.parse_nodes(LINKS["trojan"].split("#")[0] + "#2\n" + LINKS["tuic"] + "\n", "nodes.txt")
        self.assertEqual(nodes.find(found, "2")["protocol"], "trojan")
        self.assertEqual(nodes.find(found, "1")["protocol"], "trojan")
        self.assertIsNone(nodes.find(found, "3"))
        self.assertEqual(nodes.find(found, "tuic")["protocol"], "tuic")


if __name__ == "__main__":
    unittest.main()

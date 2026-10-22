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
from samples import LINKS  # noqa: E402

CLI = ROOT / "helper" / "tun4ai"


class Cli(unittest.TestCase):
    def setUp(self):
        base = Path(tempfile.mkdtemp(prefix="tun4ai-cli-"))
        self.addCleanup(shutil.rmtree, base)
        self.config = base / "config"
        self.state = base / "state"
        self.helper = base / "helper"
        for d in (self.config, self.state / "run", self.helper):
            d.mkdir(parents=True)

    def run_cli(self, *args, expect=0):
        env = {**os.environ, "TUN4AI_CONFIG_DIR": str(self.config), "TUN4AI_STATE_DIR": str(self.state),
               "TUN4AI_HELPER_DIR": str(self.helper), "TUN4AI_LANG": "en"}
        run = subprocess.run([sys.executable, str(CLI), *args, "--json"], capture_output=True, text=True, env=env)
        self.assertEqual(run.returncode, expect, run.stdout + run.stderr)
        self.assertEqual(run.stdout.count("\n"), 1, "one JSON object on one line")
        data = json.loads(run.stdout)
        self.assertEqual((data["schema"], data["ok"]), (1, expect == 0))
        return data

    def write(self, name, text):
        (self.config / name).write_text(text)

    def read(self, name):
        return (self.config / name).read_text()

    def root_choice(self):
        """What the root helper would build the tunnel from."""
        _, found = render.load_nodes(self.config)
        return render.load_selected(self.config, found)["name"]


class Nodes(Cli):
    def test_legacy_vless_txt_is_listed_and_the_first_edit_renames_it(self):
        self.write("vless.txt", "# my old node\n" + LINKS["vless-reality"] + "\n")
        listed = self.run_cli("node", "list")
        self.assertEqual((listed["file"], listed["selected"]), ("vless.txt", "reality"))
        added = self.run_cli("node", "add", LINKS["tuic"])
        self.assertEqual(added["migrated_from"], "vless.txt")
        self.assertFalse((self.config / "vless.txt").exists())
        self.assertEqual(self.read("nodes.txt"), "# my old node\n" + LINKS["vless-reality"] + "\n" + LINKS["tuic"] + "\n")
        self.assertEqual(oct((self.config / "nodes.txt").stat().st_mode & 0o777), "0o600")
        # Adding a node never moves the exit.
        self.assertEqual(self.root_choice(), "reality")

    def test_add_to_an_empty_folder_creates_nodes_txt(self):
        added = self.run_cli("node", "add", LINKS["ss"])
        self.assertEqual(added["added"]["protocol"], "shadowsocks")
        self.assertEqual(self.root_choice(), "ss")

    def test_a_bad_link_changes_nothing(self):
        self.write("nodes.txt", LINKS["trojan"] + "\n")
        error = self.run_cli("node", "add", "vless://uuid@203.0.113.7:443?security=none", expect=1)
        self.assertEqual(error["error"]["code"], "invalid_link")
        self.assertEqual(self.read("nodes.txt"), LINKS["trojan"] + "\n")

    def test_duplicate_names_are_refused_and_name_renames(self):
        self.write("nodes.txt", LINKS["trojan"] + "\n")
        clash = LINKS["trojan-ws"].split("#")[0] + "#trojan"
        self.assertEqual(self.run_cli("node", "add", clash, expect=1)["error"]["code"], "duplicate_name")
        added = self.run_cli("node", "add", clash, "--name", "东京 ws")
        self.assertEqual(added["added"]["name"], "东京 ws")
        self.assertEqual([n["name"] for n in self.run_cli("node", "list")["nodes"]], ["trojan", "东京 ws"])

    def test_list_shows_everything_but_credentials(self):
        self.write("nodes.txt", "".join(link + "\n" for link in LINKS.values()))
        listed = self.run_cli("node", "list")
        self.assertEqual(len(listed["nodes"]), len(LINKS))
        text = json.dumps(listed, ensure_ascii=False)
        for item in nodes.parse_nodes(self.read("nodes.txt"), "nodes.txt"):
            for secret in item["secrets"]:
                self.assertNotIn(secret, text)
        self.assertEqual(sorted(listed["nodes"][0]), sorted(
            ["index", "name", "protocol", "server", "port", "transport", "network", "security", "insecure", "selected"]))

    def test_use_by_name_or_index_is_what_root_builds(self):
        self.write("nodes.txt", LINKS["trojan"] + "\n" + LINKS["hysteria2"] + "\n" + LINKS["tuic"] + "\n")
        self.assertEqual(self.run_cli("node", "use", "hy2")["selected"]["name"], "hy2")
        self.assertEqual(self.root_choice(), "hy2")
        self.run_cli("node", "use", "3")
        self.assertEqual(self.root_choice(), "tuic")
        self.assertEqual(self.read("selected.txt"), "tuic\n")
        self.assertEqual(self.run_cli("node", "use", "nope", expect=1)["error"]["code"], "not_found")
        self.assertEqual(self.root_choice(), "tuic")

    def test_the_selected_node_is_not_removed_by_accident(self):
        self.write("nodes.txt", "# keep me\n" + LINKS["trojan"] + "\n" + LINKS["tuic"] + "\n")
        self.run_cli("node", "use", "tuic")
        self.assertEqual(self.run_cli("node", "remove", "tuic", expect=1)["error"]["code"], "selected_node")
        removed = self.run_cli("node", "remove", "1")
        self.assertEqual((removed["removed"], removed["selected"]), ("trojan", "tuic"))
        self.assertEqual(self.read("nodes.txt"), "# keep me\n" + LINKS["tuic"] + "\n")
        forced = self.run_cli("node", "remove", "tuic", "--force")
        self.assertIsNone(forced["selected"])
        self.assertFalse((self.config / "selected.txt").exists())

    def test_implicit_first_node_counts_as_selected(self):
        self.write("nodes.txt", LINKS["trojan"] + "\n" + LINKS["tuic"] + "\n")
        self.assertEqual(self.run_cli("node", "remove", "trojan", expect=1)["error"]["code"], "selected_node")

    def test_a_symlinked_node_file_is_refused_like_root_refuses_it(self):
        target = self.config.parent / "elsewhere.txt"
        target.write_text(LINKS["trojan"] + "\n")
        (self.config / "nodes.txt").symlink_to(target)
        self.assertEqual(self.run_cli("node", "list", expect=1)["error"]["code"], "config_unreadable")
        self.assertEqual(self.run_cli("node", "add", LINKS["tuic"], expect=1)["error"]["code"], "config_unreadable")
        self.assertEqual(target.read_text(), LINKS["trojan"] + "\n")

    def test_writes_stop_before_the_64kb_root_limit(self):
        self.write("nodes.txt", "#" * (render.MAX_USER_FILE - 100) + "\n")
        self.assertEqual(self.run_cli("node", "add", LINKS["vmess-ws-tls"], expect=1)["error"]["code"], "file_too_large")

    def test_udp_nodes_are_reported_as_not_testable(self):
        self.write("nodes.txt", LINKS["tuic"] + "\n")
        result = self.run_cli("node", "test")["results"][0]
        self.assertEqual((result["reachable"], result["error"]), (None, "udp_not_testable"))

    def test_tcp_test_measures_a_real_listener(self):
        import socket
        server = socket.socket()
        server.bind(("127.0.0.1", 0))
        server.listen(1)
        self.addCleanup(server.close)
        port = server.getsockname()[1]
        self.write("nodes.txt", f"trojan://pw@127.0.0.1:{port}#local\n")
        result = self.run_cli("node", "test", "local")["results"][0]
        self.assertTrue(result["reachable"])
        self.assertIsInstance(result["tcp_connect_ms"], int)


class Direct(Cli):
    def test_add_normalizes_dedups_and_remove_keeps_comments(self):
        self.write("direct.txt", "# office\n10.0.0.0/8\n")
        self.assertTrue(self.run_cli("direct", "add", "Corp.Example.com")["changed"])
        self.assertFalse(self.run_cli("direct", "add", "corp.example.com")["changed"])
        self.assertEqual(self.run_cli("direct", "add", "192.168.1.7/16")["entry"], "192.168.0.0/16")
        self.assertEqual(self.run_cli("direct", "list")["entries"], ["10.0.0.0/8", "corp.example.com", "192.168.0.0/16"])
        self.run_cli("direct", "remove", "10.0.0.0/8")
        self.assertEqual(self.read("direct.txt"), "# office\ncorp.example.com\n192.168.0.0/16\n")
        nets, domains = render.load_direct(self.config / "direct.txt")
        self.assertEqual(([str(n) for n in nets], domains), (["192.168.0.0/16"], ["corp.example.com"]))

    def test_bad_entries_and_missing_entries(self):
        self.assertEqual(self.run_cli("direct", "add", "not a host!", expect=1)["error"]["code"], "invalid_entry")
        self.assertEqual(self.run_cli("direct", "remove", "10.0.0.0/8", expect=1)["error"]["code"], "not_found")


class State(Cli):
    def install(self):
        (self.helper / "bin").mkdir()
        for name in ("bin/sing-box", "enable.sh", "disable.sh", "firewall-on.sh", "firewall-off.sh"):
            (self.helper / name).write_text("#!/bin/sh\nexit 0\n")

    def test_switching_nodes_while_running_asks_for_a_restart(self):
        self.install()
        self.write("nodes.txt", LINKS["trojan"] + "\n" + LINKS["tuic"] + "\n")
        (self.state / "ENABLED").touch()
        (self.state / "run/sing-box.pid").write_text(f"{os.getpid()}\n")
        built = render.active_node(nodes.parse_nodes(self.read("nodes.txt"), "nodes.txt")[0], ["203.0.113.50"])
        (self.state / "active-node.json").write_text(json.dumps(built))
        status = self.run_cli("status")
        self.assertTrue(status["tunnel"]["running"])
        self.assertFalse(status["restart_needed"])
        self.assertEqual(status["active"]["name"], "trojan")
        used = self.run_cli("node", "use", "tuic")
        self.assertTrue(used["restart_needed"])
        # No utun15 here, so the tunnel check fails and doctor exits 1; the restart hint must be there anyway.
        doctor = self.run_cli("doctor", expect=1)
        self.assertIn("restart_needed", [c["id"] for c in doctor["checks"]])

    def test_not_installed(self):
        status = self.run_cli("status")
        self.assertEqual((status["installed"], status["tunnel"]["running"]), (False, False))
        doctor = self.run_cli("doctor", expect=1)
        self.assertFalse(doctor["healthy"])
        self.assertIn({"id": "installed", "status": "fail", "message": "the background service is not installed"},
                      doctor["checks"])
        self.assertEqual(self.run_cli("start", expect=1)["error"]["code"], "not_installed")

    def test_usage_errors_are_json_too(self):
        self.assertEqual(self.run_cli("node", "teleport", expect=2)["error"]["code"], "usage")


if __name__ == "__main__":
    unittest.main()

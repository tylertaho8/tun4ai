"""A small evaluator for the pf rules render.py writes, so tests can ask "would this packet leave?"
instead of grepping for strings.

It reads the rule text independently of how render.py builds it, and implements pf's own decision:
rules are evaluated top to bottom, the last matching rule wins, a matching `quick` rule wins at once,
and nothing matching means pass. It understands only the grammar render.py emits and raises on
anything else, so a new kind of rule cannot be silently ignored. State tracking is not modelled:
every packet is judged as the first packet of a connection.
"""

import ipaddress
import re
from dataclasses import dataclass


@dataclass
class Packet:
    proto: str  # tcp, udp, icmp
    dst: str
    dport: int = 0
    direction: str = "out"
    iface: str = "en0"
    src: str = "192.168.1.20"
    sport: int = 50000
    user: int = 501
    icmp_type: str = "echoreq"

    @property
    def af(self):
        return "inet6" if ":" in self.dst else "inet"


def tokens(line):
    out = []
    for tok in re.findall(r"\{[^}]*\}|\([^)]*\)|!=|\S+", line):
        if tok.startswith("{"):
            out.append([part.strip() for part in tok[1:-1].split(",") if part.strip()])
        else:
            out.append(tok)
    return out


class Ruleset:
    def __init__(self, text):
        self.macros = {}
        self.tables = {}
        self.skip = set()
        self.rules = []
        for raw in text.splitlines():
            line = raw.split("#", 1)[0].strip()
            if not line:
                continue
            m = re.fullmatch(r'(\w+)\s*=\s*"([^"]*)"', line)
            if m:
                self.macros[m.group(1)] = m.group(2)
                continue
            m = re.fullmatch(r"table <(\w+)> persist \{([^}]*)\}", line)
            if m:
                self.tables[m.group(1)] = [ipaddress.ip_network(x.strip(), strict=False)
                                           for x in m.group(2).split(",") if x.strip()]
                continue
            m = re.fullmatch(r"set skip on (\S+)", line)
            if m:
                self.skip.add(m.group(1))
                continue
            if line.startswith("set "):
                continue
            self.rules.append(self.parse(line))

    def value(self, tok):
        return self.macros.get(tok[1:], tok) if isinstance(tok, str) and tok.startswith("$") else tok

    def parse(self, line):
        toks = tokens(line)
        rule = {"text": line, "action": toks.pop(0), "quick": False, "direction": None, "on": None,
                "route_to": False, "af": None, "proto": None, "from": None, "from_port": None,
                "to": None, "to_port": None, "user_not_root": False, "icmp_type": None}
        if rule["action"] not in ("pass", "block"):
            raise ValueError(f"unknown rule: {line}")
        while toks:
            tok = toks.pop(0)
            if tok == "drop" and rule["action"] == "block":
                continue
            if tok in ("in", "out"):
                rule["direction"] = tok
            elif tok == "quick":
                rule["quick"] = True
            elif tok == "on":
                rule["on"] = self.value(toks.pop(0))
            elif tok == "route-to":
                toks.pop(0)
                rule["route_to"] = True
            elif tok in ("inet", "inet6"):
                rule["af"] = tok
            elif tok == "proto":
                value = toks.pop(0)
                rule["proto"] = value if isinstance(value, list) else [value]
            elif tok == "all":
                continue
            elif tok in ("from", "to"):
                rule[tok] = self.value(toks.pop(0))
                if toks and toks[0] == "port":
                    toks.pop(0)
                    value = toks.pop(0)
                    rule[f"{tok}_port"] = [int(p) for p in (value if isinstance(value, list) else [value])]
            elif tok == "user":
                if toks[:2] != ["!=", "0"]:
                    raise ValueError(f"unknown user clause: {line}")
                del toks[:2]
                rule["user_not_root"] = True
            elif tok == "icmp-type":
                value = toks.pop(0)
                rule["icmp_type"] = value if isinstance(value, list) else [value]
            elif tok == "flags":
                toks.pop(0)
            elif tok == "keep":
                if toks.pop(0) != "state":
                    raise ValueError(line)
            else:
                raise ValueError(f"pfsim does not understand '{tok}' in: {line}")
        return rule

    def addr_match(self, spec, addr):
        if spec is None or spec == "any":
            return True
        ip = ipaddress.ip_address(addr)
        if spec.startswith("<"):
            nets = self.tables[spec.strip("<>")]
        else:
            nets = [ipaddress.ip_network(spec, strict=False)]
        return any(ip.version == net.version and ip in net for net in nets)

    def matches(self, rule, pkt):
        if rule["direction"] and rule["direction"] != pkt.direction:
            return False
        if rule["on"] and rule["on"] != pkt.iface:
            return False
        if rule["af"] and rule["af"] != pkt.af:
            return False
        if rule["proto"] and pkt.proto not in rule["proto"]:
            return False
        if not self.addr_match(rule["from"], pkt.src) or not self.addr_match(rule["to"], pkt.dst):
            return False
        if rule["from_port"] and (pkt.proto not in ("tcp", "udp") or pkt.sport not in rule["from_port"]):
            return False
        if rule["to_port"] and (pkt.proto not in ("tcp", "udp") or pkt.dport not in rule["to_port"]):
            return False
        if rule["user_not_root"] and pkt.user == 0:
            return False
        if rule["icmp_type"] and (pkt.proto != "icmp" or pkt.icmp_type not in rule["icmp_type"]):
            return False
        return True

    def decide(self, pkt):
        """'pass', 'block' or 'route-to', and the rule that decided it."""
        if pkt.iface in self.skip:
            return "pass", None
        verdict, decider = "pass", None
        for rule in self.rules:
            if self.matches(rule, pkt):
                verdict = "route-to" if rule["route_to"] else rule["action"]
                decider = rule
                if rule["quick"]:
                    break
        return verdict, decider

    def verdict(self, **kwargs):
        return self.decide(Packet(**kwargs))[0]

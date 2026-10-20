// One-click leak check: exit IP, DNS resolver, STUN (what WebRTC sees) and IPv6.
// Every probe takes the normal system path like any app would, never the local proxy.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::thread;
use std::time::Duration;

// render.py resolves everything through DoH to 1.1.1.1 inside the tunnel.
const EXPECTED_DNS_ORG: &str = "AS13335";
const TRACE_URL: &str = "https://www.cloudflare.com/cdn-cgi/trace";
const IPV6_URL: &str = "https://api6.ipify.org";
const RESOLVER_NAME: &str = "o-o.myaddr.l.google.com";
// 443 is outside the STUN port block, so it tests the "all UDP is dropped" rule too.
const STUN_SERVERS: [&str; 3] = ["stun.l.google.com:19302", "stun.cloudflare.com:3478", "stun.nextcloud.com:443"];
const STUN_COOKIE: u32 = 0x2112_A442;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass,
    Fail,
    Unknown,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Line {
    pub verdict: Verdict,
    pub text: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IpInfo {
    pub ip: String,
    pub country: String,
    pub city: String,
    pub org: String,
}

fn pick(zh: bool, zh_text: &str, en: &str) -> String {
    if zh { zh_text } else { en }.to_string()
}

fn line(verdict: Verdict, text: String) -> Line {
    Line { verdict, text }
}

// Rows in menu order: exit, DNS, WebRTC, IPv6. `report` is called as each one finishes.
pub fn check(zh: bool, tunnel_up: bool, report: impl Fn(usize, Line) + Sync) {
    let report = &report;
    thread::scope(|s| {
        s.spawn(move || {
            let info = ipinfo(None);
            let trace = fetch(TRACE_URL, false).and_then(|t| trace_ip(&t));
            report(0, judge_exit(info.as_ref(), trace.as_deref(), tunnel_up, zh));
        });
        s.spawn(move || {
            let resolver = resolver_ip(&crate::run(&[
                "/usr/bin/dig", "+short", "+time=3", "+tries=1", "TXT", RESOLVER_NAME,
            ]).1);
            let info = resolver.as_deref().and_then(|ip| ipinfo(Some(ip)));
            report(1, judge_dns(resolver.as_deref(), info.as_ref(), zh));
        });
        s.spawn(move || {
            let mapped = STUN_SERVERS.iter().find_map(|server| stun_query(server));
            let exit = mapped.and_then(|_| fetch(TRACE_URL, false)).and_then(|t| trace_ip(&t));
            report(2, judge_stun(mapped, exit.as_deref(), tunnel_up, zh));
        });
        s.spawn(move || {
            let v6 = fetch(IPV6_URL, true).map(|t| t.trim().to_string()).filter(|t| t.parse::<IpAddr>().is_ok());
            report(3, judge_ipv6(v6.as_deref(), zh));
        });
    });
}

fn fetch(url: &str, ipv6: bool) -> Option<String> {
    let family = if ipv6 { "-6" } else { "-4" };
    let (code, out, _) = crate::run(&["/usr/bin/curl", "-s", family, "--noproxy", "*", "--max-time", "6", url]);
    (code == 0 && !out.trim().is_empty()).then_some(out)
}

// ipinfo.io without a token: 50k requests a month, plenty for a button.
fn ipinfo(ip: Option<&str>) -> Option<IpInfo> {
    let url = match ip {
        Some(ip) => format!("https://ipinfo.io/{ip}/json"),
        None => "https://ipinfo.io/json".to_string(),
    };
    parse_ipinfo(&fetch(&url, false)?)
}

pub fn parse_ipinfo(text: &str) -> Option<IpInfo> {
    let ip = json_str(text, "ip")?;
    Some(IpInfo {
        ip,
        country: json_str(text, "country").unwrap_or_default(),
        city: json_str(text, "city").unwrap_or_default(),
        org: json_str(text, "org").unwrap_or_default(),
    })
}

// Enough JSON for ipinfo's flat object of string fields. Escapes other than \" are left as-is.
pub fn json_str(text: &str, key: &str) -> Option<String> {
    let rest = &text[text.find(&format!("\"{key}\""))? + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut value = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(value),
            '\\' => value.push(chars.next()?),
            _ => value.push(c),
        }
    }
    None
}

pub fn trace_ip(text: &str) -> Option<String> {
    text.lines().find_map(|l| l.strip_prefix("ip=")).map(|ip| ip.trim().to_string())
}

// `dig +short TXT o-o.myaddr.l.google.com` prints the resolver Google saw, plus an EDNS note.
pub fn resolver_ip(dig_out: &str) -> Option<String> {
    dig_out
        .split('"')
        .map(str::trim)
        .find(|token| token.parse::<IpAddr>().is_ok())
        .map(str::to_string)
}

pub fn stun_request(tid: &[u8; 12]) -> [u8; 20] {
    let mut req = [0u8; 20];
    req[0..2].copy_from_slice(&0x0001u16.to_be_bytes());
    req[4..8].copy_from_slice(&STUN_COOKIE.to_be_bytes());
    req[8..20].copy_from_slice(tid);
    req
}

// The public address a STUN server saw: exactly what WebRTC would hand to a web page.
pub fn stun_mapped(resp: &[u8], tid: &[u8; 12]) -> Option<Ipv4Addr> {
    if resp.len() < 20 || resp[0..2] != [0x01, 0x01] || &resp[8..20] != tid {
        return None;
    }
    let mut i = 20;
    while i + 4 <= resp.len() {
        let kind = u16::from_be_bytes([resp[i], resp[i + 1]]);
        let len = u16::from_be_bytes([resp[i + 2], resp[i + 3]]) as usize;
        let value = resp.get(i + 4..i + 4 + len)?;
        if (kind == 0x0020 || kind == 0x0001) && len >= 8 && value[1] == 0x01 {
            let raw = u32::from_be_bytes([value[4], value[5], value[6], value[7]]);
            return Some(Ipv4Addr::from(if kind == 0x0020 { raw ^ STUN_COOKIE } else { raw }));
        }
        i += 4 + len + (4 - len % 4) % 4;
    }
    None
}

fn stun_query(server: &str) -> Option<Ipv4Addr> {
    let addr = server.to_socket_addrs().ok()?.find(SocketAddr::is_ipv4)?;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
        ^ u64::from(std::process::id()) << 32;
    let tid: [u8; 12] = std::array::from_fn(|i| (seed >> (i % 8 * 8)) as u8 ^ i as u8);
    socket.send_to(&stun_request(&tid), addr).ok()?;
    let mut buf = [0u8; 512];
    let (n, _) = socket.recv_from(&mut buf).ok()?;
    stun_mapped(&buf[..n], &tid)
}

fn place(info: &IpInfo) -> String {
    let region = format!("{} {}", info.country, info.city).trim().to_string();
    let org: String = info.org.chars().take(40).collect();
    [info.ip.clone(), region, org]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

pub fn judge_exit(info: Option<&IpInfo>, trace: Option<&str>, tunnel_up: bool, zh: bool) -> Line {
    let shown = match (info, trace) {
        (Some(info), _) => place(info),
        (None, Some(ip)) => ip.to_string(),
        (None, None) => return line(Verdict::Unknown, pick(zh, "连不上外网，查不到出口", "no internet, exit unknown")),
    };
    if let (Some(info), Some(ip)) = (info, trace) {
        if info.ip != ip {
            let text = if zh {
                format!("两个来源不一致：{} / {ip}，有流量没走隧道", info.ip)
            } else {
                format!("sources disagree: {} / {ip}, some traffic skips the tunnel", info.ip)
            };
            return line(Verdict::Fail, text);
        }
    }
    if !tunnel_up {
        let text = if zh { format!("{shown}（隧道没开，这是真实出口）") } else { format!("{shown} (tunnel off: this is your real exit)") };
        return line(Verdict::Fail, text);
    }
    line(Verdict::Pass, shown)
}

pub fn judge_dns(resolver: Option<&str>, info: Option<&IpInfo>, zh: bool) -> Line {
    let Some(ip) = resolver else {
        return line(Verdict::Unknown, pick(zh, "解析失败，查不到解析器", "lookup failed, resolver unknown"));
    };
    let Some(info) = info else {
        let text = if zh { format!("{ip}（查不到归属）") } else { format!("{ip} (owner unknown)") };
        return line(Verdict::Unknown, text);
    };
    if info.org.starts_with(EXPECTED_DNS_ORG) {
        return line(Verdict::Pass, place(info));
    }
    let text = if zh { format!("{}，不是隧道里的 DNS，可能泄露", place(info)) } else { format!("{}, not the tunnel's DNS: possible leak", place(info)) };
    line(Verdict::Fail, text)
}

pub fn judge_stun(mapped: Option<Ipv4Addr>, exit: Option<&str>, tunnel_up: bool, zh: bool) -> Line {
    let Some(ip) = mapped else {
        return line(Verdict::Pass, pick(zh, "STUN 已拦截，网页拿不到你的 IP", "STUN blocked, pages can't learn your IP"));
    };
    if tunnel_up && exit == Some(ip.to_string().as_str()) {
        let text = if zh { format!("STUN 看到 {ip}，与出口一致") } else { format!("STUN sees {ip}, same as the exit") };
        return line(Verdict::Pass, text);
    }
    let text = if zh { format!("STUN 看到 {ip}，网页能拿到这个 IP") } else { format!("STUN sees {ip}: web pages can learn it") };
    line(Verdict::Fail, text)
}

pub fn judge_ipv6(v6: Option<&str>, zh: bool) -> Line {
    match v6 {
        None => line(Verdict::Pass, pick(zh, "没有 IPv6 出口", "no IPv6 route")),
        Some(ip) => {
            let text = if zh { format!("{ip} 直连，绕过了隧道") } else { format!("{ip} goes direct, around the tunnel") };
            line(Verdict::Fail, text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IPINFO: &str = r#"{
  "ip": "203.0.113.9",
  "city": "Sanwan",
  "region": "Miaoli",
  "country": "TW",
  "org": "AS3462 Data Communication Business Group",
  "readme": "https://ipinfo.io/missingauth"
}"#;

    fn info(ip: &str, org: &str) -> IpInfo {
        IpInfo { ip: ip.into(), country: "TW".into(), city: "Sanwan".into(), org: org.into() }
    }

    #[test]
    fn ipinfo_fields_are_read() {
        let got = parse_ipinfo(IPINFO).unwrap();
        assert_eq!(got, info("203.0.113.9", "AS3462 Data Communication Business Group"));
        assert_eq!(parse_ipinfo(r#"{"error": {"title": "Rate limit exceeded"}}"#), None);
        assert_eq!(json_str(r#"{"org":"A \"B\" C"}"#, "org"), Some(r#"A "B" C"#.to_string()));
        assert_eq!(json_str(r#"{"org": "unterminated"#, "org"), None);
    }

    #[test]
    fn trace_and_resolver_ips_are_read() {
        assert_eq!(trace_ip("fl=1\nip=203.0.113.9\nloc=TW\n"), Some("203.0.113.9".into()));
        assert_eq!(resolver_ip("\"162.158.241.33\"\n\"edns0-client-subnet was not used\"\n"), Some("162.158.241.33".into()));
        assert_eq!(resolver_ip("\"edns0-client-subnet 1.2.3.0/24\"\n\"2400:cb00::1\"\n"), Some("2400:cb00::1".into()));
        assert_eq!(resolver_ip(";; connection timed out\n"), None);
    }

    fn response(tid: &[u8; 12], kind: u16, raw: [u8; 4]) -> Vec<u8> {
        let mut r = vec![0x01, 0x01, 0x00, 0x0c];
        r.extend_from_slice(&STUN_COOKIE.to_be_bytes());
        r.extend_from_slice(tid);
        r.extend_from_slice(&kind.to_be_bytes());
        r.extend_from_slice(&[0x00, 0x08, 0x00, 0x01, 0xd4, 0x31]);
        r.extend_from_slice(&raw);
        r
    }

    #[test]
    fn stun_mapped_address_is_decoded() {
        let tid = [7u8; 12];
        let req = stun_request(&tid);
        assert_eq!(&req[0..2], &[0x00, 0x01]);
        assert_eq!(&req[8..20], &tid);
        let ip = Ipv4Addr::new(203, 0, 113, 9);
        let xored = (u32::from(ip) ^ STUN_COOKIE).to_be_bytes();
        assert_eq!(stun_mapped(&response(&tid, 0x0020, xored), &tid), Some(ip));
        assert_eq!(stun_mapped(&response(&tid, 0x0001, ip.octets()), &tid), Some(ip));
        assert_eq!(stun_mapped(&response(&[8u8; 12], 0x0020, xored), &tid), None, "someone else's reply");
        assert_eq!(stun_mapped(&response(&tid, 0x0020, xored)[..30], &tid), None, "truncated");
    }

    #[test]
    fn exit_passes_only_with_the_tunnel_up_and_sources_agreeing() {
        let tw = info("203.0.113.9", "AS3462 Chunghwa");
        let pass = judge_exit(Some(&tw), Some("203.0.113.9"), true, true);
        assert_eq!(pass.verdict, Verdict::Pass);
        assert_eq!(pass.text, "203.0.113.9 · TW Sanwan · AS3462 Chunghwa");
        assert_eq!(judge_exit(Some(&tw), Some("198.51.100.1"), true, true).verdict, Verdict::Fail);
        assert_eq!(judge_exit(Some(&tw), Some("203.0.113.9"), false, false).verdict, Verdict::Fail);
        assert_eq!(judge_exit(None, Some("203.0.113.9"), true, true).verdict, Verdict::Pass);
        assert_eq!(judge_exit(None, None, true, false).text, "no internet, exit unknown");
    }

    #[test]
    fn dns_passes_only_through_cloudflare() {
        let cf = info("162.158.241.33", "AS13335 Cloudflare, Inc.");
        assert_eq!(judge_dns(Some("162.158.241.33"), Some(&cf), true).verdict, Verdict::Pass);
        let isp = info("202.96.128.86", "AS4134 CHINANET-BACKBONE");
        let fail = judge_dns(Some("202.96.128.86"), Some(&isp), false);
        assert_eq!(fail.verdict, Verdict::Fail);
        assert!(fail.text.ends_with("not the tunnel's DNS: possible leak"));
        assert_eq!(judge_dns(Some("162.158.241.33"), None, true).verdict, Verdict::Unknown);
        assert_eq!(judge_dns(None, None, true).verdict, Verdict::Unknown);
    }

    #[test]
    fn any_stun_answer_is_a_leak_unless_it_is_the_tunnel_exit() {
        let ip = Ipv4Addr::new(203, 0, 113, 9);
        assert_eq!(judge_stun(None, None, true, true).verdict, Verdict::Pass);
        assert_eq!(judge_stun(Some(ip), Some("203.0.113.9"), true, true).verdict, Verdict::Pass);
        assert_eq!(judge_stun(Some(ip), Some("203.0.113.9"), false, true).verdict, Verdict::Fail);
        assert_eq!(judge_stun(Some(ip), Some("198.51.100.1"), true, true).verdict, Verdict::Fail);
        assert_eq!(judge_stun(Some(ip), None, true, true).verdict, Verdict::Fail);
    }

    #[test]
    fn any_ipv6_exit_is_a_leak() {
        assert_eq!(judge_ipv6(None, true).verdict, Verdict::Pass);
        assert_eq!(judge_ipv6(Some("2001:db8::1"), false).text, "2001:db8::1 goes direct, around the tunnel");
    }

    #[test]
    #[ignore = "needs the tunnel up"]
    fn live_check_passes_with_the_tunnel_up() {
        let rows = std::sync::Mutex::new(vec![None; 4]);
        check(false, true, |i, line| rows.lock().unwrap()[i] = Some(line));
        for (i, row) in rows.into_inner().unwrap().into_iter().enumerate() {
            let row = row.expect("every row reports");
            println!("{i}: {:?} {}", row.verdict, row.text);
            assert_eq!(row.verdict, Verdict::Pass, "row {i}: {}", row.text);
        }
    }
}

// The node list comes from the tun4ai CLI, never from parsing share links here:
// one parser (helper/nodes.py) decides what a node is, for root, the CLI and the tray.
// Nothing read here carries credentials; the CLI and active-node.json leave them out.

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub protocol: String,
    pub server: String,
    pub port: u16,
    pub transport: String,
    pub selected: bool,
}

impl Row {
    pub fn detail(&self) -> String {
        let udp = if self.transport == "udp" { " · UDP" } else { "" };
        format!("{} · {}:{}{udp}", label(&self.protocol), self.server, self.port)
    }
}

// What the running tunnel was built from, as render.py wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Active {
    pub name: String,
    pub protocol: String,
    pub server: String,
    pub server_name: String,
    pub port: u16,
    pub transport: String,
}

impl Active {
    // The name the TLS probe should present. Falls back to the server for nodes without SNI.
    pub fn tls_host(&self) -> String {
        if self.server_name.is_empty() { self.server.clone() } else { self.server_name.clone() }
    }
}

pub fn label(protocol: &str) -> &'static str {
    match protocol {
        "vless" => "VLESS",
        "vmess" => "VMess",
        "trojan" => "Trojan",
        "shadowsocks" => "Shadowsocks",
        "hysteria2" => "Hysteria2",
        "tuic" => "TUIC",
        _ => "Proxy",
    }
}

fn text(obj: &Value, key: &str) -> Option<String> {
    obj.get(key)?.as_str().map(str::to_string)
}

fn port(obj: &Value) -> Option<u16> {
    u16::try_from(obj.get("port")?.as_u64()?).ok()
}

// `tun4ai node list --json`. Err carries the CLI's own message, to show instead of a list.
pub fn rows_from(out: &str) -> Result<Vec<Row>, String> {
    let doc: Value = serde_json::from_str(out.trim()).map_err(|err| err.to_string())?;
    if doc.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(doc
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("tun4ai node list failed")
            .to_string());
    }
    let list = doc.get("nodes").and_then(Value::as_array).ok_or("no nodes field")?;
    Ok(list
        .iter()
        .filter_map(|n| {
            Some(Row {
                name: text(n, "name")?,
                protocol: text(n, "protocol")?,
                server: text(n, "server")?,
                port: port(n)?,
                transport: text(n, "transport")?,
                selected: n.get("selected")?.as_bool()?,
            })
        })
        .collect())
}

pub fn active_from(text_in: &str) -> Option<Active> {
    let doc: Value = serde_json::from_str(text_in).ok()?;
    Some(Active {
        name: text(&doc, "name")?,
        protocol: text(&doc, "protocol")?,
        server: text(&doc, "server")?,
        server_name: text(&doc, "server_name").unwrap_or_default(),
        port: port(&doc)?,
        transport: text(&doc, "transport")?,
    })
}

// `tun4ai node use --json`: Ok(restart_needed) or the CLI's error message.
pub fn use_result(out: &str) -> Result<bool, String> {
    let doc: Value = serde_json::from_str(out.trim()).map_err(|_| out.trim().to_string())?;
    if doc.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(doc.get("restart_needed").and_then(Value::as_bool).unwrap_or(false));
    }
    Err(doc.pointer("/error/message").and_then(Value::as_str).unwrap_or("tun4ai node use failed").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shapes copied from the CLI's real output (helper/tun4ai, schema 1).
    const LIST: &str = r#"{"ok": true, "schema": 1, "command": "node list", "file": "nodes.txt", "nodes": [{"index": 1, "name": "东京 reality", "protocol": "vless", "server": "203.0.113.7", "port": 443, "transport": "tcp", "network": "tcp", "security": "reality", "insecure": false, "selected": false}, {"index": 2, "name": "hy2", "protocol": "hysteria2", "server": "hy.example.com", "port": 8443, "transport": "udp", "network": "quic", "security": "tls", "insecure": false, "selected": true}], "selected": "hy2", "selected_error": null}"#;

    #[test]
    fn list_is_read_from_the_cli_json() {
        let rows = rows_from(LIST).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "东京 reality");
        assert!(!rows[0].selected && rows[1].selected);
        assert_eq!(rows[0].detail(), "VLESS · 203.0.113.7:443");
        assert_eq!(rows[1].detail(), "Hysteria2 · hy.example.com:8443 · UDP");
    }

    #[test]
    fn cli_errors_come_back_as_their_message() {
        let out = r#"{"ok": false, "schema": 1, "command": "node list", "error": {"code": "invalid_node_file", "message": "nodes.txt:3: unsupported link type"}}"#;
        assert_eq!(rows_from(out), Err("nodes.txt:3: unsupported link type".to_string()));
        assert!(rows_from("Traceback (most recent call last)").is_err());
    }

    #[test]
    fn active_node_falls_back_to_the_server_for_tls() {
        let with_sni = r#"{"index": 1, "name": "a", "protocol": "trojan", "server": "198.51.100.5", "port": 443, "transport": "tcp", "network": "ws", "security": "tls", "insecure": false, "selected": true, "server_ips": ["198.51.100.5"], "server_name": "cdn.example.com"}"#;
        let active = active_from(with_sni).unwrap();
        assert_eq!((active.tls_host(), active.port, active.transport.as_str()), ("cdn.example.com".to_string(), 443, "tcp"));
        let bare = with_sni.replace(r#""server_name": "cdn.example.com""#, r#""server_name": """#);
        assert_eq!(active_from(&bare).unwrap().tls_host(), "198.51.100.5");
        assert_eq!(active_from("{}"), None);
    }

    #[test]
    fn use_reports_whether_a_restart_is_needed() {
        assert_eq!(use_result(r#"{"ok": true, "restart_needed": true}"#), Ok(true));
        assert_eq!(use_result(r#"{"ok": true, "restart_needed": false}"#), Ok(false));
        assert_eq!(
            use_result(r#"{"ok": false, "error": {"code": "not_found", "message": "no node named \"x\""}}"#),
            Err("no node named \"x\"".to_string())
        );
    }
}

// The frosted panel is a SwiftUI window. This process keeps the probes and
// the script calls, and only sends the window a snapshot plus show/hide.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;

pub enum Action {
    Delay,
    Speed,
    Leak,
    Browser,
    Tunnel,
    Restart,
    Firewall(bool),
    EditNode,
    EditDirect,
    Install,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Green,
    Yellow,
    Red,
    Gray,
}

pub struct Line {
    pub text: String,
    pub tone: Tone,
}

pub struct Model {
    pub status: String,
    pub subtitle: String,
    pub status_tone: Tone,
    pub mark: String,
    pub zh: bool,
    pub icmp: String,
    pub tcp: String,
    pub real: String,
    pub speed: String,
    pub leaks: [Line; 4],
    pub tunnel_on: bool,
    pub tunnel_enabled: bool,
    pub restart_enabled: bool,
    pub firewall_on: bool,
    pub firewall_enabled: bool,
    pub delay_enabled: bool,
    pub speed_enabled: bool,
    pub leak_enabled: bool,
    pub install: String,
    pub install_enabled: bool,
    pub install_visible: bool,
}

static OPEN: AtomicBool = AtomicBool::new(false);
static HIDDEN_AT: Mutex<Option<Instant>> = Mutex::new(None);
static LAST: Mutex<Option<String>> = Mutex::new(None);
static INCOMING: OnceLock<Mutex<Vec<Action>>> = OnceLock::new();

struct Bridge {
    child: Child,
    stdin: ChildStdin,
}

static BRIDGE: Mutex<Option<Bridge>> = Mutex::new(None);

fn incoming() -> &'static Mutex<Vec<Action>> {
    INCOMING.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn poll() -> Option<Action> {
    incoming().lock().ok()?.pop()
}

pub fn is_open() -> bool {
    OPEN.load(Ordering::SeqCst)
}

pub fn toggle(_mtm: MainThreadMarker) {
    if is_open() {
        send_hide();
        return;
    }
    let just_closed = HIDDEN_AT
        .lock()
        .ok()
        .and_then(|at| *at)
        .is_some_and(|at| at.elapsed() < Duration::from_millis(350));
    if !just_closed {
        send_show();
    }
}

pub fn apply(model: &Model) {
    let line = state_line(model);
    if let Ok(mut last) = LAST.lock() {
        *last = Some(line.clone());
    }
    if is_open() {
        let _ = write_line(&line);
    }
}

fn send_show() {
    let _ = ensure();
    if let Ok(last) = LAST.lock() {
        if let Some(line) = last.clone() {
            let _ = write_line(&line);
        }
    }
    if write_line(r#"{"op":"show"}"#).is_ok() {
        OPEN.store(true, Ordering::SeqCst);
    }
}

fn send_hide() {
    let _ = write_line(r#"{"op":"hide"}"#);
    OPEN.store(false, Ordering::SeqCst);
}

fn note_hidden() {
    OPEN.store(false, Ordering::SeqCst);
    if let Ok(mut at) = HIDDEN_AT.lock() {
        *at = Some(Instant::now());
    }
}

fn ensure() -> bool {
    if let Ok(bridge) = BRIDGE.lock() {
        if bridge.is_some() {
            return true;
        }
    }
    let Some(path) = helper_path() else {
        return false;
    };
    let mut cmd = Command::new(path);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
    if let Some(log) = panel_log() {
        cmd.stderr(log);
    } else {
        cmd.stderr(Stdio::null());
    }
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    let Some(stdin) = child.stdin.take() else {
        return false;
    };
    let Some(stdout) = child.stdout.take() else {
        return false;
    };
    std::thread::spawn(move || read_actions(stdout));
    if let Ok(mut bridge) = BRIDGE.lock() {
        *bridge = Some(Bridge { child, stdin });
    }
    true
}

fn panel_log() -> Option<std::fs::File> {
    let home = std::env::var("HOME").ok()?;
    let dir = PathBuf::from(home).join("Library/Logs");
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::File::create(dir.join("tun4ai-panel.log")).ok()
}

fn read_actions(stdout: impl std::io::Read) {
    let lines = BufReader::new(stdout).lines();
    for line in lines.map_while(Result::ok) {
        match line.trim() {
            "hide" => note_hidden(),
            "delay" => push(Action::Delay),
            "speed" => push(Action::Speed),
            "leak" => push(Action::Leak),
            "browser" => push(Action::Browser),
            "tunnel" => push(Action::Tunnel),
            "restart" => push(Action::Restart),
            "firewall 1" => push(Action::Firewall(true)),
            "firewall 0" => push(Action::Firewall(false)),
            "node" => push(Action::EditNode),
            "direct" => push(Action::EditDirect),
            "install" => push(Action::Install),
            "quit" => push(Action::Quit),
            _ => {}
        }
    }
    if let Ok(mut bridge) = BRIDGE.lock() {
        *bridge = None;
    }
    note_hidden();
}

fn push(action: Action) {
    if let Ok(mut queue) = incoming().lock() {
        queue.insert(0, action);
    }
}

fn write_line(line: &str) -> std::io::Result<()> {
    if !ensure() {
        return Err(std::io::Error::other("panel helper missing"));
    }
    let mut guard = BRIDGE.lock().map_err(|_| std::io::Error::other("panel lock"))?;
    let Some(bridge) = guard.as_mut() else {
        return Err(std::io::Error::other("panel helper missing"));
    };
    if let Err(err) = writeln!(bridge.stdin, "{line}").and_then(|_| bridge.stdin.flush()) {
        let _ = bridge.child.kill();
        let _ = bridge.child.wait();
        *guard = None;
        return Err(err);
    }
    Ok(())
}

fn helper_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let beside = exe.parent()?.join("Tun4aiPanel");
    beside.is_file().then_some(beside)
}

fn tone_name(tone: Tone) -> &'static str {
    match tone {
        Tone::Green => "green",
        Tone::Yellow => "yellow",
        Tone::Red => "red",
        Tone::Gray => "gray",
    }
}

fn state_line(model: &Model) -> String {
    let leaks: Vec<String> = model
        .leaks
        .iter()
        .map(|line| format!(r#"{{"text":{},"tone":"{}"}}"#, json_string(&line.text), tone_name(line.tone)))
        .collect();
    format!(
        concat!(
            r#"{{"op":"state","title":{},"subtitle":{},"tone":"{}","mark":"{}","zh":{},"icmp":{},"tcp":{},"real":{},"speed":{},"#,
            r#""firewall":{},"tunnelOn":{},"busy":{},"delayOn":{},"speedOn":{},"restartOn":{},"firewallEnabled":{},"#,
            r#""leakOn":{},"install":{},"installOn":{},"installShow":{},"leaks":[{}]}}"#
        ),
        json_string(&model.status),
        json_string(&model.subtitle),
        tone_name(model.status_tone),
        model.mark,
        model.zh,
        json_string(&model.icmp),
        json_string(&model.tcp),
        json_string(&model.real),
        json_string(&model.speed),
        model.firewall_on,
        model.tunnel_on,
        !model.tunnel_enabled,
        model.delay_enabled,
        model.speed_enabled,
        model.restart_enabled,
        model.firewall_enabled,
        model.leak_enabled,
        json_string(&model.install),
        model.install_enabled,
        model.install_visible,
        leaks.join(",")
    )
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_json_escapes_quotes_and_names_the_mark() {
        let mut model = sample();
        model.icmp = "he said \"ok\"".into();
        let line = state_line(&model);
        assert!(line.contains(r#"he said \"ok\""#));
        assert!(line.contains(r#""title":"Tunnel connected""#));
        assert!(line.contains(r#""mark":"on""#));
        assert!(line.contains(r#""zh":false"#));
        assert!(line.starts_with(r#"{"op":"state""#));
    }

    fn sample() -> Model {
        Model {
            status: "Tunnel connected".into(),
            subtitle: "VLESS · protected for 1 min".into(),
            status_tone: Tone::Green,
            mark: "on".into(),
            zh: false,
            icmp: "not tested".into(),
            tcp: "not tested".into(),
            real: "not tested".into(),
            speed: "not tested".into(),
            leaks: [
                Line { text: "not tested".into(), tone: Tone::Gray },
                Line { text: "not tested".into(), tone: Tone::Gray },
                Line { text: "not tested".into(), tone: Tone::Gray },
                Line { text: "not tested".into(), tone: Tone::Gray },
            ],
            tunnel_on: true,
            tunnel_enabled: true,
            restart_enabled: true,
            firewall_on: true,
            firewall_enabled: true,
            delay_enabled: true,
            speed_enabled: true,
            leak_enabled: true,
            install: String::new(),
            install_enabled: false,
            install_visible: true,
        }
    }
}

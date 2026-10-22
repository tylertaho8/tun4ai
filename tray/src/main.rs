use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEventMask};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode};
#[cfg(test)]
use tray_icon::menu::NativeIcon;
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

mod leak;
mod panel;
mod setup;

// Written by helper/install-root.sh; the tray only reads them.
const HELPER: &str = "/Library/PrivilegedHelperTools/tun4ai";
const STATE: &str = "/Library/Application Support/tun4ai";
// Same targets as v2rayN's defaults.
const LOCAL_PROXY: &str = "socks5h://127.0.0.1:17897";
const PING_URL: &str = "https://www.google.com/generate_204";
const SPEED_URL: &str = "https://cachefly.cachefly.net/50mb.test";
const BROWSER_CHECK_URL: &str = "https://ippure.com/";
const ICON_SIZE: u32 = 36;
const ON_RGB: [u8; 3] = [46, 170, 110];
const OFF_RGB: [u8; 3] = [140, 140, 140];

static PENDING: Mutex<Option<String>> = Mutex::new(None);
static ZH: OnceLock<bool> = OnceLock::new();

fn zh() -> bool {
    *ZH.get_or_init(|| match std::env::var("TUN4AI_LANG") {
        Ok(lang) => lang.starts_with("zh"),
        Err(_) => {
            let out = Command::new("/usr/bin/defaults")
                .args(["read", "-g", "AppleLanguages"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            first_language(&out).is_some_and(|l| l.starts_with("zh"))
        }
    })
}

fn tr(zh_text: &'static str, en: &'static str) -> &'static str {
    if zh() { zh_text } else { en }
}

// `defaults read -g AppleLanguages` prints a plist array: ( "zh-Hans-CN", "en-CN" ).
fn first_language(out: &str) -> Option<String> {
    out.lines()
        .map(|l| l.trim().trim_end_matches(',').trim_matches('"'))
        .find(|l| !l.is_empty() && *l != "(" && *l != ")")
        .map(str::to_string)
}

fn state(name: &str) -> String {
    format!("{STATE}/{name}")
}

fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    Path::new(&home).join("Library/Application Support/tun4ai")
}

struct Probe {
    busy: bool,
    icmp: String,
    tcp: String,
    real: String,
    speed: String,
}

static PROBE: Mutex<Probe> = Mutex::new(Probe {
    busy: false,
    icmp: String::new(),
    tcp: String::new(),
    real: String::new(),
    speed: String::new(),
});

// Rows: exit, DNS, WebRTC, IPv6. None is "not tested yet", or "testing" while busy.
struct LeakState {
    busy: bool,
    rows: [Option<leak::Line>; 4],
}

static LEAK: Mutex<LeakState> = Mutex::new(LeakState { busy: false, rows: [None, None, None, None] });

struct SetupRun {
    busy: bool,
}

static SETUP: Mutex<SetupRun> = Mutex::new(SetupRun { busy: false });

#[derive(Clone, Copy, PartialEq, Eq)]
enum Goal {
    Enable,
    Disable,
    Restart,
    FwOn,
    FwOff,
}

struct Control {
    running: bool,
    next: Option<&'static str>,
    goal: Option<Goal>,
}

static CTRL: Mutex<Control> = Mutex::new(Control {
    running: false,
    next: None,
    goal: None,
});

struct Ui {
    tray: TrayIcon,
    color: Icon,
    gray: Icon,
    shown_color: Mutex<Option<bool>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dot {
    Green,
    Yellow,
    Red,
    Gray,
}

impl Dot {
    #[cfg(test)]
    fn icon(self) -> NativeIcon {
        match self {
            Dot::Green => NativeIcon::StatusAvailable,
            Dot::Yellow => NativeIcon::StatusPartiallyAvailable,
            Dot::Red => NativeIcon::StatusUnavailable,
            Dot::Gray => NativeIcon::StatusNone,
        }
    }
}



fn main() {
    let color = Icon::from_rgba(mark_rgba(ICON_SIZE, ON_RGB, true), ICON_SIZE, ICON_SIZE).expect("color icon");
    let gray = Icon::from_rgba(mark_rgba(ICON_SIZE, OFF_RGB, false), ICON_SIZE, ICON_SIZE).expect("gray icon");
    let tray = TrayIconBuilder::new()
        .with_icon(gray.clone())
        .with_icon_as_template(false)
        .with_tooltip("tun4ai")
        .build()
        .expect("status item");
    let ui = Ui { tray, color, gray, shown_color: Mutex::new(None) };
    let mtm = MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    refresh(&ui);

    loop {
        let until = NSDate::dateWithTimeIntervalSinceNow(0.25);
        if let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
            NSEventMask::Any,
            Some(&until),
            unsafe { NSDefaultRunLoopMode },
            true,
        ) {
            app.sendEvent(&event);
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click { button, button_state, .. } = event {
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    panel::toggle(mtm);
                }
            }
        }
        while let Some(action) = panel::poll() {
            handle_panel(action);
        }
        refresh(&ui);
    }
}

fn handle_panel(action: panel::Action) {
    match action {
        panel::Action::Delay => start_probe(false),
        panel::Action::Speed => start_probe(true),
        panel::Action::Leak => start_leak(),
        panel::Action::Browser => {
            let _ = Command::new("open").arg(BROWSER_CHECK_URL).spawn();
        }
        panel::Action::Tunnel => {
            let on = Path::new(&state("ENABLED")).exists() && process_alive();
            start_action(if on { "disable" } else { "enable" });
        }
        panel::Action::Restart => start_action("restart"),
        panel::Action::Firewall(on) => start_action(if on { "firewall-on" } else { "firewall-off" }),
        panel::Action::EditNode => open_config("vless.txt"),
        panel::Action::EditDirect => open_config("direct.txt"),
        panel::Action::Install => start_setup(),
        panel::Action::Quit => std::process::exit(0),
    }
}

// install.sh creates both files. If one is gone, show the folder instead of failing silently.
fn open_config(name: &str) {
    let path = config_dir().join(name);
    let target = if path.is_file() { path } else { config_dir() };
    let _ = Command::new("open").arg("-t").arg(&target).spawn();
}

fn goal_of(action: &str) -> Goal {
    match action {
        "disable" => Goal::Disable,
        "restart" => Goal::Restart,
        "firewall-on" => Goal::FwOn,
        "firewall-off" => Goal::FwOff,
        _ => Goal::Enable,
    }
}

fn start_action(action: &'static str) {
    let spawn = {
        let mut ctrl = CTRL.lock().unwrap();
        ctrl.goal = Some(goal_of(action));
        if ctrl.running {
            ctrl.next = Some(action);
            false
        } else {
            ctrl.running = true;
            true
        }
    };
    if spawn {
        thread::spawn(move || worker(action));
    }
}

fn worker(mut action: &'static str) {
    loop {
        // Restart is disable then enable. The firewall stays as it is in between.
        let (code, out, err) = if action == "restart" {
            let stopped = run_action("disable");
            if stopped.0 == 0 { run_action("enable") } else { stopped }
        } else {
            run_action(action)
        };
        let checked_as = if action == "restart" { "enable" } else { action };
        let message = failure_message(checked_as, code, &out, &err);
        let mut ctrl = CTRL.lock().unwrap();
        if let Some(next) = ctrl.next.take() {
            drop(ctrl);
            action = next;
            continue;
        }
        ctrl.running = false;
        if ctrl.goal == Some(goal_of(action)) {
            ctrl.goal = None;
        }
        drop(ctrl);
        if let Some(message) = message {
            *PENDING.lock().unwrap() = Some(message);
        }
        break;
    }
}

// The helper scripts are root-owned and listed in /etc/sudoers.d/tun4ai by install.sh.
// Nothing here ever asks for a password or runs a file from the user's home as root.
fn run_action(action: &str) -> (i32, String, String) {
    let target = format!("{HELPER}/{action}.sh");
    if !Path::new(&target).is_file() {
        return (1, String::new(), tr(
            "后台服务没有安装。请在项目目录运行 ./install.sh",
            "The background service is not installed. Run ./install.sh from the project folder.",
        ).to_string());
    }
    let result = run(&["/usr/bin/sudo", "-n", &target]);
    let refused = result.2.contains("password is required")
        || result.2.contains("not permitted")
        || result.2.contains("not allowed");
    if refused {
        return (1, String::new(), tr(
            "没有执行权限。请重新运行 ./install.sh",
            "Not authorized to run the helper. Run ./install.sh again.",
        ).to_string());
    }
    result
}

fn run(argv: &[&str]) -> (i32, String, String) {
    let mut cmd = Command::new(argv[0]);
    cmd.args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match cmd.output() {
        Ok(out) => (
            out.status.code().unwrap_or(1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ),
        Err(err) => (1, String::new(), err.to_string()),
    }
}

fn failure_message(action: &str, code: i32, out: &str, err: &str) -> Option<String> {
    let result = fs::read_to_string(state("run/last-result.txt"))
        .unwrap_or_default()
        .trim()
        .to_string();
    let enabled = Path::new(&state("ENABLED")).exists();
    let failed = code != 0 || (action == "enable" && !enabled) || result.starts_with("FAIL");
    if !failed {
        return None;
    }
    let mut message = result;
    if message.is_empty() || message == "OK" {
        message = if err.trim().is_empty() { out.trim().to_string() } else { err.trim().to_string() };
    }
    if message.is_empty() {
        message = tr("操作没有完成。", "The action did not finish.").to_string();
    }
    Some(message)
}

fn refresh(ui: &Ui) {
    if let Ok(mut pending) = PENDING.try_lock() {
        if let Some(message) = pending.take() {
            alert(&message);
        }
    }
    let enabled = Path::new(&state("ENABLED")).exists();
    let firewall_off = Path::new(&state("FIREWALL_OFF")).exists() || !Path::new(STATE).exists();
    let alive = process_alive();
    let colored = enabled && alive;
    let goal = CTRL.try_lock().ok().and_then(|ctrl| ctrl.goal);
    if let Ok(mut shown) = ui.shown_color.try_lock() {
        if *shown != Some(colored) {
            let _ = ui
                .tray
                .set_icon(Some(if colored { ui.color.clone() } else { ui.gray.clone() }));
            *shown = Some(colored);
        }
    }
    let view = status_view(colored, enabled, alive, firewall_off, goal, zh());
    let setup_busy = SETUP.try_lock().map(|s| s.busy).unwrap_or(false);
    let installed = helper_installed();
    let (title, dot) = if setup_busy {
        (tr("正在安装…", "Installing…").to_string(), Dot::Yellow)
    } else if !installed && !view.busy {
        (tr("未安装 · 点「安装」完成设置", "Not installed · choose Install to set up").to_string(), Dot::Gray)
    } else {
        (view.title, view.dot)
    };
    let _ = ui.tray.set_tooltip(Some(title.clone()));
    if panel::is_open() {
        panel::apply(&panel_model(&title, dot, view.fw_off, view.busy, installed, setup_busy, enabled, alive));
    }
}

struct StatusView {
    title: String,
    dot: Dot,
    fw_off: bool,
    busy: bool,
}

fn status_view(
    colored: bool,
    enabled: bool,
    alive: bool,
    firewall_off: bool,
    goal: Option<Goal>,
    zh: bool,
) -> StatusView {
    let pick = |zh_text: &'static str, en: &'static str| if zh { zh_text } else { en };
    let fw_off = match goal {
        Some(Goal::FwOff) => true,
        Some(Goal::FwOn) => false,
        _ => firewall_off,
    };
    let opening = matches!(goal, Some(Goal::Enable)) && !colored;
    let restarting = matches!(goal, Some(Goal::Restart));
    let closing = matches!(goal, Some(Goal::Disable)) && (colored || alive || enabled);
    let (title, dot) = if restarting {
        (pick("正在重启隧道…", "Restarting tunnel…"), Dot::Yellow)
    } else if opening {
        (pick("正在开启隧道…", "Starting tunnel…"), Dot::Yellow)
    } else if closing {
        (pick("正在关闭隧道…", "Stopping tunnel…"), Dot::Yellow)
    } else if colored && fw_off {
        (pick("隧道已连接 · 防火墙关，断了会漏网", "Tunnel up · firewall off, a drop would leak"), Dot::Yellow)
    } else if colored {
        (pick("隧道已连接 · 防火墙开", "Tunnel up · firewall on"), Dot::Green)
    } else if enabled && fw_off {
        (pick("隧道断开 · 防火墙关，会漏网", "Tunnel down · firewall off, traffic leaks"), Dot::Red)
    } else if enabled {
        (pick("隧道断开 · 外网已切断", "Tunnel down · internet blocked"), Dot::Red)
    } else if fw_off {
        (pick("隧道关 · 防火墙关，网络直连", "Tunnel off · firewall off, direct"), Dot::Gray)
    } else {
        (pick("隧道关 · 防火墙开，外网已切断", "Tunnel off · firewall on, internet blocked"), Dot::Gray)
    };
    StatusView {
        title: title.to_string(),
        dot,
        fw_off,
        busy: restarting || opening || closing,
    }
}

// Menu-bar copy of the app icon: a rounded tile, a white tunnel arch, and a
// light in the opening. y grows downward. `lit` is the tunnel-up state.
fn mark_rgba(size: u32, tile: [u8; 3], lit: bool) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let n = (SAMPLES * SAMPLES) as u32;
    let mut out = vec![0u8; (size * size * 4) as usize];
    for py in 0..size {
        for px in 0..size {
            let mut pr = 0u32;
            let mut pg = 0u32;
            let mut pb = 0u32;
            let mut pa = 0u32;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = (px as f64 + (sx as f64 + 0.5) / SAMPLES as f64) / size as f64;
                    let y = (py as f64 + (sy as f64 + 0.5) / SAMPLES as f64) / size as f64;
                    let [r, g, b, a] = mark_sample(x, y, tile, lit);
                    pr += r as u32 * a as u32;
                    pg += g as u32 * a as u32;
                    pb += b as u32 * a as u32;
                    pa += a as u32;
                }
            }
            let i = ((py * size + px) * 4) as usize;
            if pa == 0 {
                out[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
            } else {
                out[i..i + 4].copy_from_slice(&[
                    (pr / pa) as u8,
                    (pg / pa) as u8,
                    (pb / pa) as u8,
                    (pa / n) as u8,
                ]);
            }
        }
    }
    out
}

fn mark_sample(x: f64, y: f64, tile: [u8; 3], lit: bool) -> [u8; 4] {
    if in_disc(x, y, 0.50, 0.60, 0.045) {
        return if lit { [255, 219, 115, 255] } else { [226, 226, 226, 255] };
    }
    if in_arch(x, y) {
        return [255, 255, 255, 255];
    }
    if in_tile(x, y) {
        return [tile[0], tile[1], tile[2], 255];
    }
    [0, 0, 0, 0]
}

fn in_tile(x: f64, y: f64) -> bool {
    let inset = 0.04;
    let radius = 0.22;
    let (x0, y0, x1, y1) = (inset, inset, 1.0 - inset, 1.0 - inset);
    if !(x0..=x1).contains(&x) || !(y0..=y1).contains(&y) {
        return false;
    }
    let cx = if x < x0 + radius {
        x0 + radius
    } else if x > x1 - radius {
        x1 - radius
    } else {
        x
    };
    let cy = if y < y0 + radius {
        y0 + radius
    } else if y > y1 - radius {
        y1 - radius
    } else {
        y
    };
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= radius * radius
}

fn in_arch(x: f64, y: f64) -> bool {
    let spring = 0.44;
    let floor = 0.82;
    let outer = 0.34;
    let inner = 0.17;
    let dx = (x - 0.5).abs();
    if (spring..=floor).contains(&y) {
        return dx >= inner && dx <= outer;
    }
    if y < spring {
        let dy = spring - y;
        let r = (dx * dx + dy * dy).sqrt();
        return r >= inner && r <= outer;
    }
    false
}

fn in_disc(x: f64, y: f64, cx: f64, cy: f64, r: f64) -> bool {
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firewall_click_shows_the_new_state_before_the_script_finishes() {
        let view = status_view(true, true, true, false, Some(Goal::FwOff), true);
        assert_eq!(view.title, "隧道已连接 · 防火墙关，断了会漏网");
        assert_eq!(view.dot, Dot::Yellow);
        assert!(view.fw_off);
        let view = status_view(true, true, true, true, Some(Goal::FwOn), true);
        assert_eq!(view.title, "隧道已连接 · 防火墙开");
        assert_eq!(view.dot, Dot::Green);
        assert!(!view.fw_off);
    }

    #[test]
    fn tunnel_switch_does_not_pretend_it_is_already_up() {
        let view = status_view(false, false, false, false, Some(Goal::Enable), true);
        assert_eq!(view.title, "正在开启隧道…");
        assert!(view.busy);
        let view = status_view(true, true, true, false, Some(Goal::Disable), true);
        assert_eq!(view.title, "正在关闭隧道…");
        assert!(view.busy);
    }

    #[test]
    fn restart_is_busy_until_the_script_finishes() {
        let view = status_view(true, true, true, false, Some(Goal::Restart), true);
        assert_eq!(view.title, "正在重启隧道…");
        assert!(view.busy);
        assert!(!view.fw_off);
    }

    #[test]
    fn idle_states_are_gray() {
        assert_eq!(status_view(false, false, false, false, None, true).dot, Dot::Gray);
        assert_eq!(status_view(false, false, false, true, None, true).dot, Dot::Gray);
    }

    #[test]
    fn panel_headlines_follow_the_long_status() {
        assert_eq!(panel_headline("正在开启隧道…"), "正在连接");
        assert_eq!(panel_headline("正在关闭隧道…"), "正在断开");
        assert_eq!(panel_headline("隧道已连接 · 防火墙开"), "隧道已连接");
        assert_eq!(panel_headline("隧道断开 · 外网已切断"), "连接失败");
        assert_eq!(panel_mark("正在重启隧道…"), "wait");
        assert_eq!(panel_mark("隧道关 · 防火墙关，网络直连"), "off");
        assert_eq!(panel_headline("Starting tunnel…"), "Connecting");
        assert_eq!(panel_headline("Stopping tunnel…"), "Disconnecting");
        assert_eq!(panel_headline("Tunnel up · firewall on"), "Tunnel connected");
        assert_eq!(panel_headline("Tunnel down · internet blocked"), "Connection failed");
        assert_eq!(panel_headline("Tunnel off · firewall off, direct"), "Tunnel off");
        assert_eq!(panel_mark("Restarting tunnel…"), "wait");
        assert_eq!(panel_mark("Tunnel down · internet blocked"), "fail");
    }

    #[test]
    fn english_titles_match_the_same_states() {
        assert_eq!(status_view(true, true, true, false, None, false).title, "Tunnel up · firewall on");
        assert_eq!(status_view(false, true, false, false, None, false).title, "Tunnel down · internet blocked");
        assert_eq!(status_view(false, false, false, true, None, false).title, "Tunnel off · firewall off, direct");
    }

    #[test]
    fn leak_rows_show_untested_checking_and_verdicts() {
        let (text, icon) = leak_row_view(0, None, false, true);
        assert_eq!(text, "出口：未测");
        assert_eq!(icon, Dot::Gray.icon());
        assert_eq!(leak_row_view(1, None, true, false).0, "DNS: checking…");
        let fail = leak::Line { verdict: leak::Verdict::Fail, text: "2001:db8::1 goes direct, around the tunnel".into() };
        let (text, icon) = leak_row_view(3, Some(&fail), false, false);
        assert_eq!(text, "IPv6: 2001:db8::1 goes direct, around the tunnel");
        assert_eq!(icon, Dot::Red.icon());
    }

    #[test]
    fn first_language_is_read_from_the_defaults_array() {
        assert_eq!(first_language("(\n    \"zh-Hans-CN\",\n    \"en-CN\"\n)\n"), Some("zh-Hans-CN".to_string()));
        assert_eq!(first_language("(\n    en,\n    ja\n)\n"), Some("en".to_string()));
        assert_eq!(first_language(""), None);
    }

    #[test]
    #[ignore = "needs the tunnel up"]
    fn real_delay_goes_through_the_local_proxy() {
        let (best, first) = real_delay().expect("tunnel should answer generate_204");
        assert!(best > 0 && best <= first);
    }

    #[test]
    fn node_sni_and_port_come_from_vless_txt() {
        let text = "# comment\n\nvless://uuid@203.0.113.7:443?pbk=k&sid=ab&sni=Node.Example.com\n";
        assert_eq!(node_from(text), Some(("node.example.com".to_string(), 443)));
        assert_eq!(node_from("vless://uuid@hk.example.com:8443"), Some(("hk.example.com".to_string(), 8443)));
        assert_eq!(node_from("# only comments\n"), None);
        assert_eq!(node_from("https://u:p@hk.example.com"), None);
    }

    #[test]
    fn ping_summary_is_parsed() {
        let out = "--- 1.2.3.4 ping statistics ---\n4 packets transmitted, 3 packets received, 25.0% packet loss\nround-trip min/avg/max/stddev = 30.1/31.6/33.0/1.0 ms\n";
        assert_eq!(parse_ping(out, true), Some("32 ms，丢包 25.0%".to_string()));
        assert_eq!(parse_ping(out, false), Some("32 ms, 25.0% loss".to_string()));
        assert_eq!(parse_ping("4 packets transmitted, 0 packets received, 100.0% packet loss\n", true), None);
    }

    #[test]
    fn proxy_ip_comes_from_the_kill_switch_table() {
        let conf = "x\ntable <proxy4> persist { 203.0.113.7 }\ny\n";
        assert_eq!(proxy_ip_from(conf), Some("203.0.113.7".to_string()));
        assert_eq!(proxy_ip_from("table <direct4> persist { 10.0.0.0/8 }\n"), None);
    }

    #[test]
    fn icon_matches_the_app_mark() {
        let size = ICON_SIZE as usize;
        let px = mark_rgba(ICON_SIZE, ON_RGB, true);
        let at = |x: usize, y: usize| {
            let i = (y * size + x) * 4;
            (px[i], px[i + 1], px[i + 2], px[i + 3])
        };
        assert_eq!(px.len(), size * size * 4);
        assert_eq!(at(0, 0).3, 0, "corner is transparent");
        let crown = at(size / 2, size * 18 / 100);
        assert!(crown.0 > 240 && crown.3 == 255, "crown of the arch is white");
        let opening = at(size / 2, size * 70 / 100);
        assert_eq!(&opening.0, &ON_RGB[0], "opening shows the green tile");
        let leg = at(size * 76 / 100, size * 65 / 100);
        assert!(leg.0 > 240 && leg.3 == 255, "a leg of the arch is white");
        let light = at(size / 2, size * 60 / 100);
        assert!(light.0 > 240 && light.2 < 160, "the light is yellow");
    }
}

fn panel_model(
    title: &str,
    dot: Dot,
    firewall_off: bool,
    busy: bool,
    installed: bool,
    setup_busy: bool,
    enabled: bool,
    alive: bool,
) -> panel::Model {
    let blank = || tr("未测", "not tested").to_string();
    let (probe_busy, icmp, tcp, real, speed) = match PROBE.try_lock() {
        Ok(probe) => (probe.busy, shown(&probe.icmp, blank()), shown(&probe.tcp, blank()), shown(&probe.real, blank()), shown(&probe.speed, blank())),
        Err(_) => (true, blank(), blank(), blank(), blank()),
    };
    let (leak_busy, leaks) = match LEAK.try_lock() {
        Ok(state) => {
            let lines = std::array::from_fn(|i| match state.rows[i].as_ref() {
                Some(line) => panel::Line { text: line.text.clone(), tone: tone_of(verdict_dot(line.verdict)) },
                None if state.busy => panel::Line { text: tr("检测中…", "checking…").to_string(), tone: panel::Tone::Gray },
                None => panel::Line { text: blank(), tone: panel::Tone::Gray },
            });
            (state.busy, lines)
        }
        Err(_) => (true, std::array::from_fn(|_| panel::Line { text: blank(), tone: panel::Tone::Gray })),
    };
    let install = if setup_busy {
        tr("正在安装…", "Installing…")
    } else if installed {
        tr("重新安装（保留节点和设置）", "Reinstall (keeps your node and settings)")
    } else {
        tr("安装（下载 sing-box，需要管理员密码）", "Install (downloads sing-box, needs your password)")
    };
    let tunnel_on = enabled && alive;
    panel::Model {
        status: panel_headline(title),
        subtitle: panel_subtitle(title, tunnel_on, firewall_off),
        status_tone: tone_of(dot),
        mark: panel_mark(title).to_string(),
        zh: zh(),
        icmp,
        tcp,
        real,
        speed,
        leaks,
        tunnel_on,
        tunnel_enabled: !busy && installed,
        restart_enabled: !busy && installed && (enabled || alive),
        firewall_on: !firewall_off,
        firewall_enabled: installed && !setup_busy,
        delay_enabled: !probe_busy,
        speed_enabled: !probe_busy,
        leak_enabled: !leak_busy,
        install: install.to_string(),
        install_enabled: !setup_busy,
        install_visible: true,
    }
}

fn panel_headline(status: &str) -> String {
    let en = status.starts_with("Tunnel")
        || status.starts_with("Starting")
        || status.starts_with("Stopping")
        || status.starts_with("Restarting");
    if status.starts_with("正在关闭") || status.starts_with("Stopping") {
        return if en { "Disconnecting" } else { "正在断开" }.into();
    }
    if status.starts_with("正在") || status.starts_with("Starting") || status.starts_with("Restarting") {
        return if en { "Connecting" } else { "正在连接" }.into();
    }
    if status.contains("已连接") || status.starts_with("Tunnel up") {
        return if en { "Tunnel connected" } else { "隧道已连接" }.into();
    }
    if status.contains("断开") || status.starts_with("Tunnel down") {
        return if en { "Connection failed" } else { "连接失败" }.into();
    }
    if en { "Tunnel off" } else { "隧道未连接" }.into()
}

fn panel_mark(status: &str) -> &'static str {
    if status.starts_with("正在") || status.starts_with("Stopping") || status.starts_with("Starting") || status.starts_with("Restarting") {
        "wait"
    } else if status.contains("已连接") || status.starts_with("Tunnel up") {
        "on"
    } else if status.contains("断开") || status.starts_with("Tunnel down") {
        "fail"
    } else {
        "off"
    }
}

fn panel_subtitle(status: &str, tunnel_on: bool, firewall_off: bool) -> String {
    if tunnel_on && firewall_off {
        return tr("防火墙已关闭，断开后会漏网", "Firewall is off, a drop would leak").into();
    }
    if tunnel_on {
        return format!("VLESS · {}", protected_for());
    }
    if let Some((_, rest)) = status.split_once('·') {
        return rest.trim().to_string();
    }
    if firewall_off {
        tr("网络直连", "Direct").into()
    } else {
        tr("外网已切断", "Internet blocked").into()
    }
}

fn protected_for() -> String {
    let Ok(meta) = fs::metadata(state("ENABLED")) else {
        return tr("已保护", "protected").into();
    };
    let Ok(modified) = meta.modified() else {
        return tr("已保护", "protected").into();
    };
    let secs = modified.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    if zh() {
        if secs < 60 {
            "已保护不到 1 分钟".into()
        } else if secs < 3600 {
            format!("已保护 {} 分钟", secs / 60)
        } else {
            let hours = secs / 3600;
            let minutes = (secs % 3600) / 60;
            if minutes == 0 {
                format!("已保护 {hours} 小时")
            } else {
                format!("已保护 {hours}小时{minutes}分钟")
            }
        }
    } else if secs < 60 {
        "protected for under a minute".into()
    } else if secs < 3600 {
        format!("protected for {} min", secs / 60)
    } else {
        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        if minutes == 0 {
            format!("protected for {hours} h")
        } else {
            format!("protected for {hours} h {minutes} min")
        }
    }
}

fn shown(value: &str, fallback: String) -> String {
    if value.is_empty() { fallback } else { value.to_string() }
}

fn tone_of(dot: Dot) -> panel::Tone {
    match dot {
        Dot::Green => panel::Tone::Green,
        Dot::Yellow => panel::Tone::Yellow,
        Dot::Red => panel::Tone::Red,
        Dot::Gray => panel::Tone::Gray,
    }
}

fn verdict_dot(verdict: leak::Verdict) -> Dot {
    match verdict {
        leak::Verdict::Pass => Dot::Green,
        leak::Verdict::Fail => Dot::Red,
        leak::Verdict::Unknown => Dot::Yellow,
    }
}

fn set_probe(update: impl FnOnce(&mut Probe)) {
    update(&mut PROBE.lock().unwrap());
}

#[cfg(test)]
fn leak_row_view(index: usize, row: Option<&leak::Line>, busy: bool, zh: bool) -> (String, NativeIcon) {
    let pick = |zh_text: &'static str, en: &'static str| if zh { zh_text } else { en };
    let label = [
        pick("出口：", "Exit: "),
        pick("DNS：", "DNS: "),
        pick("WebRTC：", "WebRTC: "),
        pick("IPv6：", "IPv6: "),
    ][index];
    match row {
        Some(line) => {
            let icon = match line.verdict {
                leak::Verdict::Pass => Dot::Green,
                leak::Verdict::Fail => Dot::Red,
                leak::Verdict::Unknown => Dot::Yellow,
            };
            (format!("{label}{}", line.text), icon.icon())
        }
        None if busy => (format!("{label}{}", pick("检测中…", "checking…")), Dot::Gray.icon()),
        None => (format!("{label}{}", pick("未测", "not checked")), Dot::Gray.icon()),
    }
}

fn start_leak() {
    {
        let mut state = LEAK.lock().unwrap();
        if state.busy {
            return;
        }
        state.busy = true;
        state.rows = [None, None, None, None];
    }
    thread::spawn(|| {
        let tunnel_up = Path::new(&state("ENABLED")).exists() && process_alive();
        leak::check(zh(), tunnel_up, |i, line| LEAK.lock().unwrap().rows[i] = Some(line));
        LEAK.lock().unwrap().busy = false;
    });
}

fn helper_installed() -> bool {
    Path::new(&format!("{HELPER}/bin/sing-box")).is_file()
}

fn start_setup() {
    {
        let mut setup = SETUP.lock().unwrap();
        if setup.busy {
            return;
        }
        setup.busy = true;
    }
    thread::spawn(|| {
        if let Err(message) = perform_setup() {
            *PENDING.lock().unwrap() = Some(message);
        }
        SETUP.lock().unwrap().busy = false;
    });
}

// Download and check sing-box, copy the example config, then ask for the admin password once.
fn perform_setup() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let resources = setup::resources_of(&exe).ok_or_else(|| tr(
        "找不到安装资源。请从发布页下载 tun4ai.app",
        "App resources are missing. Download tun4ai.app from the releases page.",
    ))?;
    let bundled = setup::bundle(&resources).ok_or_else(|| tr(
        "安装包不完整。请重新下载 tun4ai.app",
        "This app bundle is incomplete. Download tun4ai.app again.",
    ))?;
    setup::prepare_config(&bundled.examples, &config_dir()).map_err(|err| {
        format!("{} {err}", tr("写配置目录失败：", "Could not create the config folder:"))
    })?;
    let work = std::env::temp_dir().join(format!("tun4ai-setup-{}", std::process::id()));
    let _cleanup = Cleanup(work.clone());
    fs::create_dir_all(&work).map_err(|err| err.to_string())?;
    let fetch = bundled.fetch.to_str().ok_or("path")?;
    let dest = work.to_str().ok_or("path")?;
    let (code, _out, err) = run(&[fetch, dest]);
    if code != 0 {
        let detail = err.trim();
        return Err(if detail.is_empty() {
            tr("sing-box 下载或校验失败", "sing-box download or checksum failed").to_string()
        } else {
            detail.to_string()
        });
    }
    let sing_box = work.join("sing-box");
    let owner = run(&["/usr/bin/id", "-un"]).1.trim().to_string();
    if owner.is_empty() {
        return Err(tr("读不到当前用户名", "Could not read the current user name").to_string());
    }
    let prompt = tr("tun4ai 需要安装后台服务", "tun4ai needs to install its background service");
    let script = setup::admin_command(
        &bundled.install_root,
        &owner,
        &config_dir(),
        &sing_box,
        if zh() { "zh" } else { "en" },
        prompt,
    );
    let (code, out, err) = run(&["/usr/bin/osascript", "-e", &script]);
    if code != 0 {
        let detail = if err.trim().is_empty() { out } else { err };
        if detail.contains("User canceled") || detail.contains("-128") {
            return Err(tr("已取消安装", "Install canceled").to_string());
        }
        let detail = detail.trim();
        return Err(if detail.is_empty() {
            tr("安装没有完成", "Install did not finish").to_string()
        } else {
            detail.to_string()
        });
    }
    Ok(())
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Delay test: ICMP, TCPing and real delay. Speed test: real delay first, then download only if it worked.
// That is v2rayN's order for its mixed test.
fn start_probe(speed: bool) {
    {
        let mut probe = PROBE.lock().unwrap();
        if probe.busy {
            return;
        }
        probe.busy = true;
    }
    thread::spawn(move || {
        let testing = tr("测试中…", "testing…");
        if !speed {
            let no_server = tr("找不到服务器地址", "server address unknown");
            set_probe(|p| {
                p.icmp = testing.into();
                p.tcp = tr("等待", "waiting").into();
            });
            let ip = proxy_ip();
            let icmp = match &ip {
                Some(ip) => icmp_delay(ip),
                None => no_server.into(),
            };
            set_probe(|p| {
                p.icmp = icmp;
                p.tcp = testing.into();
            });
            let tcp = match (&ip, node_server()) {
                (Some(ip), Some(node)) => tls_delay(ip, &node),
                (_, None) => tr("没有配置节点", "no node configured").into(),
                (None, _) => no_server.into(),
            };
            set_probe(|p| p.tcp = tcp);
        }
        set_probe(|p| p.real = testing.into());
        let real = real_delay();
        set_probe(|p| {
            p.real = match real {
                Some((best, first)) => {
                    if zh() { format!("{best} ms（首次 {first} ms）") } else { format!("{best} ms (first {first} ms)") }
                }
                None => tr("超时", "timed out").into(),
            }
        });
        if speed {
            let text = if real.is_some() {
                set_probe(|p| p.speed = tr("连接中…", "connecting…").into());
                download_speed()
            } else {
                tr("跳过：真实延迟不通", "skipped: real latency failed").into()
            };
            set_probe(|p| p.speed = text);
        }
        set_probe(|p| p.busy = false);
    });
}

fn node_server() -> Option<(String, u16)> {
    node_from(&fs::read_to_string(config_dir().join("vless.txt")).ok()?)
}

// SNI and port for the TLS probe. The UUID and keys are never read here.
fn node_from(text: &str) -> Option<(String, u16)> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))?;
    let rest = line.strip_prefix("vless://")?;
    let (authority, query) = rest.split_once('?').unwrap_or((rest, ""));
    let host_port = authority.rsplit('@').next()?;
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().ok()?),
        None => (host_port, 443),
    };
    let sni = query
        .split('&')
        .find_map(|part| part.strip_prefix("sni="))
        .filter(|s| !s.is_empty())
        .unwrap_or(host);
    (!sni.is_empty()).then(|| (sni.to_lowercase(), port))
}

fn proxy_ip() -> Option<String> {
    proxy_ip_from(&fs::read_to_string(state("pf.killswitch.conf")).ok()?)
}

fn proxy_ip_from(conf: &str) -> Option<String> {
    let line = conf.lines().find(|l| l.starts_with("table <proxy4>"))?;
    let inner = line.split('{').nth(1)?.split('}').next()?;
    inner
        .split(',')
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(str::to_string)
}

fn icmp_delay(ip: &str) -> String {
    let (_, out, _) = run(&["/sbin/ping", "-q", "-n", "-c", "4", "-i", "0.2", "-t", "4", ip]);
    parse_ping(&out, zh()).unwrap_or_else(|| tr("不通", "unreachable").into())
}

fn parse_ping(out: &str, zh: bool) -> Option<String> {
    let loss = out
        .lines()
        .find(|l| l.contains("packet loss"))?
        .split(", ")
        .find(|part| part.ends_with("packet loss"))?
        .split_whitespace()
        .next()?
        .to_string();
    let avg: f64 = out
        .lines()
        .find(|l| l.contains("min/avg/max"))?
        .split('=')
        .nth(1)?
        .trim()
        .split('/')
        .nth(1)?
        .parse()
        .ok()?;
    Some(if zh { format!("{avg:.0} ms，丢包 {loss}") } else { format!("{avg:.0} ms, {loss} loss") })
}

// The hotspot's transparent proxy answers TCP SYNs itself, so a bare connect only measures the LAN.
// The server's TLS reply has to come from the server, so time the handshake instead.
fn tls_delay(ip: &str, node: &(String, u16)) -> String {
    let (host, port) = node;
    let resolve = format!("{host}:{port}:{ip}");
    let url = format!("https://{host}:{port}/");
    let mut best: Option<u32> = None;
    for _ in 0..3 {
        let (_, out, _) = run(&[
            "/usr/bin/curl", "-s", "-I", "-o", "/dev/null", "--noproxy", "*", "--max-time", "5",
            "--resolve", &resolve, "-w", "%{time_appconnect}", &url,
        ]);
        let secs: f64 = out.trim().parse().unwrap_or(0.0);
        if secs > 0.0 {
            let ms = (secs * 1000.0).round() as u32;
            best = Some(best.map_or(ms, |b| b.min(ms)));
        }
    }
    best.map_or_else(|| tr("超时", "timed out").into(), |ms| format!("{ms} ms"))
}

// v2rayN: two GETs on one client, keep the faster; one more round after 500 ms if both fail.
fn real_delay() -> Option<(u32, u32)> {
    for attempt in 0..2 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(500));
        }
        let (_, out, _) = run(&[
            "/usr/bin/curl", "-s", "--max-time", "5", "-x", LOCAL_PROXY,
            "-o", "/dev/null", "-o", "/dev/null",
            "-w", "%{http_code} %{time_total}\n", PING_URL, PING_URL,
        ]);
        let times: Vec<u32> = out
            .lines()
            .filter_map(|line| {
                let (code, secs) = line.split_once(' ')?;
                let secs: f64 = secs.trim().parse().ok()?;
                (code != "000").then(|| (secs * 1000.0).round() as u32)
            })
            .collect();
        if let Some(&best) = times.iter().min() {
            return Some((best, times[0]));
        }
    }
    None
}

// v2rayN: 10 second download through the local proxy, report the peak per-second rate in MB/s.
// The average is shown too, because the peak flatters bursty links.
fn download_speed() -> String {
    let child = Command::new("/usr/bin/curl")
        .args(["-s", "--max-time", "10", "-x", LOCAL_PROXY, SPEED_URL])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let failed = tr("curl 启动失败", "curl failed to start");
    let Ok(mut child) = child else {
        return failed.into();
    };
    let Some(mut out) = child.stdout.take() else {
        return failed.into();
    };
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    let mut first: Option<Instant> = None;
    let mut last = Instant::now();
    let mut window_start = Instant::now();
    let mut window_bytes: u64 = 0;
    let mut peak: f64 = 0.0;
    loop {
        let n = match out.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        last = Instant::now();
        if first.is_none() {
            first = Some(last);
            window_start = last;
        }
        total += n as u64;
        window_bytes += n as u64;
        let span = last.duration_since(window_start).as_secs_f64();
        if span >= 1.0 {
            let rate = window_bytes as f64 / span;
            peak = peak.max(rate);
            window_start = last;
            window_bytes = 0;
            set_probe(|p| p.speed = format!("{} {:.1} MB/s", tr("测速中", "measuring"), rate / 1e6));
        }
    }
    let _ = child.wait();
    let Some(start) = first else {
        return tr("没有收到数据", "no data received").into();
    };
    let secs = last.duration_since(start).as_secs_f64().max(0.001);
    let avg = total as f64 / secs;
    let peak = if peak > 0.0 { peak } else { avg };
    let (peak, avg, used) = (peak / 1e6, avg / 1e6, total as f64 / 1e6);
    if zh() {
        format!("峰值 {peak:.1} MB/s，平均 {avg:.1} MB/s，用了 {used:.1} MB")
    } else {
        format!("peak {peak:.1} MB/s, average {avg:.1} MB/s, used {used:.1} MB")
    }
}

fn process_alive() -> bool {
    let Ok(text) = fs::read_to_string(state("run/sing-box.pid")) else {
        return false;
    };
    let Ok(pid) = text.trim().parse::<i32>() else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    let rc = unsafe { libc_kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(3)
}

unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    kill(pid, sig)
}

fn alert(text: &str) {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    let _ = Command::new("/usr/bin/osascript")
        .args(["-e", &format!("display alert \"tun4ai\" message \"{escaped}\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

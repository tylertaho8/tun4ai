use std::cell::Cell;
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
use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
use tray_icon::menu::{
    CheckMenuItem, IconMenuItem, Menu, MenuEvent, MenuItem, NativeIcon, PredefinedMenuItem,
};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

mod leak;

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
    status: IconMenuItem,
    tunnel: MenuItem,
    restart: MenuItem,
    firewall: CheckMenuItem,
    icmp: MenuItem,
    tcp: MenuItem,
    real: MenuItem,
    speed: MenuItem,
    test_delay: MenuItem,
    test_speed: MenuItem,
    leak_rows: [IconMenuItem; 4],
    test_leak: MenuItem,
    color: Icon,
    gray: Icon,
    shown_color: Mutex<Option<bool>>,
    shown_title: Mutex<String>,
    shown_probe: Mutex<[String; 4]>,
    shown_leak: Mutex<[String; 4]>,
    shown_dot: Cell<Option<Dot>>,
    shown_tunnel_on: Cell<Option<bool>>,
    shown_fw_checked: Cell<Option<bool>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dot {
    Green,
    Yellow,
    Red,
    Gray,
}

impl Dot {
    fn icon(self) -> NativeIcon {
        match self {
            Dot::Green => NativeIcon::StatusAvailable,
            Dot::Yellow => NativeIcon::StatusPartiallyAvailable,
            Dot::Red => NativeIcon::StatusUnavailable,
            Dot::Gray => NativeIcon::StatusNone,
        }
    }
}

fn cmd(code: Code) -> Option<Accelerator> {
    Some(Accelerator::new(Some(Modifiers::SUPER), code))
}

fn main() {
    let color = Icon::from_rgba(shield_rgba(ICON_SIZE, ON_RGB), ICON_SIZE, ICON_SIZE).expect("color icon");
    let gray = Icon::from_rgba(shield_rgba(ICON_SIZE, OFF_RGB), ICON_SIZE, ICON_SIZE).expect("gray icon");
    // The status line stays clickable so the dot keeps its color; clicking it opens the log.
    let status = IconMenuItem::with_native_icon(tr("正在读取…", "Loading…"), true, Some(NativeIcon::StatusNone), None);
    let icmp = MenuItem::new("", false, None);
    let tcp = MenuItem::new("", false, None);
    let real = MenuItem::new("", false, None);
    let speed = MenuItem::new("", false, None);
    let test_delay = MenuItem::new(tr("测延迟", "Test latency"), true, cmd(Code::KeyT));
    let test_speed = MenuItem::new(tr("测速（最多耗 50MB 流量）", "Test speed (uses up to 50 MB)"), true, None);
    // Enabled like the status line, so the dots keep their color. Clicking them does nothing.
    let leak_rows: [IconMenuItem; 4] =
        std::array::from_fn(|_| IconMenuItem::with_native_icon("", true, Some(NativeIcon::StatusNone), None));
    let test_leak = MenuItem::new(tr("检测泄露（出口 / DNS / WebRTC / IPv6）", "Check for leaks (exit / DNS / WebRTC / IPv6)"), true, cmd(Code::KeyL));
    let browser_check = MenuItem::new(tr("在浏览器中检测（ippure.com）…", "Check in browser (ippure.com)…"), true, None);
    let tunnel = MenuItem::new(tr("开启隧道", "Start tunnel"), true, cmd(Code::KeyO));
    let restart = MenuItem::new(tr("重启隧道", "Restart tunnel"), false, cmd(Code::KeyR));
    let firewall = CheckMenuItem::new(
        tr("防火墙（隧道断了就断网）", "Firewall (block internet if the tunnel drops)"),
        true,
        false,
        None,
    );
    let edit = MenuItem::new(tr("编辑直连名单…", "Edit direct list…"), true, cmd(Code::KeyE));
    let edit_node = MenuItem::new(tr("编辑 VLESS 节点…", "Edit VLESS node…"), true, cmd(Code::KeyN));
    let quit = MenuItem::new(tr("退出菜单栏程序", "Quit menu bar app"), true, cmd(Code::KeyQ));
    let menu = Menu::new();
    let separator = PredefinedMenuItem::separator;
    menu.append_items(&[
        &status,
        &separator(),
        &icmp,
        &tcp,
        &real,
        &speed,
        &test_delay,
        &test_speed,
        &separator(),
        &leak_rows[0],
        &leak_rows[1],
        &leak_rows[2],
        &leak_rows[3],
        &test_leak,
        &browser_check,
        &separator(),
        &tunnel,
        &restart,
        &firewall,
        &separator(),
        &edit_node,
        &edit,
        &separator(),
        &quit,
    ])
    .ok();

    let status_id = status.id().clone();
    let tunnel_id = tunnel.id().clone();
    let restart_id = restart.id().clone();
    let firewall_id = firewall.id().clone();
    let test_delay_id = test_delay.id().clone();
    let test_speed_id = test_speed.id().clone();
    let test_leak_id = test_leak.id().clone();
    let browser_check_id = browser_check.id().clone();
    let edit_id = edit.id().clone();
    let edit_node_id = edit_node.id().clone();
    let quit_id = quit.id().clone();

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(gray.clone())
        .with_icon_as_template(false)
        .build()
        .expect("status item");

    let ui = Ui {
        tray,
        status,
        tunnel,
        restart,
        firewall,
        icmp,
        tcp,
        real,
        speed,
        test_delay,
        test_speed,
        color,
        gray,
        shown_color: Mutex::new(None),
        shown_title: Mutex::new(String::new()),
        shown_probe: Mutex::new(Default::default()),
        shown_dot: Cell::new(None),
        shown_tunnel_on: Cell::new(None),
        shown_fw_checked: Cell::new(None),
    };
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
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == tunnel_id {
                start_action(if ui.shown_tunnel_on.get() == Some(true) { "disable" } else { "enable" });
            } else if event.id == restart_id {
                start_action("restart");
            } else if event.id == firewall_id {
                // The item has already flipped its own check mark.
                start_action(if ui.firewall.is_checked() { "firewall-on" } else { "firewall-off" });
                ui.shown_fw_checked.set(None);
            } else if event.id == status_id {
                let _ = Command::new("open")
                    .args(["-a", "Console", &state("run/supervise.log")])
                    .spawn();
            } else if event.id == test_delay_id {
                start_probe(false);
            } else if event.id == test_speed_id {
                start_probe(true);
            } else if event.id == edit_id {
                open_config("direct.txt");
            } else if event.id == edit_node_id {
                open_config("vless.txt");
            } else if event.id == quit_id {
                std::process::exit(0);
            }
        }
        refresh(&ui);
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
    refresh_probe(ui);
    let view = status_view(colored, enabled, alive, firewall_off, goal, zh());
    if let Ok(mut shown_title) = ui.shown_title.try_lock() {
        if *shown_title != view.title {
            ui.status.set_text(&view.title);
            let _ = ui.tray.set_tooltip(Some(view.title.clone()));
            *shown_title = view.title.clone();
        }
    }
    if ui.shown_dot.get() != Some(view.dot) {
        ui.status.set_native_icon(Some(view.dot.icon()));
        ui.shown_dot.set(Some(view.dot));
    }
    let tunnel_on = colored && !view.busy;
    if ui.shown_tunnel_on.get() != Some(tunnel_on) {
        ui.tunnel.set_text(if tunnel_on { tr("关闭隧道", "Stop tunnel") } else { tr("开启隧道", "Start tunnel") });
        ui.shown_tunnel_on.set(Some(tunnel_on));
    }
    ui.tunnel.set_enabled(!view.busy);
    ui.restart.set_enabled(!view.busy && (enabled || alive));
    if ui.shown_fw_checked.get() != Some(!view.fw_off) {
        ui.firewall.set_checked(!view.fw_off);
        ui.shown_fw_checked.set(Some(!view.fw_off));
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

// A shield with a tunnel arch cut out, drawn here so the repo ships no image assets.
fn shield_rgba(size: u32, rgb: [u8; 3]) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let mut out = vec![0u8; (size * size * 4) as usize];
    for py in 0..size {
        for px in 0..size {
            let mut hits = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = (px as f64 + (sx as f64 + 0.5) / SAMPLES as f64) / size as f64;
                    let y = (py as f64 + (sy as f64 + 0.5) / SAMPLES as f64) / size as f64;
                    if in_shield(x, y) && !in_tunnel(x, y) {
                        hits += 1;
                    }
                }
            }
            let i = ((py * size + px) * 4) as usize;
            out[i..i + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], (hits * 255 / (SAMPLES * SAMPLES)) as u8]);
        }
    }
    out
}

fn in_shield(x: f64, y: f64) -> bool {
    let (top, waist, tip, half, corner) = (0.08, 0.55, 0.94, 0.40, 0.10);
    let dx = (x - 0.5).abs();
    if !(top..=tip).contains(&y) {
        return false;
    }
    if y <= waist {
        if y < top + corner && dx > half - corner {
            let (cx, cy) = (half - corner, top + corner);
            return (dx - cx).powi(2) + (y - cy).powi(2) <= corner * corner;
        }
        return dx <= half;
    }
    let k = (y - waist) / (tip - waist);
    dx <= half * (1.0 - k * k).sqrt()
}

fn in_tunnel(x: f64, y: f64) -> bool {
    let (spring, floor, r) = (0.45, 0.74, 0.15);
    let dx = (x - 0.5).abs();
    ((spring..=floor).contains(&y) && dx <= r) || (y < spring && dx * dx + (y - spring).powi(2) <= r * r)
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
    fn english_titles_match_the_same_states() {
        assert_eq!(status_view(true, true, true, false, None, false).title, "Tunnel up · firewall on");
        assert_eq!(status_view(false, true, false, false, None, false).title, "Tunnel down · internet blocked");
        assert_eq!(status_view(false, false, false, true, None, false).title, "Tunnel off · firewall off, direct");
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
    fn icon_is_a_shield_with_a_see_through_tunnel() {
        let size = ICON_SIZE as usize;
        let px = shield_rgba(ICON_SIZE, ON_RGB);
        let alpha = |x: usize, y: usize| px[(y * size + x) * 4 + 3];
        assert_eq!(px.len(), size * size * 4);
        assert_eq!(alpha(0, 0), 0, "corner is transparent");
        assert_eq!(alpha(size / 2, size * 65 / 100), 0, "tunnel is cut out");
        assert_eq!(alpha(size / 5, size * 40 / 100), 255, "shield body is solid");
        assert_eq!(alpha(size / 2, size * 85 / 100), 255, "below the tunnel is solid");
    }
}

fn refresh_probe(ui: &Ui) {
    let Ok(probe) = PROBE.try_lock() else {
        return;
    };
    let shown_or = |value: &str| if value.is_empty() { tr("未测", "not tested").to_string() } else { value.to_string() };
    let want = [
        format!("{}{}", tr("ICMP 延迟：", "ICMP latency: "), shown_or(&probe.icmp)),
        format!("{}{}", tr("TCPing（TLS 握手）：", "TCPing (TLS handshake): "), shown_or(&probe.tcp)),
        format!("{}{}", tr("真实延迟：", "Real latency: "), shown_or(&probe.real)),
        format!("{}{}", tr("下载速度：", "Download speed: "), shown_or(&probe.speed)),
    ];
    let busy = probe.busy;
    drop(probe);
    ui.test_delay.set_enabled(!busy);
    ui.test_speed.set_enabled(!busy);
    let Ok(mut shown) = ui.shown_probe.try_lock() else {
        return;
    };
    let items = [&ui.icmp, &ui.tcp, &ui.real, &ui.speed];
    for (i, text) in want.into_iter().enumerate() {
        if shown[i] != text {
            items[i].set_text(&text);
            shown[i] = text;
        }
    }
}

fn set_probe(update: impl FnOnce(&mut Probe)) {
    update(&mut PROBE.lock().unwrap());
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

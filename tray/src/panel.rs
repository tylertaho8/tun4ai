// Menu-bar panel. Rows keep updating while it is open.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol};
use objc2::{define_class, extern_methods, msg_send, sel, AllocAnyThread, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBorderType, NSButton, NSCellImagePosition, NSColor, NSControl, NSControlStateValue,
    NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventMask, NSFont, NSImage, NSPanel,
    NSScreen, NSScrollView, NSSwitch, NSTextAlignment, NSTextField, NSTrackingArea, NSTrackingAreaOptions,
    NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowCollectionBehavior, NSWindowStyleMask, NSPopUpMenuWindowLevel,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

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

/// Fixed captions. Values that change live are in `Model`.
pub struct Labels {
    pub title: &'static str,
    pub delay: &'static str,
    pub icmp: &'static str,
    pub tcp: &'static str,
    pub real: &'static str,
    pub speed: &'static str,
    pub test_delay: &'static str,
    pub test_speed: &'static str,
    pub leaks: &'static str,
    pub leak_names: [&'static str; 4],
    pub test_leak: &'static str,
    pub browser: &'static str,
    pub tunnel: &'static str,
    pub restart: &'static str,
    pub firewall: &'static str,
    pub edit_node: &'static str,
    pub edit_direct: &'static str,
    pub quit: &'static str,
}

pub struct Model {
    pub status: String,
    pub status_tone: Tone,
    pub icmp: String,
    pub tcp: String,
    pub real: String,
    pub speed: String,
    pub leaks: [Line; 4],
    pub tunnel: String,
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

const WIDTH: f64 = 332.0;

struct Fields {
    panel: Retained<NSPanel>,
    status: Retained<NSTextField>,
    dot: Retained<NSView>,
    icmp: Retained<NSTextField>,
    tcp: Retained<NSTextField>,
    real: Retained<NSTextField>,
    speed: Retained<NSTextField>,
    leaks: [Retained<NSTextField>; 4],
    tunnel: Retained<NSButton>,
    restart: Retained<NSButton>,
    firewall: Retained<NSSwitch>,
    delay: Retained<NSButton>,
    speed_button: Retained<NSButton>,
    leak_button: Retained<NSButton>,
    install: Retained<NSButton>,
    edit_node: Retained<NSButton>,
    edit_direct: Retained<NSButton>,
    quit: Retained<NSButton>,
    glass: Retained<NSView>,
    scroll: Retained<NSScrollView>,
    doc_h: f64,
    edit_node_top: f64,
    edit_direct_top: f64,
    quit_top: f64,
    install_span: f64,
    // Buttons do not retain their target.
    _target: Retained<Target>,
}

fn channel() -> &'static Sender<Action> {
    static TX: OnceLock<Sender<Action>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let _ = RX.set(std::sync::Mutex::new(rx));
        tx
    })
}

static RX: OnceLock<std::sync::Mutex<Receiver<Action>>> = OnceLock::new();

std::thread_local! {
    static SUPPRESS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn suppressed() -> bool {
    SUPPRESS.with(|flag| flag.get())
}

fn set_suppressed(value: bool) {
    SUPPRESS.with(|flag| flag.set(value));
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "Tun4aiPanelTarget"]
    struct Target;

    impl Target {
        #[unsafe(method(onClick:))]
        fn on_click(&self, sender: &NSControl) {
            if suppressed() {
                return;
            }
            let action = match sender.tag() {
                1 => Action::Delay,
                2 => Action::Speed,
                3 => Action::Leak,
                4 => Action::Browser,
                5 => Action::Tunnel,
                6 => Action::Restart,
                7 => Action::Firewall(sender_on(sender)),
                8 => Action::EditNode,
                9 => Action::EditDirect,
                10 => Action::Install,
                11 => Action::Quit,
                _ => return,
            };
            let _ = channel().send(action);
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {}

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            hover_tag(tag_at(event));
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            hover_tag(0);
        }
    }
);

impl Target {
    extern_methods!(
        #[unsafe(method(new))]
        fn new(mtm: MainThreadMarker) -> Retained<Self>;
    );
}

fn set_row_hover(view: &NSView, on: bool) {
    view.setWantsLayer(true);
    unsafe {
        let layer: *mut AnyObject = msg_send![view, layer];
        if layer.is_null() {
            return;
        }
        let _: () = msg_send![layer, setCornerRadius: 8.0_f64];
        let _: () = msg_send![layer, setMasksToBounds: true];
        if on {
            let color = NSColor::labelColor().colorWithAlphaComponent(0.10);
            let cg = color.CGColor();
            let _: () = msg_send![layer, setBackgroundColor: &*cg];
        } else {
            let _: () = msg_send![layer, setBackgroundColor: std::ptr::null::<AnyObject>()];
        }
    }
}

fn hover_tag(tag: isize) {
    FIELDS.with(|fields| {
        let fields = fields.borrow();
        let Some(fields) = fields.as_ref() else { return };
        for button in [
            &fields.delay,
            &fields.speed_button,
            &fields.leak_button,
            &fields.tunnel,
            &fields.restart,
            &fields.install,
            &fields.edit_node,
            &fields.edit_direct,
            &fields.quit,
        ] {
            set_row_hover(button, button.tag() == tag && tag != 0 && button.isEnabled());
        }
    });
}

fn sender_on(sender: &NSControl) -> bool {
    let state: NSControlStateValue = unsafe { msg_send![sender, state] };
    state == NSControlStateValueOn
}

fn ns(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

fn tone_color(tone: Tone) -> Retained<NSColor> {
    match tone {
        Tone::Green => NSColor::systemGreenColor(),
        Tone::Yellow => NSColor::systemYellowColor(),
        Tone::Red => NSColor::systemRedColor(),
        Tone::Gray => NSColor::secondaryLabelColor(),
    }
}

fn paint(view: &NSView, color: &NSColor, radius: f64) {
    view.setWantsLayer(true);
    unsafe {
        let layer: *mut AnyObject = msg_send![&*view, layer];
        if !layer.is_null() {
            let cg = color.CGColor();
            let _: () = msg_send![layer, setCornerRadius: radius];
            let _: () = msg_send![layer, setBackgroundColor: &*cg];
        }
    }
}

fn clear_cell(view: &NSView) {
    unsafe {
        let cell: *mut AnyObject = msg_send![view, cell];
        if !cell.is_null() {
            let _: () = msg_send![cell, setDrawsBackground: false];
        }
    }
}

fn caption(mtm: MainThreadMarker, text: &str, size: f64, bold: bool) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&ns(text), mtm);
    let font = if bold { NSFont::boldSystemFontOfSize(size) } else { NSFont::systemFontOfSize(size) };
    let color = if bold { NSColor::labelColor() } else { NSColor::secondaryLabelColor() };
    field.setFont(Some(&font));
    field.setTextColor(Some(&color));
    field.setMaximumNumberOfLines(1);
    clear_cell(&field);
    field
}

fn value_field(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = caption(mtm, "", 13.0, true);
    let color = NSColor::labelColor();
    field.setAlignment(NSTextAlignment::Right);
    field.setTextColor(Some(&color));
    field
}

fn symbol(name: &str) -> Option<Retained<NSImage>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), None)?;
    image.setSize(NSSize { width: 15.0, height: 15.0 });
    image.setTemplate(true);
    Some(image)
}

fn button(mtm: MainThreadMarker, title: &str, symbol_name: &str, tag: isize, target: &Target) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&ns(&row_title(title)), Some(target), Some(sel!(onClick:)), mtm)
    };
    button.setBordered(false);
    button.setAlignment(NSTextAlignment::Left);
    button.setTag(tag);
    button.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    clear_cell(&button);
    if let Some(image) = symbol(symbol_name) {
        button.setImage(Some(&image));
        button.setImagePosition(NSCellImagePosition::ImageLeading);
        button.setImageHugsTitle(true);
    }
    button
}

fn tag_at(event: &NSEvent) -> isize {
    let Some(mtm) = MainThreadMarker::new() else { return 0 };
    let Some(window) = event.window(mtm) else { return 0 };
    let Some(content) = window.contentView() else { return 0 };
    let mut view: *mut NSView = unsafe { msg_send![&*content, hitTest: event.locationInWindow()] };
    while !view.is_null() {
        let tag: isize = unsafe { msg_send![view, tag] };
        if tag != 0 {
            return tag;
        }
        view = unsafe { msg_send![view, superview] };
    }
    0
}

fn row_title(title: &str) -> String {
    if title.is_empty() { String::new() } else { format!("  {title}    ›") }
}

struct Cursor {
    top: f64,
}

impl Cursor {
    fn take(&mut self, height: f64) -> f64 {
        let here = self.top;
        self.top += height;
        here
    }
}

fn place(view: &NSView, x: f64, top: f64, width: f64, height: f64, doc_h: f64) {
    view.setFrame(NSRect {
        origin: NSPoint { x, y: doc_h - top - height },
        size: NSSize { width, height },
    });
}

fn build(mtm: MainThreadMarker, labels: &Labels) -> Fields {
    let _ = channel();
    let target = Target::new(mtm);
    let mut cursor = Cursor { top: 12.0 };
    let inner = WIDTH - 24.0;

    let title = caption(mtm, labels.title, 16.0, true);
    let title_top = cursor.take(24.0);
    let dot = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect { origin: NSPoint { x: 0.0, y: 0.0 }, size: NSSize { width: 8.0, height: 8.0 } },
    );
    paint(&dot, &NSColor::secondaryLabelColor(), 4.0);
    let status = caption(mtm, "", 12.0, false);
    let status_top = cursor.take(20.0);
    cursor.take(10.0);

    let delay_head = caption(mtm, labels.delay, 11.0, false);
    let delay_head_top = cursor.take(16.0);
    let icmp_name = caption(mtm, labels.icmp, 12.0, false);
    let icmp = value_field(mtm);
    let icmp_top = cursor.take(20.0);
    let tcp_name = caption(mtm, labels.tcp, 12.0, false);
    let tcp = value_field(mtm);
    let tcp_top = cursor.take(20.0);
    let real_name = caption(mtm, labels.real, 12.0, false);
    let real = value_field(mtm);
    let real_top = cursor.take(20.0);
    let speed_name = caption(mtm, labels.speed, 12.0, false);
    let speed = value_field(mtm);
    let speed_top = cursor.take(20.0);
    cursor.take(6.0);
    let delay = button(mtm, labels.test_delay, "timer", 1, &target);
    let delay_top = cursor.take(36.0);
    let speed_button = button(mtm, labels.test_speed, "speedometer", 2, &target);
    let speed_button_top = cursor.take(36.0);
    cursor.take(8.0);

    let leak_head = caption(mtm, labels.leaks, 11.0, false);
    let leak_head_top = cursor.take(16.0);
    let mut leak_names = Vec::new();
    let mut leaks = Vec::new();
    let mut leak_tops = Vec::new();
    for name in labels.leak_names {
        leak_names.push(caption(mtm, name, 12.0, false));
        leaks.push(value_field(mtm));
        leak_tops.push(cursor.take(20.0));
    }
    cursor.take(6.0);
    let leak_button = button(mtm, labels.test_leak, "checkmark.shield", 3, &target);
    let leak_button_top = cursor.take(36.0);
    let browser = button(mtm, labels.browser, "globe", 4, &target);
    let browser_top = cursor.take(36.0);
    cursor.take(8.0);

    let tunnel_head = caption(mtm, labels.tunnel, 11.0, false);
    let tunnel_head_top = cursor.take(16.0);
    let tunnel = button(mtm, "", "power", 5, &target);
    let tunnel_top = cursor.take(36.0);
    let restart = button(mtm, labels.restart, "arrow.clockwise", 6, &target);
    let restart_top = cursor.take(36.0);
    let firewall_name = caption(mtm, labels.firewall, 13.0, false);
    let label_color = NSColor::labelColor();
    firewall_name.setTextColor(Some(&label_color));
    let firewall = NSSwitch::new(mtm);
    unsafe {
        firewall.setTarget(Some(&target));
        firewall.setAction(Some(sel!(onClick:)));
    }
    firewall.setTag(7);
    let firewall_top = cursor.take(28.0);
    let install = button(mtm, "", "arrow.down.circle", 10, &target);
    let install_top = cursor.take(36.0);
    cursor.take(8.0);

    let edit_node = button(mtm, labels.edit_node, "link", 8, &target);
    let edit_node_top = cursor.take(36.0);
    let edit_direct = button(mtm, labels.edit_direct, "list.bullet", 9, &target);
    let edit_direct_top = cursor.take(36.0);
    let quit = button(mtm, labels.quit, "rectangle.portrait.and.arrow.right", 11, &target);
    quit.setTitle(&ns(&format!("  {}", labels.quit)));
    let quit_top = cursor.take(36.0);
    let doc_h = cursor.top + 8.0;

    let doc = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect { origin: NSPoint { x: 0.0, y: 0.0 }, size: NSSize { width: WIDTH, height: doc_h } },
    );
    let names = [
        &icmp_name, &tcp_name, &real_name, &speed_name,
    ];
    let name_tops = [icmp_top, tcp_top, real_top, speed_top];
    let values: [&NSTextField; 4] = [&icmp, &tcp, &real, &speed];
    doc.addSubview(&title);
    doc.addSubview(&dot);
    doc.addSubview(&status);
    doc.addSubview(&delay_head);
    doc.addSubview(&icmp_name);
    doc.addSubview(&icmp);
    doc.addSubview(&tcp_name);
    doc.addSubview(&tcp);
    doc.addSubview(&real_name);
    doc.addSubview(&real);
    doc.addSubview(&speed_name);
    doc.addSubview(&speed);
    doc.addSubview(&delay);
    doc.addSubview(&speed_button);
    doc.addSubview(&leak_head);
    doc.addSubview(&leak_button);
    doc.addSubview(&browser);
    doc.addSubview(&tunnel_head);
    doc.addSubview(&tunnel);
    doc.addSubview(&restart);
    doc.addSubview(&firewall_name);
    doc.addSubview(&firewall);
    doc.addSubview(&install);
    doc.addSubview(&edit_node);
    doc.addSubview(&edit_direct);
    doc.addSubview(&quit);
    let area = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            NSRect { origin: NSPoint { x: 0.0, y: 0.0 }, size: NSSize { width: WIDTH, height: doc_h } },
            NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect,
            Some(&*target),
            None,
        )
    };
    doc.addTrackingArea(&area);
    for (name, field) in leak_names.iter().zip(leaks.iter()) {
        doc.addSubview(name);
        doc.addSubview(field);
    }
    let _ = (names, name_tops, values);

    place(&title, 16.0, title_top, inner - 4.0, 24.0, doc_h);
    place(&dot, 16.0, status_top + 5.0, 8.0, 8.0, doc_h);
    place(&status, 30.0, status_top, inner - 18.0, 18.0, doc_h);
    place(&delay_head, 16.0, delay_head_top, inner, 16.0, doc_h);
    for (index, top) in [icmp_top, tcp_top, real_top, speed_top].into_iter().enumerate() {
        let name = [&icmp_name, &tcp_name, &real_name, &speed_name][index];
        let value = [&icmp, &tcp, &real, &speed][index];
        place(name, 20.0, top, 100.0, 20.0, doc_h);
        place(value, 124.0, top, inner - 112.0, 20.0, doc_h);
    }
    place(&delay, 10.0, delay_top, WIDTH - 20.0, 32.0, doc_h);
    place(&speed_button, 10.0, speed_button_top, WIDTH - 20.0, 32.0, doc_h);
    place(&leak_head, 16.0, leak_head_top, inner, 16.0, doc_h);
    for (index, top) in leak_tops.iter().copied().enumerate() {
        place(&leak_names[index], 20.0, top, 78.0, 20.0, doc_h);
        place(&leaks[index], 102.0, top, inner - 90.0, 20.0, doc_h);
    }
    place(&leak_button, 10.0, leak_button_top, WIDTH - 20.0, 32.0, doc_h);
    place(&browser, 10.0, browser_top, WIDTH - 20.0, 32.0, doc_h);
    place(&tunnel_head, 16.0, tunnel_head_top, inner, 16.0, doc_h);
    place(&tunnel, 10.0, tunnel_top, WIDTH - 20.0, 32.0, doc_h);
    place(&restart, 10.0, restart_top, WIDTH - 20.0, 32.0, doc_h);
    place(&firewall_name, 16.0, firewall_top + 6.0, inner - 70.0, 20.0, doc_h);
    place(&firewall, WIDTH - 16.0 - 48.0, firewall_top + 4.0, 48.0, 24.0, doc_h);
    place(&install, 10.0, install_top, WIDTH - 20.0, 32.0, doc_h);
    place(&edit_node, 10.0, edit_node_top, WIDTH - 20.0, 32.0, doc_h);
    place(&edit_direct, 10.0, edit_direct_top, WIDTH - 20.0, 32.0, doc_h);
    place(&quit, 10.0, quit_top, WIDTH - 20.0, 32.0, doc_h);

    let visible_h = visible_limit(mtm).min(doc_h);
    let glass = make_glass(
        mtm,
        NSRect { origin: NSPoint { x: 0.0, y: 0.0 }, size: NSSize { width: WIDTH, height: visible_h } },
    );
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect { origin: NSPoint { x: 0.0, y: 0.0 }, size: NSSize { width: WIDTH, height: visible_h } },
    );
    scroll.setBorderType(NSBorderType::NoBorder);
    scroll.setDrawsBackground(false);
    scroll.setHasVerticalScroller(doc_h > visible_h);
    scroll.setAutohidesScrollers(true);
    scroll.setDocumentView(Some(&doc));
    glass.addSubview(&scroll);

    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        NSRect { origin: NSPoint { x: 0.0, y: 0.0 }, size: NSSize { width: WIDTH, height: visible_h } },
        NSWindowStyleMask(NSWindowStyleMask::Borderless.0 | NSWindowStyleMask::NonactivatingPanel.0 | NSWindowStyleMask::FullSizeContentView.0),
        NSBackingStoreType::Buffered,
        false,
    );
    panel.setAcceptsMouseMovedEvents(true);
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(true);
    panel.setLevel(NSPopUpMenuWindowLevel);
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setHidesOnDeactivate(false);
    panel.setMovableByWindowBackground(false);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces.0
                | NSWindowCollectionBehavior::Transient.0
                | NSWindowCollectionBehavior::FullScreenAuxiliary.0
                | NSWindowCollectionBehavior::IgnoresCycle.0,
        ),
    );
    panel.setContentView(Some(&glass));

    let monitor = RcBlock::new(|_event: std::ptr::NonNull<NSEvent>| click_outside());
    let mask = NSEventMask(NSEventMask::LeftMouseDown.0 | NSEventMask::RightMouseDown.0);
    if let Some(kept) = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &monitor) {
        std::mem::forget(kept);
    }
    std::mem::forget(monitor);

    Fields {
        panel,
        status,
        dot,
        icmp,
        tcp,
        real,
        speed,
        leaks: leaks.try_into().ok().expect("four leak rows"),
        tunnel,
        restart,
        firewall,
        delay,
        speed_button,
        leak_button,
        install,
        edit_node,
        edit_direct,
        quit,
        glass,
        scroll,
        doc_h,
        edit_node_top,
        edit_direct_top,
        quit_top,
        install_span: edit_node_top - install_top,
        _target: target,
    }
}

const RADIUS: f64 = 14.0;

// Corner radius is applied on the glass view. A layer mask leaves the window backing visible.
fn make_glass(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSView> {
    unsafe {
        if let Some(cls) = AnyClass::get(c"NSGlassEffectView") {
            let allocated: *mut AnyObject = msg_send![cls, alloc];
            let view: *mut AnyObject = msg_send![allocated, initWithFrame: frame];
            let view = Retained::from_raw(view.cast::<NSView>()).expect("glass");
            let _: () = msg_send![&*view, setStyle: 0_isize];
            let _: () = msg_send![&*view, setCornerRadius: RADIUS];
            return view;
        }
    }
    let effect = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
    effect.setMaterial(NSVisualEffectMaterial::Popover);
    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect.setState(NSVisualEffectState::Active);
    unsafe {
        if effect.respondsToSelector(sel!(setCornerRadius:)) {
            let _: () = msg_send![&*effect, setCornerRadius: RADIUS];
        }
    }
    unsafe { Retained::cast(effect) }
}

fn visible_limit(mtm: MainThreadMarker) -> f64 {
    NSScreen::mainScreen(mtm).map(|screen| screen.visibleFrame().size.height - 12.0).unwrap_or(700.0)
}

std::thread_local! {
    static FIELDS: std::cell::RefCell<Option<Fields>> = const { std::cell::RefCell::new(None) };
    static OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static HIDDEN_AT: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

pub fn poll() -> Option<Action> {
    let _ = channel();
    RX.get()?.lock().ok()?.try_recv().ok()
}

pub fn is_open() -> bool {
    OPEN.with(|open| open.get())
}

pub fn toggle(mtm: MainThreadMarker, labels: &Labels) {
    ensure(mtm, labels);
    if is_open() {
        hide();
        return;
    }
    // The mouse-down that hits the icon can hide the panel before mouse-up asks
    // to open it. A click that just closed it must not open it again.
    let just_closed = HIDDEN_AT.with(|at| at.get().is_some_and(|at| at.elapsed() < Duration::from_millis(350)));
    if !just_closed {
        show(mtm);
    }
}

fn ensure(mtm: MainThreadMarker, labels: &Labels) {
    FIELDS.with(|fields| {
        if fields.borrow().is_none() {
            *fields.borrow_mut() = Some(build(mtm, labels));
        }
    });
}

fn show(mtm: MainThreadMarker) {
    FIELDS.with(|fields| {
        let fields = fields.borrow();
        let Some(fields) = fields.as_ref() else { return };
        let mouse = NSEvent::mouseLocation();
        let size = fields.panel.frame().size;
        let screen = NSScreen::mainScreen(mtm);
        let visible = screen.as_ref().map(|screen| screen.visibleFrame());
        let mut x = mouse.x - size.width / 2.0;
        let mut y = mouse.y - size.height - 4.0;
        if let Some(visible) = visible {
            let min_x = visible.origin.x + 8.0;
            let max_x = visible.origin.x + visible.size.width - size.width - 8.0;
            x = x.clamp(min_x, max_x.max(min_x));
            if y < visible.origin.y {
                y = mouse.y + 8.0;
            }
        }
        fields.panel.setFrameOrigin(NSPoint { x, y });
        fields.panel.orderFrontRegardless();
        OPEN.with(|open| open.set(true));
    });
}

// A click on the status icon is in the menu bar. That click toggles the panel
// on mouse-up; hiding it here would make the next click only reopen it.
fn click_outside() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    if !is_open() {
        return;
    }
    let mouse = NSEvent::mouseLocation();
    if let Some(screen) = NSScreen::mainScreen(mtm) {
        let visible = screen.visibleFrame();
        if mouse.y >= visible.origin.y + visible.size.height - 1.0 {
            return;
        }
    }
    let inside = FIELDS.with(|fields| {
        let fields = fields.borrow();
        let Some(fields) = fields.as_ref() else { return false };
        let frame = fields.panel.frame();
        mouse.x >= frame.origin.x
            && mouse.x <= frame.origin.x + frame.size.width
            && mouse.y >= frame.origin.y
            && mouse.y <= frame.origin.y + frame.size.height
    });
    if !inside {
        hide();
    }
}

fn hide() {
    FIELDS.with(|fields| {
        if let Some(fields) = fields.borrow().as_ref() {
            fields.panel.orderOut(None);
        }
    });
    OPEN.with(|open| open.set(false));
    HIDDEN_AT.with(|at| at.set(Some(Instant::now())));
}

pub fn apply(model: &Model) {
    set_suppressed(true);
    FIELDS.with(|fields| {
        let fields = fields.borrow();
        let Some(fields) = fields.as_ref() else { return };
        set_text(&fields.status, &model.status);
        paint(&fields.dot, &tone_color(model.status_tone), 4.0);
        set_text(&fields.icmp, &model.icmp);
        set_text(&fields.tcp, &model.tcp);
        set_text(&fields.real, &model.real);
        set_text(&fields.speed, &model.speed);
        for (field, line) in fields.leaks.iter().zip(model.leaks.iter()) {
            set_text(field, &line.text);
            let color = tone_color(line.tone);
            field.setTextColor(Some(&color));
            field.setToolTip(Some(&ns(&line.text)));
        }
        set_button(&fields.tunnel, &model.tunnel, model.tunnel_enabled);
        fields.restart.setEnabled(model.restart_enabled);
        fields.delay.setEnabled(model.delay_enabled);
        fields.speed_button.setEnabled(model.speed_enabled);
        fields.leak_button.setEnabled(model.leak_enabled);
        set_button(&fields.install, &model.install, model.install_enabled);
        fields.install.setHidden(!model.install_visible);
        let shift = if model.install_visible { 0.0 } else { fields.install_span };
        let row = NSSize { width: WIDTH - 20.0, height: 32.0 };
        place(&fields.edit_node, 10.0, fields.edit_node_top - shift, row.width, row.height, fields.doc_h);
        place(&fields.edit_direct, 10.0, fields.edit_direct_top - shift, row.width, row.height, fields.doc_h);
        place(&fields.quit, 10.0, fields.quit_top - shift, row.width, row.height, fields.doc_h);
        hug_bottom(&fields, shift);
        let want = if model.firewall_on { NSControlStateValueOn } else { NSControlStateValueOff };
        if fields.firewall.state() != want {
            fields.firewall.setState(want);
        }
        fields.firewall.setEnabled(model.firewall_enabled);
    });
    set_suppressed(false);
}

fn hug_bottom(fields: &Fields, shift: f64) {
    let height = (fields.doc_h - shift).max(1.0);
    let frame = fields.panel.frame();
    if (frame.size.height - height).abs() > 0.5 {
        fields.panel.setFrame_display(
            NSRect {
                origin: NSPoint { x: frame.origin.x, y: frame.origin.y + frame.size.height - height },
                size: NSSize { width: WIDTH, height },
            },
            true,
        );
    }
    let bounds = NSRect { origin: NSPoint { x: 0.0, y: 0.0 }, size: NSSize { width: WIDTH, height } };
    fields.glass.setFrame(bounds);
    fields.scroll.setFrame(bounds);
    let clip = fields.scroll.contentView();
    clip.scrollToPoint(NSPoint { x: 0.0, y: shift });
    fields.scroll.reflectScrolledClipView(&clip);
}

fn set_text(field: &NSTextField, text: &str) {
    field.setStringValue(&ns(text));
}

fn set_button(button: &NSButton, title: &str, enabled: bool) {
    button.setTitle(&ns(&row_title(title)));
    button.setEnabled(enabled);
}

import AppKit
import Darwin
import SwiftUI

private let panelWidth: CGFloat = 320
private let panelRadius: CGFloat = 14

enum PanelTab {
    case delay
    case leak
}

struct LeakLine: Identifiable, Equatable {
    let id: Int
    var text: String
    var tone: String
}

final class PanelModel: ObservableObject {
    @Published var title = "隧道未连接"
    @Published var subtitle = ""
    @Published var icmp = "未测"
    @Published var tcp = "未测"
    @Published var real = "未测"
    @Published var speed = "未测"
    @Published var firewall = false
    @Published var firewallEnabled = true
    @Published var tunnelOn = false
    @Published var busy = false
    @Published var delayOn = true
    @Published var speedOn = true
    @Published var restartOn = true
    @Published var leakOn = true
    @Published var leaks: [LeakLine] = []
    @Published var install = ""
    @Published var installOn = false
    @Published var installShow = false
    @Published var zh = true
    @Published var mark = "off"
    @Published var tab = PanelTab.delay
    @Published var confirmOff = false

    func t(_ zhText: String, _ en: String) -> String {
        zh ? zhText : en
    }

    func send(_ line: String) {
        fputs(line + "\n", stdout)
        fflush(stdout)
    }

    func apply(_ obj: [String: Any]) {
        title = string(obj, "title") ?? title
        subtitle = string(obj, "subtitle") ?? subtitle
        icmp = string(obj, "icmp") ?? icmp
        tcp = string(obj, "tcp") ?? tcp
        real = string(obj, "real") ?? real
        speed = string(obj, "speed") ?? speed
        firewall = bool(obj, "firewall") ?? firewall
        firewallEnabled = bool(obj, "firewallEnabled") ?? firewallEnabled
        tunnelOn = bool(obj, "tunnelOn") ?? tunnelOn
        busy = bool(obj, "busy") ?? busy
        delayOn = bool(obj, "delayOn") ?? delayOn
        speedOn = bool(obj, "speedOn") ?? speedOn
        restartOn = bool(obj, "restartOn") ?? restartOn
        leakOn = bool(obj, "leakOn") ?? leakOn
        install = string(obj, "install") ?? install
        installOn = bool(obj, "installOn") ?? installOn
        installShow = bool(obj, "installShow") ?? installShow
        zh = bool(obj, "zh") ?? zh
        mark = string(obj, "mark") ?? mark
        if let rows = obj["leaks"] as? [[String: Any]] {
            leaks = rows.enumerated().map { index, row in
                LeakLine(
                    id: index,
                    text: row["text"] as? String ?? "未测",
                    tone: row["tone"] as? String ?? "gray"
                )
            }
        }
    }
}

private func string(_ obj: [String: Any], _ key: String) -> String? {
    obj[key] as? String
}

private func bool(_ obj: [String: Any], _ key: String) -> Bool? {
    obj[key] as? Bool
}

final class PanelController: NSObject {
    let model: PanelModel
    let panel: NSPanel
    let host: NSHostingView<PanelRoot>

    override init() {
        let model = PanelModel()
        self.model = model
        host = NSHostingView(rootView: PanelRoot(model: model))
        panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: panelWidth, height: 440),
            styleMask: [.borderless, .nonactivatingPanel, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        super.init()
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.level = .popUpMenu
        panel.isFloatingPanel = true
        panel.hidesOnDeactivate = false
        panel.becomesKeyOnlyIfNeeded = true
        panel.isMovableByWindowBackground = true
        panel.acceptsMouseMovedEvents = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        panel.isReleasedWhenClosed = false
        host.wantsLayer = true
        host.layer?.backgroundColor = NSColor.clear.cgColor
        host.layer?.isOpaque = false
        panel.contentView = host
        NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [weak self] _ in
            self?.clickOutside()
        }
    }

    func handle(_ data: Data) {
        guard let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        switch obj["op"] as? String {
        case "show":
            fit()
            showNearMouse()
        case "hide":
            conceal()
        case "state":
            model.apply(obj)
            if panel.isVisible {
                fit()
            }
        default:
            break
        }
    }

    func fit() {
        host.layoutSubtreeIfNeeded()
        var size = host.fittingSize
        size.width = panelWidth
        guard size.height > 80 else { return }
        var frame = panel.frame
        let growth = size.height - frame.height
        frame.size = size
        frame.origin.y -= growth
        if let screen = panel.screen ?? NSScreen.main {
            let visible = screen.visibleFrame
            if frame.maxY > visible.maxY - 8 {
                frame.origin.y = visible.maxY - frame.height - 8
            }
            if frame.minY < visible.minY + 8 {
                frame.origin.y = visible.minY + 8
            }
        }
        panel.setFrame(frame, display: true)
        host.frame.size = size
    }

    func showNearMouse() {
        let mouse = NSEvent.mouseLocation
        let screen = NSScreen.screens.first { $0.frame.contains(mouse) } ?? NSScreen.main
        let visible = screen?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1200, height: 800)
        var frame = panel.frame
        frame.origin.x = min(max(mouse.x - frame.width / 2, visible.minX + 8), max(visible.minX + 8, visible.maxX - frame.width - 8))
        frame.origin.y = mouse.y - frame.height - 8
        if frame.minY < visible.minY {
            frame.origin.y = mouse.y + 8
        }
        panel.setFrameOrigin(frame.origin)
        panel.alphaValue = 0
        panel.orderFrontRegardless()
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.16
            panel.animator().alphaValue = 1
        }
    }

    func conceal() {
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.12
            panel.animator().alphaValue = 0
        } completionHandler: { [weak self] in
            self?.panel.orderOut(nil)
        }
    }

    func clickOutside() {
        guard panel.isVisible else { return }
        let mouse = NSEvent.mouseLocation
        if let screen = NSScreen.main, mouse.y >= screen.visibleFrame.maxY - 1 {
            return
        }
        if panel.frame.contains(mouse) {
            return
        }
        conceal()
        model.send("hide")
    }
}

struct PanelRoot: View {
    @ObservedObject var model: PanelModel
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            header
            tabSwitch
            if model.tab == .delay {
                delayCard
            } else {
                leakCard
            }
            if model.tab == .delay {
                delayButtons
            } else {
                leakButtons
            }
            controls
        }
        .padding(10)
        .frame(width: panelWidth)
        .fixedSize(horizontal: true, vertical: true)
        .background {
            RoundedRectangle(cornerRadius: panelRadius, style: .continuous)
                .fill(.ultraThinMaterial)
                .overlay {
                    RoundedRectangle(cornerRadius: panelRadius, style: .continuous)
                        .strokeBorder(.white.opacity(colorScheme == .dark ? 0.14 : 0.5), lineWidth: 0.6)
                }
        }
        .onChange(of: model.tab) { _ in
            DispatchQueue.main.async {
                PanelBridge.shared?.fit()
            }
        }
        .confirmationDialog(
            model.t("防火墙保护开着。断开隧道后将无法访问外网。", "The firewall is on. Disconnecting blocks the internet."),
            isPresented: $model.confirmOff,
            titleVisibility: .visible
        ) {
            Button(model.t("断开", "Disconnect"), role: .destructive) { model.send("tunnel") }
            Button(model.t("取消", "Cancel"), role: .cancel) {}
        }
    }

    private var header: some View {
        HStack(alignment: .center, spacing: 8) {
            statusMark
            VStack(alignment: .leading, spacing: 1) {
                Text(model.title)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text(model.subtitle)
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer(minLength: 4)
            circleButton("xmark") { model.send("hide"); PanelBridge.shared?.conceal() }
        }
    }

    private var tabSwitch: some View {
        HStack(spacing: 2) {
            tabButton(model.t("延迟", "Latency"), .delay)
            tabButton(model.t("泄露", "Leaks"), .leak)
        }
        .padding(2)
        .background {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(Color.primary.opacity(colorScheme == .dark ? 0.1 : 0.06))
        }
    }

    private func tabButton(_ title: String, _ value: PanelTab) -> some View {
        let on = model.tab == value
        return Text(title)
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(on ? Color.primary : Color.secondary)
            .frame(maxWidth: .infinity)
            .frame(height: 22)
            .background {
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .fill(on ? Color(nsColor: .windowBackgroundColor).opacity(colorScheme == .dark ? 0.55 : 1) : Color.clear)
                    .shadow(color: .black.opacity(on ? 0.12 : 0), radius: on ? 1.2 : 0, y: on ? 0.5 : 0)
            }
            .contentShape(Rectangle())
            .onTapGesture { model.tab = value }
    }

    @ViewBuilder
    private var statusMark: some View {
        let connected = model.mark == "on"
        let failing = model.mark == "fail"
        let working = model.mark == "wait"
        ZStack {
            Circle().fill(markColor.opacity(connected || failing || working ? 1 : 0.18))
            if working {
                ProgressView().controlSize(.mini).tint(.white)
            } else {
                Image(systemName: connected ? "checkmark" : (failing ? "xmark" : "minus"))
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(connected || failing ? .white : markColor)
            }
        }
        .frame(width: 28, height: 28)
    }

    private var markColor: Color {
        switch model.mark {
        case "on": return .green
        case "fail": return .red
        case "wait": return .blue
        default: return Color.secondary.opacity(0.45)
        }
    }
}

extension PanelRoot {
    private var delayCard: some View {
        card {
            VStack(spacing: 0) {
                metric("waveform.path.ecg", model.t("ICMP 延迟", "ICMP"), model.icmp)
                hairline()
                metric("link", model.t("TCPing（TLS 握手）", "TCPing (TLS)"), model.tcp)
                hairline()
                metric("globe", model.t("真实延迟", "Real delay"), model.real)
                hairline()
                metric("arrow.down", model.t("下载速度", "Download"), model.speed)
            }
        }
    }

    private var leakCard: some View {
        card {
            VStack(spacing: 0) {
                ForEach(0..<4, id: \.self) { index in
                    if index > 0 { hairline() }
                    let line = model.leaks[safe: index]
                    let text = line?.text ?? model.t("未测", "not tested")
                    let tone = line?.tone ?? "gray"
                    let untouched = text == model.t("未测", "not tested")
                    LeakRow(
                        name: [model.t("出口", "Exit"), "DNS", "WebRTC", "IPv6"][index],
                        text: text,
                        color: valueColor(tone == "gray" && untouched ? "muted" : tone)
                    )
                }
            }
        }
        .zIndex(1)
    }

    private var delayButtons: some View {
        HStack(spacing: 8) {
            CommandButton(
                title: model.delayOn ? model.t("测延迟", "Test latency") : model.t("测试中", "Testing"),
                symbol: "waveform.path.ecg",
                enabled: model.delayOn,
                action: { model.send("delay") }
            )
            CommandButton(
                title: model.speedOn ? model.t("测速", "Test speed") : model.t("测速中", "Measuring"),
                symbol: "gauge.with.needle",
                enabled: model.speedOn,
                action: { model.send("speed") }
            )
        }
    }

    private var leakButtons: some View {
        HStack(spacing: 8) {
            CommandButton(
                title: model.leakOn ? model.t("检测泄露", "Check for leaks") : model.t("检测中", "Checking"),
                symbol: "eye",
                enabled: model.leakOn,
                action: { model.send("leak") }
            )
            CommandButton(
                title: model.t("在浏览器中检测", "Check in browser"),
                symbol: "safari",
                action: { model.send("browser") }
            )
        }
    }

    private var controls: some View {
        card {
            VStack(spacing: 0) {
                HStack(spacing: 8) {
                    iconBadge("checkmark.shield.fill", Color.blue.opacity(0.16), .blue)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(model.t("防火墙保护", "Firewall"))
                            .font(.system(size: 13, weight: .semibold))
                        Text(model.t("隧道断开时阻止网络访问", "Block the internet if the tunnel drops"))
                            .font(.system(size: 11))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    Spacer(minLength: 6)
                    PillSwitch(isOn: Binding(
                        get: { model.firewall },
                        set: { new in
                            guard new != model.firewall else { return }
                            model.firewall = new
                            model.send(new ? "firewall 1" : "firewall 0")
                        }
                    ), enabled: model.firewallEnabled)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
                hairline()
                actionRow("arrow.triangle.2.circlepath", model.t("重连隧道", "Reconnect"), "⌘R", tint: .primary, enabled: model.restartOn) {
                    model.send("restart")
                }
                hairline()
                actionRow(
                    model.tunnelOn ? "link.slash" : "link",
                    model.tunnelOn ? model.t("断开隧道", "Disconnect") : model.t("连接隧道", "Connect"),
                    "⌘O",
                    tint: model.tunnelOn ? .red : .primary,
                    enabled: !model.busy
                ) {
                    if model.tunnelOn && model.firewall {
                        model.confirmOff = true
                    } else {
                        model.send("tunnel")
                    }
                }
                hairline()
                actionRow("server.rack", model.t("节点管理", "Node"), "⌘N", tint: .primary, enabled: true) { model.send("node") }
                hairline()
                actionRow("list.bullet", model.t("直连名单", "Direct list"), "⌘E", tint: .primary, enabled: true) { model.send("direct") }
                if model.installShow {
                    hairline()
                    installRow
                }
                hairline()
                actionRow("power", model.t("退出托盘", "Quit tray"), nil, tint: .red, enabled: true) { model.send("quit") }
            }
        }
    }

    private var installRow: some View {
        HStack(spacing: 8) {
            Image(systemName: "arrow.down.circle")
                .font(.system(size: 12, weight: .medium))
                .frame(width: 16)
            Text(model.install.isEmpty ? model.t("安装", "Install") : model.install)
                .font(.system(size: 13))
                .lineLimit(2)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .frame(maxWidth: .infinity, minHeight: 30)
        .contentShape(Rectangle())
        .onTapGesture { if model.installOn { model.send("install") } }
        .opacity(model.installOn ? 1 : 0.45)
    }
}

private struct PillSwitch: View {
    @Binding var isOn: Bool
    var enabled: Bool

    var body: some View {
        Button(action: { isOn.toggle() }) {
            ZStack(alignment: isOn ? .trailing : .leading) {
                Capsule()
                    .fill(isOn ? Color.accentColor : Color.primary.opacity(0.18))
                Circle()
                    .fill(Color.white)
                    .shadow(color: .black.opacity(0.2), radius: 0.6, y: 0.5)
                    .padding(2)
            }
            .frame(width: 32, height: 18)
            .opacity(enabled ? 1 : 0.4)
            .animation(.easeOut(duration: 0.15), value: isOn)
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
    }
}

private struct LeakRow: View {
    var name: String
    var text: String
    var color: Color
    @State private var hovering = false
    @State private var truncated = false

    var body: some View {
        HStack(spacing: 8) {
            Text(name)
                .foregroundStyle(.secondary)
                .fixedSize()
            Text(text)
                .foregroundStyle(color)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .trailing)
                .background {
                    GeometryReader { slot in
                        Color.clear
                            .onAppear { truncated = Self.overflows(text, slot.size.width) }
                            .onChange(of: text) { new in truncated = Self.overflows(new, slot.size.width) }
                            .onChange(of: slot.size.width) { width in truncated = Self.overflows(text, width) }
                    }
                }
        }
        .font(.system(size: 13))
        .padding(.horizontal, 10)
        .frame(height: 30)
        .frame(maxWidth: .infinity)
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .overlay(alignment: .topTrailing) {
            if hovering && truncated {
                Text(text)
                    .font(.system(size: 12))
                    .foregroundStyle(.primary)
                    .multilineTextAlignment(.leading)
                    .lineLimit(6)
                    .frame(maxWidth: 248, alignment: .leading)
                    .fixedSize(horizontal: true, vertical: true)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 5)
                    .background {
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .fill(.regularMaterial)
                            .shadow(color: .black.opacity(0.18), radius: 8, y: 3)
                    }
                    .padding(.trailing, 8)
                    .offset(y: 24)
                    .allowsHitTesting(false)
            }
        }
        .zIndex(hovering && truncated ? 2 : 0)
    }

    private static func overflows(_ text: String, _ slot: CGFloat) -> Bool {
        guard slot > 1 else { return false }
        let width = (text as NSString).size(withAttributes: [.font: NSFont.systemFont(ofSize: 13)]).width
        return width > slot - 4
    }
}

private struct CommandButton: View {
    var title: String
    var symbol: String
    var enabled: Bool = true
    var action: () -> Void
    @State private var hovering = false

    var body: some View {
        let hot = hovering && enabled
        Label(title, systemImage: symbol)
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(hot ? Color.white : Color.primary)
            .lineLimit(1)
            .frame(maxWidth: .infinity)
            .frame(height: 32)
            .background {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(hot ? Color.accentColor : Color.white.opacity(0.34))
                    .overlay {
                        if !hot {
                            RoundedRectangle(cornerRadius: 10, style: .continuous)
                                .strokeBorder(.white.opacity(0.5), lineWidth: 0.7)
                        }
                    }
            }
            .shadow(color: Color.accentColor.opacity(hot ? 0.42 : 0), radius: hot ? 10 : 0, y: hot ? 5 : 0)
            .offset(y: hot ? -1.5 : 0)
            .opacity(enabled ? 1 : 0.45)
            .contentShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .onHover { if enabled { hovering = $0 } }
            .onTapGesture { if enabled { action() } }
            .animation(.easeOut(duration: 0.16), value: hot)
    }
}

private func circleButton(_ symbol: String, action: @escaping () -> Void) -> some View {
    Button(action: action) {
        Image(systemName: symbol)
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(.secondary)
            .frame(width: 26, height: 26)
            .background {
                Circle()
                    .fill(.white.opacity(0.28))
                    .overlay { Circle().strokeBorder(.white.opacity(0.4), lineWidth: 0.6) }
            }
    }
    .buttonStyle(.plain)
}

private func iconBadge(_ symbol: String, _ fill: Color, _ tint: Color) -> some View {
    ZStack {
        Circle().fill(fill)
        Image(systemName: symbol)
            .font(.system(size: 12, weight: .semibold))
            .foregroundStyle(tint)
    }
    .frame(width: 26, height: 26)
}

private func metric(_ symbol: String, _ name: String, _ value: String) -> some View {
    HStack(spacing: 8) {
        Image(systemName: symbol)
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(.secondary)
            .frame(width: 16)
        Text(name)
            .font(.system(size: 13))
            .foregroundStyle(.secondary)
            .lineLimit(1)
        Spacer(minLength: 6)
        if value.contains("…") || value.hasPrefix("测速中") || value.hasPrefix("measuring") {
            ProgressView().controlSize(.mini)
        }
        Text(value)
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(valueColor(value == "未测" ? "muted" : (value == "不通" ? "red" : "text")))
            .lineLimit(1)
    }
    .padding(.horizontal, 10)
    .frame(height: 30)
}

private func actionRow(_ symbol: String, _ title: String, _ shortcut: String?, tint: Color, enabled: Bool, action: @escaping () -> Void) -> some View {
    HStack(spacing: 8) {
        Image(systemName: symbol)
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(tint)
            .frame(width: 16)
        Text(title)
            .font(.system(size: 13))
            .foregroundStyle(tint)
        Spacer(minLength: 6)
        if let shortcut {
            Text(shortcut)
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
        }
    }
    .padding(.horizontal, 10)
    .frame(maxWidth: .infinity)
    .frame(height: 30)
    .contentShape(Rectangle())
    .onTapGesture { if enabled { action() } }
    .opacity(enabled ? 1 : 0.45)
}

private func card<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
    content()
        .background {
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(Color(nsColor: .windowBackgroundColor).opacity(0.55))
                .overlay {
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .strokeBorder(.white.opacity(0.35), lineWidth: 0.5)
                }
        }
}

private func hairline() -> some View {
    Rectangle()
        .fill(Color.primary.opacity(0.06))
        .frame(height: 0.5)
        .padding(.leading, 10)
}

private func valueColor(_ name: String) -> Color {
    switch name {
    case "red": return .red
    case "green": return .green
    case "yellow": return .orange
    case "muted": return .secondary
    default: return .primary
    }
}

private extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}

final class PanelBridge {
    static var shared: PanelController?
}

@main
struct PanelMain {
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let controller = PanelController()
        PanelBridge.shared = controller
        Thread {
            while let line = readLine() {
                let data = Data(line.utf8)
                DispatchQueue.main.async {
                    controller.handle(data)
                }
            }
            DispatchQueue.main.async {
                app.terminate(nil)
            }
        }.start()
        app.run()
    }
}

import AppKit
import Foundation

// App icon: the same tunnel arch as the menu-bar mark, on a green tile.
// macOS masks the square to a squircle, so the artwork fills the frame.
let side = CGFloat(1024)
let image = NSImage(size: NSSize(width: side, height: side))
image.lockFocus()
NSColor(calibratedRed: 0.145, green: 0.58, blue: 0.40, alpha: 1).setFill()
NSRect(x: 0, y: 0, width: side, height: side).fill()

func arch(outer: CGFloat, inner: CGFloat) -> NSBezierPath {
    let path = NSBezierPath()
    let spring = side * 0.56
    let floor = side * 0.18
    let mid = side * 0.5
    path.move(to: NSPoint(x: mid - outer, y: floor))
    path.line(to: NSPoint(x: mid - outer, y: spring))
    path.appendArc(withCenter: NSPoint(x: mid, y: spring), radius: outer, startAngle: 180, endAngle: 0, clockwise: true)
    path.line(to: NSPoint(x: mid + outer, y: floor))
    path.line(to: NSPoint(x: mid + inner, y: floor))
    path.line(to: NSPoint(x: mid + inner, y: spring))
    path.appendArc(withCenter: NSPoint(x: mid, y: spring), radius: inner, startAngle: 0, endAngle: 180, clockwise: false)
    path.line(to: NSPoint(x: mid - inner, y: floor))
    path.close()
    return path
}

NSColor.white.setFill()
arch(outer: side * 0.34, inner: side * 0.17).fill()
NSColor(calibratedRed: 1, green: 0.86, blue: 0.45, alpha: 1).setFill()
let light = side * 0.045
NSBezierPath(
    ovalIn: NSRect(x: side * 0.5 - light, y: side * 0.40 - light, width: light * 2, height: light * 2)
).fill()
image.unlockFocus()

guard let tiff = image.tiffRepresentation,
      let rep = NSBitmapImageRep(data: tiff),
      let png = rep.representation(using: .png, properties: [:]) else {
    fputs("could not encode the icon\n", stderr)
    exit(1)
}
let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "AppIcon.png"
try png.write(to: URL(fileURLWithPath: out))

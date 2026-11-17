// Small helper for mac_shot.sh: find a process's main window, click in it.
//   winshot window <pid>          -> "<windowID> <x> <y> <w> <h>" (largest on-screen window)
//   winshot front <pid>           -> the same for its frontmost window (Settings, a sheet's window)
//   winshot click <pid> <x> <y> [n] -> CGEvent click (needs Accessibility for the caller;
//                                    mac_shot.sh uses System Events instead)
import AppKit
import CoreGraphics

/// The largest of `pid`'s on-screen windows, or with `front` its frontmost:
/// the list comes front to back.
func window(_ pid: pid_t, front: Bool = false) -> (CGWindowID, CGRect)? {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    var best: (CGWindowID, CGRect)?
    for w in list where (w[kCGWindowOwnerPID as String] as? Int32) == pid && (w[kCGWindowLayer as String] as? Int) == 0 {
        guard let id = w[kCGWindowNumber as String] as? CGWindowID,
              let b = w[kCGWindowBounds as String] as? [String: CGFloat] else { continue }
        let r = CGRect(x: b["X"]!, y: b["Y"]!, width: b["Width"]!, height: b["Height"]!)
        if front { return (id, r) }
        if best == nil || r.width * r.height > best!.1.width * best!.1.height { best = (id, r) }
    }
    return best
}

let a = CommandLine.arguments
guard a.count >= 3, let pid = pid_t(a[2]), let (id, r) = window(pid, front: a[1] == "front") else { print("no window"); exit(1) }
switch a[1] {
case "window", "front": print(id, Int(r.minX), Int(r.minY), Int(r.width), Int(r.height))
case "click":
    let p = CGPoint(x: r.minX + CGFloat(Double(a[3])!), y: r.minY + CGFloat(Double(a[4])!))
    let count = a.count > 5 ? Int(a[5])! : 1
    // Real HID clicks: SwiftUI ignores events posted to the process.
    CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left)!.post(tap: .cghidEventTap)
    usleep(80_000)
    for n in 1...count {
        for type in [CGEventType.leftMouseDown, .leftMouseUp] {
            let e = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: p, mouseButton: .left)!
            e.setIntegerValueField(.mouseEventClickState, value: Int64(n))
            e.post(tap: .cghidEventTap)
            usleep(30_000)
        }
    }
default: exit(2)
}

import CoreGraphics
import Foundation
// Prints the on-screen window number of the given PID's largest window,
// or with "front" its frontmost ordinary one.
let pid = Int32(CommandLine.arguments[1])!
let front = CommandLine.arguments.count > 2 && CommandLine.arguments[2] == "front"
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
var best: (Int, Double) = (0, 0)
for w in list where (w[kCGWindowOwnerPID as String] as? Int32) == pid {
    if front {
        if (w[kCGWindowLayer as String] as? Int) == 0 { print(w[kCGWindowNumber as String] as! Int); exit(0) }
        continue
    }
    let b = w[kCGWindowBounds as String] as! [String: Double]
    let area = b["Width"]! * b["Height"]!
    if area > best.1 { best = (w[kCGWindowNumber as String] as! Int, area) }
}
print(best.0)

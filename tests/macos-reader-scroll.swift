// Synthetic native-input regression helper; run only against an isolated test app.
// swift tests/macos-reader-scroll.swift <app-pid>
import AppKit
import CoreGraphics
let pid = pid_t(CommandLine.arguments[1])!
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String:Any]]
let entry = windows.first { ($0[kCGWindowOwnerPID as String] as? Int) == Int(pid) && ($0[kCGWindowLayer as String] as? Int) == 0 && (($0[kCGWindowBounds as String] as? [String:Double])?["Width"] ?? 0) > 600 && (($0[kCGWindowBounds as String] as? [String:Double])?["Height"] ?? 0) > 300 }!
let b = entry[kCGWindowBounds as String] as! [String:Double]
let point = CGPoint(x:b["X"]! + b["Width"]! * 0.85,y:b["Y"]! + b["Height"]! * 0.65)
NSRunningApplication(processIdentifier:pid)?.activate(options: [])
let mouse = CGEvent(mouseEventSource:nil,mouseType:.mouseMoved,mouseCursorPosition:point,mouseButton:.left)!
mouse.post(tap: .cghidEventTap)
Thread.sleep(forTimeInterval:0.3)
let event = CGEvent(scrollWheelEvent2Source:nil,units:.pixel,wheelCount:2,wheel1:-320,wheel2:0,wheel3:0)!
event.location=point;event.post(tap: .cghidEventTap)
print("posted native wheel at \(point); accessibility=\(AXIsProcessTrusted())")

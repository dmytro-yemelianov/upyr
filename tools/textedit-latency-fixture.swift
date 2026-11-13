// Test-only observer of a new TextEdit instance and its one owned document.
import Cocoa
import ApplicationServices

guard CommandLine.arguments.count == 2, AXIsProcessTrusted() else { exit(2) }
let root = URL(fileURLWithPath: CommandLine.arguments[1])
let document = root.appendingPathComponent("Upyr-owned-probe.txt")
try Data().write(to: document)
let previous = Set(NSWorkspace.shared.runningApplications.map { $0.processIdentifier })
let configuration = NSWorkspace.OpenConfiguration()
configuration.createsNewApplicationInstance = true
configuration.activates = true
var target: NSRunningApplication?
NSWorkspace.shared.open([document], withApplicationAt: URL(fileURLWithPath: "/System/Applications/TextEdit.app"), configuration: configuration) { application, error in
    guard error == nil, let application, !previous.contains(application.processIdentifier) else { exit(3) }
    target = application
    let owner = try? JSONSerialization.data(withJSONObject: ["fixture_pid": getpid(), "app_pid": application.processIdentifier])
    try? owner?.write(to: root.appendingPathComponent("owner.json"), options: .atomic)
}
func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var result: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &result) == .success else { return nil }
    return result
}
func textArea(_ element: AXUIElement, depth: Int = 0) -> AXUIElement? {
    if attribute(element, kAXRoleAttribute) as? String == kAXTextAreaRole { return element }
    guard depth < 12 else { return nil }
    for child in attribute(element, kAXChildrenAttribute) as? [AXUIElement] ?? [] {
        if let found = textArea(child, depth: depth + 1) { return found }
    }
    return nil
}
var nonce = "", field: AXUIElement?, ownedWindow: AXUIElement?, windowID = 0
let deadline = Date().addingTimeInterval(20)
while true {
    RunLoop.current.run(until: Date().addingTimeInterval(0.005))
    guard let target else { if Date() > deadline { exit(4) }; continue }
    if target.isTerminated { exit(5) }
    let application = AXUIElementCreateApplication(target.processIdentifier)
    AXUIElementSetMessagingTimeout(application, 0.05)
    if field == nil {
        for window in attribute(application, kAXWindowsAttribute) as? [AXUIElement] ?? [] {
            guard let value = attribute(window, kAXDocumentAttribute) as? String,
                  URL(string: value)?.standardizedFileURL == document.standardizedFileURL else { continue }
            ownedWindow = window
            field = textArea(window)
            let windows = CGWindowListCopyWindowInfo(.optionOnScreenOnly, 0) as? [[String: Any]] ?? []
            windowID = windows.first { ($0[kCGWindowOwnerPID as String] as? Int32) == target.processIdentifier
                && ($0[kCGWindowLayer as String] as? Int) == 0 }?[kCGWindowNumber as String] as? Int ?? 0
        }
    }
    guard let field, let ownedWindow, windowID != 0 else { if Date() > deadline { target.terminate(); exit(6) }; continue }
    let control = (try? JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("control.json")))) as? [String: Any] ?? [:]
    if control["stop"] as? Bool == true { target.terminate(); exit(0) }
    let focused = attribute(application, kAXFocusedUIElementAttribute)
    let focusedWindow = attribute(application, kAXFocusedWindowAttribute)
    let ready = NSWorkspace.shared.frontmostApplication?.processIdentifier == target.processIdentifier
        && focused.map { CFEqual($0, field) } == true
        && focusedWindow.map { CFEqual($0, ownedWindow) } == true
    if let new = control["nonce"] as? String, new != nonce, ready {
        let seed = control["seed"] as? String ?? ""
        guard AXUIElementSetAttributeValue(field, kAXValueAttribute as CFString, seed as CFString) == .success else { target.terminate(); exit(7) }
        var range = CFRange(location: (seed as NSString).length, length: 0)
        let value = AXValueCreate(.cfRange, &range)!
        guard AXUIElementSetAttributeValue(field, kAXSelectedTextRangeAttribute as CFString, value) == .success else { target.terminate(); exit(8) }
        nonce = new
    }
    let text = attribute(field, kAXValueAttribute) as? String ?? ""
    let state: [String: Any] = ["pid": getpid(), "app_pid": target.processIdentifier, "window": windowID,
        "ready": ready, "nonce": nonce, "text": text, "enters": text.filter { $0 == "\n" }.count,
        "observed_at_ms": Date().timeIntervalSince1970 * 1000]
    let data = try JSONSerialization.data(withJSONObject: state, options: [.sortedKeys])
    try data.write(to: root.appendingPathComponent("state.json"), options: .atomic)
}

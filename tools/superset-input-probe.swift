// Test-only physical input for tools/superset_input_probe.py. Never shipped.
import Carbon
import Cocoa

func sourceID() -> String {
    let source = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
    guard let value = TISGetInputSourceProperty(source, kTISPropertyInputSourceID) else { return "?" }
    return Unmanaged<CFString>.fromOpaque(value).takeUnretainedValue() as String
}
func emit(_ object: [String: Any]) {
    print(String(data: try! JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]), encoding: .utf8)!)
}
func firstWindow(_ pid: pid_t) -> Int? {
    let windows = CGWindowListCopyWindowInfo(.optionOnScreenOnly, 0) as? [[String: Any]] ?? []
    return windows.first { ($0[kCGWindowOwnerPID as String] as? Int) == Int(pid)
        && ($0[kCGWindowLayer as String] as? Int) == 0
        && (($0[kCGWindowBounds as String] as? [String: Double])?["Height"] ?? 0) > 40
    }?[kCGWindowNumber as String] as? Int
}
let args = CommandLine.arguments
if args.count == 1 {
    let app = NSWorkspace.shared.frontmostApplication
    emit(["source": sourceID(), "frontPID": app?.processIdentifier ?? 0,
          "bundle": app?.bundleIdentifier ?? "", "window": firstWindow(app?.processIdentifier ?? 0) ?? 0,
          "screenLocked": (CGSessionCopyCurrentDictionary() as? [String: Any])?["CGSSessionScreenIsLocked"] as? Bool ?? false,
          "postAccess": CGPreflightPostEventAccess()])
    exit(0)
}
if args.count == 4 && args[1] == "marker" {
    guard let bitmap = NSBitmapImageRep(data: try Data(contentsOf: URL(fileURLWithPath: args[2]))),
          let code = UInt32(args[3], radix: 16) else { exit(2) }
    let bits = (0..<32).reversed().map { code & (1 << $0) == 0 ? "b" : "r" }.joined()
    let expected = "g" + bits + "g"
    var matched = false
    // Read only the exact window image returned by Cua Driver. The opaque
    // background blocks survive display scaling; text recognition is unused.
    for y in stride(from: 0, to: bitmap.pixelsHigh, by: 3) {
        var line = "", previous = ".", run = 0
        func finishRun() { if previous != "." && run >= 3 { line += previous } }
        for x in 0..<bitmap.pixelsWide {
            var symbol = "."
            if let c = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) {
                let red = c.redComponent * 255, green = c.greenComponent * 255, blue = c.blueComponent * 255
                // Display profiles and terminal renderers can shift channels.
                // Match distinct color families; the complete 32-bit code must
                // still match exactly, between two green sentinels.
                if red > 220 && green < 140 && blue > 65 && blue < 200 { symbol = "r" }
                else if blue > 220 && red < 140 && green > 100 && green < 230 { symbol = "b" }
                else if green > 220 && green > red + 80 && green > blue + 80 { symbol = "g" }
            }
            if symbol == previous { run += 1 } else { finishRun(); previous = symbol; run = 1 }
        }
        finishRun()
        if line.contains(expected) { matched = true; break }
    }
    emit(["ownedFixtureVisible": matched])
    exit(0)
}
if args.count == 3 && args[1] == "focus-undo" {
    let app = NSApplication.shared
    app.setActivationPolicy(.regular)
    let window = NSWindow(contentRect: NSRect(x: 200, y: 200, width: 420, height: 70), styleMask: [.titled], backing: .buffered, defer: false)
    window.title = "Upyr isolated focus probe"
    let field = NSTextField(frame: NSRect(x: 10, y: 15, width: 400, height: 28))
    field.stringValue = "untouched"
    window.contentView?.addSubview(field)
    window.makeKeyAndOrderFront(nil)
    app.activate(ignoringOtherApps: true)
    window.makeFirstResponder(field)
    var external = false
    let testedPID = Int64(args[2]) ?? -1
    let observer = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
        let origin = event.cgEvent?.getIntegerValueField(.eventSourceUnixProcessID) ?? -1
        if origin != Int64(getpid()) && origin != testedPID { external = true }
        return event
    }
    _ = observer
    func pumpField(_ ms: Int) {
        let until = Date().addingTimeInterval(Double(ms) / 1000)
        while let event = app.nextEvent(matching: .any, until: until, inMode: .default, dequeue: true) { app.sendEvent(event) }
    }
    pumpField(600)
    guard NSWorkspace.shared.frontmostApplication?.processIdentifier == getpid(), window.isKeyWindow, !external else { exit(3) }
    let revision = NSPasteboard.general.changeCount
    let beforeSource = sourceID()
    let source = CGEventSource(stateID: .hidSystemState)
    func postField(_ code: CGKeyCode, _ down: Bool, _ flags: CGEventFlags, _ modifier: Bool = false) {
        let event = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down)!
        if modifier { event.type = .flagsChanged }
        event.flags = flags; event.post(tap: .cghidEventTap)
    }
    postField(55, true, .maskCommand, true); pumpField(20)
    postField(58, true, [.maskCommand, .maskAlternate], true); pumpField(20)
    if NSWorkspace.shared.frontmostApplication?.processIdentifier == getpid(), window.isKeyWindow, !external {
        postField(6, true, [.maskCommand, .maskAlternate]); postField(6, false, [.maskCommand, .maskAlternate])
        pumpField(80)
    } else { external = true }
    postField(58, false, .maskCommand, true); postField(55, false, [], true)
    pumpField(700)
    let value = field.currentEditor()?.string ?? field.stringValue
    let unchanged = value == "untouched" && sourceID() == beforeSource && NSPasteboard.general.changeCount == revision && !external
    emit(["focusUndoIgnored": unchanged])
    exit(unchanged ? 0 : 1)
}
if args.count == 3 && args[1] == "select" {
    let list = TISCreateInputSourceList([kTISPropertyInputSourceID: args[2]] as CFDictionary, false).takeRetainedValue() as! [TISInputSource]
    guard let source = list.first, TISSelectInputSource(source) == noErr else { exit(1) }
    RunLoop.current.run(until: Date().addingTimeInterval(0.15))
    emit(["source": sourceID()]); exit(0)
}
guard args.count == 6, let pid = Int32(args[1]), let window = Int(args[2]), CGPreflightPostEventAccess() else { exit(2) }
let root = URL(fileURLWithPath: args[3]), nonce = args[4]
let plan = try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: args[5]))) as! [String: Any]
let upyrPID = (plan["upyrPID"] as? Int64) ?? -1
func ownsFixture() -> Bool {
    guard NSWorkspace.shared.frontmostApplication?.processIdentifier == pid,
          firstWindow(pid) == window,
          let data = try? Data(contentsOf: root.appendingPathComponent("state.json")),
          let state = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
          state["nonce"] as? String == nonce, state["ready"] as? Bool == true,
          let fixturePID = state["pid"] as? Int32, kill(fixturePID, 0) == 0 else { return false }
    if let observed = state["observed_at_ms"] as? Double,
       abs(Date().timeIntervalSince1970 * 1000 - observed) > 500 { return false }
    return true
}
var interrupted = false
let mask = [CGEventType.keyDown, .flagsChanged, .leftMouseDown, .rightMouseDown, .otherMouseDown, .scrollWheel]
    .reduce(CGEventMask(0)) { $0 | (1 << $1.rawValue) }
let tap = CGEvent.tapCreate(tap: .cgSessionEventTap, place: .headInsertEventTap, options: .listenOnly,
    eventsOfInterest: mask, callback: { _, type, event, _ in
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput { interrupted = true }
        let origin = event.getIntegerValueField(.eventSourceUnixProcessID)
        if origin != Int64(getpid()) && origin != upyrPID { interrupted = true }
        return Unmanaged.passUnretained(event)
    }, userInfo: nil)
guard let tap else { emit(["error": "input observation permission missing"]); exit(2) }
let runSource = CFMachPortCreateRunLoopSource(nil, tap, 0)!
CFRunLoopAddSource(CFRunLoopGetCurrent(), runSource, .commonModes)
CGEvent.tapEnable(tap: tap, enable: true)
func pump(_ ms: Int) { RunLoop.current.run(until: Date().addingTimeInterval(Double(ms) / 1000)) }
func clipboard() -> [[String: Data]] {
    (NSPasteboard.general.pasteboardItems ?? []).map { item in
        Dictionary(uniqueKeysWithValues: item.types.compactMap { type in item.data(forType: type).map { (type.rawValue, $0) } })
    }
}
let before = clipboard(), revision = NSPasteboard.general.changeCount
let eventSource = CGEventSource(stateID: .hidSystemState)
var posted: [[String: Any]] = []
var checkpoints: [[String: Any]] = []
var marks: [String: UInt64] = [:]
var activeMark: String?
var latestKey: UInt64 = 0
func post(_ code: CGKeyCode, _ down: Bool, _ flags: CGEventFlags = [], modifier: Bool = false) {
    let event = CGEvent(keyboardEventSource: eventSource, virtualKey: code, keyDown: down)!
    if modifier { event.type = .flagsChanged }
    event.flags = flags
    let now = DispatchTime.now().uptimeNanoseconds
    if down && !modifier {
        latestKey = now
        if let label = activeMark { marks[label] = now }
        posted.append(["key": code, "flags": flags.rawValue, "post_ns": now])
    }
    event.post(tap: .cghidEventTap)
}
func requireFocus() {
    guard !interrupted, ownsFixture() else {
        let state = (try? JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("state.json")))) as? [String: Any] ?? [:]
        let partial: [String: Any] = ["checkpoints": checkpoints, "posted": posted, "outsideInput": interrupted]
        if let data = try? JSONSerialization.data(withJSONObject: partial, options: [.sortedKeys]) {
            try? data.write(to: root.appendingPathComponent("interrupted.json"), options: .atomic)
        }
        emit(["error": "test interrupted or owned fixture lost focus", "outsideInput": interrupted,
              "frontPID": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0,
              "expectedPID": pid, "actualWindow": firstWindow(pid) ?? 0, "expectedWindow": window,
              "ready": state["ready"] as? Bool ?? false, "nonceMatches": state["nonce"] as? String == nonce,
              "stateAge_ms": Date().timeIntervalSince1970 * 1000 - (state["observed_at_ms"] as? Double ?? Date().timeIntervalSince1970 * 1000),
              "postedKeys": posted.count]); exit(3)
    }
}
pump(80); requireFocus()
if let sourceID = plan["source"] as? String {
    let list = TISCreateInputSourceList([kTISPropertyInputSourceID: sourceID] as CFDictionary, false).takeRetainedValue() as! [TISInputSource]
    guard let source = list.first, TISSelectInputSource(source) == noErr else { exit(2) }
    pump(160)
}
let codes: [Character: CGKeyCode] = ["a":0,"b":11,"c":8,"d":2,"e":14,"f":3,"g":5,"h":4,"i":34,"j":38,"k":40,"l":37,"m":46,"n":45,"o":31,"p":35,"q":12,"r":15,"s":1,"t":17,"u":32,"v":9,"w":13,"x":7,"y":16,"z":6,"'":39,"[":33,"]":30,";":41,",":43,".":47,"/":44," ":49,"\\":42,"`":50]
for action in plan["actions"] as? [[String: Any]] ?? [] {
    requireFocus()
    activeMark = action["mark"] as? String
    let wanted = (action["expect"] as? String).map { [$0] } ?? action["expect"] as? [String]
    if let wanted {
        let deadline = DispatchTime.now().uptimeNanoseconds + UInt64(action["timeout"] as? Int ?? 2000) * 1_000_000
        let start = (action["since"] as? String).flatMap { marks[$0] } ?? latestKey
        var textAt: UInt64?, layoutAt: UInt64?
        var current = ""
        while DispatchTime.now().uptimeNanoseconds < deadline {
            requireFocus()
            let state = (try? JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("state.json")))) as? [String: Any]
            current = state?["text"] as? String ?? ""
            let now = DispatchTime.now().uptimeNanoseconds
            if wanted.contains(current) && textAt == nil { textAt = now }
            if (action["layout"] as? String).map({ sourceID() == $0 }) ?? true {
                if layoutAt == nil { layoutAt = now }
            }
            if wanted.contains(current) && ((action["layout"] as? String).map({ sourceID() == $0 }) ?? true) { break }
            pump(2)
        }
        let passed = wanted.contains(current) && ((action["layout"] as? String).map({ sourceID() == $0 }) ?? true)
        var record: [String: Any] = ["name": action["name"] as? String ?? "checkpoint", "passed": passed, "observed": current, "source": sourceID(), "timed": action["since"] is String]
        if start != 0 && passed, let textAt, let layoutAt {
            record["text_ms"] = Double(textAt - start) / 1_000_000
            record["layout_ms"] = Double(layoutAt - start) / 1_000_000
            record["settled_ms"] = Double(max(textAt, layoutAt) - start) / 1_000_000
        }
        checkpoints.append(record)
        if !passed { break }
        continue
    }
    if let selected = action["source"] as? String {
        let list = TISCreateInputSourceList([kTISPropertyInputSourceID: selected] as CFDictionary, false).takeRetainedValue() as! [TISInputSource]
        guard let source = list.first, TISSelectInputSource(source) == noErr else { exit(2) }
        pump(160); continue
    }
    if action["globe"] as? Bool == true {
        post(63, true, .maskSecondaryFn, modifier: true); pump(60)
        let now = DispatchTime.now().uptimeNanoseconds
        latestKey = now
        if let label = activeMark { marks[label] = now }
        post(63, false, [], modifier: true)
        continue
    }
    if let wait = action["wait"] as? Int { pump(wait); continue }
    if action["undo"] as? Bool == true {
        post(55, true, .maskCommand, modifier: true); pump(20)
        post(58, true, [.maskCommand, .maskAlternate], modifier: true); pump(20)
        if !interrupted && ownsFixture() {
            post(6, true, [.maskCommand, .maskAlternate]); post(6, false, [.maskCommand, .maskAlternate]); pump(action["keyWait"] as? Int ?? 80)
        } else { interrupted = true }
        // Always release our modifiers, including after an interruption.
        post(58, false, .maskCommand, modifier: true); post(55, false, [], modifier: true)
        pump(action["settle"] as? Int ?? 700); continue
    }
    let sequence: [(CGKeyCode, Bool)]
    if let text = action["text"] as? String {
        guard text.allSatisfy({ codes[Character(String($0).lowercased())] != nil }) else { exit(2) }
        sequence = text.map { (codes[Character(String($0).lowercased())]!, $0.isUppercase) }
    } else if let key = action["key"] as? UInt16 { sequence = [(key, action["shift"] as? Bool ?? false)] } else { exit(2) }
    for (code, shifted) in sequence {
        requireFocus()
        if shifted { post(56, true, .maskShift, modifier: true) }
        post(code, true, shifted ? .maskShift : []); post(code, false, shifted ? .maskShift : [])
        if shifted { post(56, false, [], modifier: true) }
        pump(action["gap"] as? Int ?? 70)
    }
}
let cleanupStart = DispatchTime.now().uptimeNanoseconds
pump(plan["finalWait"] as? Int ?? 150); requireFocus()
// Clipboard restoration is asynchronous and can lag editor confirmation when
// the app's event loop is busy (for example during cold audio initialization).
// This bounded wait is outside every editing-latency checkpoint.
let cleanupDeadline = DispatchTime.now().uptimeNanoseconds + UInt64(max(0, plan["clipboardRestoreTimeout"] as? Int ?? 0)) * 1_000_000
var contentRestored = clipboard() == before
while !contentRestored && DispatchTime.now().uptimeNanoseconds < cleanupDeadline {
    pump(10); requireFocus()
    contentRestored = clipboard() == before
}
let after = clipboard()
var clipboardDiagnostic: [String: Any] = [:]
if !contentRestored {
    // Format names/sizes only, never the user's clipboard payload.
    clipboardDiagnostic["before"] = before.map { $0.mapValues { $0.count } }
    clipboardDiagnostic["after"] = after.map { $0.mapValues { $0.count } }
    clipboardDiagnostic["changed_formats"] = before.enumerated().map { index, item in
        item.keys.filter { key in index >= after.count || after[index][key] != item[key] }.sorted()
    }
}
emit(["source": sourceID(), "clipboardUnchanged": after == before && NSPasteboard.general.changeCount == revision,
      "clipboardContentRestored": contentRestored,
      "clipboardDiagnostic": clipboardDiagnostic,
      "clipboard_restore_wait_ms": Double(DispatchTime.now().uptimeNanoseconds - cleanupStart) / 1_000_000,
      "posted": posted, "checkpoints": checkpoints,
      "poll_interval_ms": 2])

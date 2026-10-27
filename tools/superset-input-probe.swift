// Test-only physical input for tools/superset_input_probe.py. Never shipped.
import Carbon
import Cocoa
import Vision

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
          "postAccess": CGPreflightPostEventAccess()])
    exit(0)
}
if args.count == 4 && args[1] == "ocr" {
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .fast
    request.usesLanguageCorrection = false
    try VNImageRequestHandler(url: URL(fileURLWithPath: args[2])).perform([request])
    let lines = (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }
    func fold(_ text: String) -> String {
        var result = text.lowercased().filter { $0.isLetter || $0.isNumber }
        for (letter, digit) in [("o", "0"), ("l", "1"), ("i", "1")] {
            result = result.replacingOccurrences(of: letter, with: digit)
        }
        return result
    }
    let recognized = fold(lines.joined())
    let expected = fold(args[3])
    let marker = fold("upyrprobe")
    var matched = recognized.contains(marker + expected)
    if !matched, let range = recognized.range(of: marker) {
        let code = String(recognized[range.upperBound...].prefix(expected.count))
        // One OCR substitution is allowed in an otherwise unique run code.
        matched = code.count == expected.count && zip(code, expected).filter { $0 != $1 }.count <= 1
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
func post(_ code: CGKeyCode, _ down: Bool, _ flags: CGEventFlags = [], modifier: Bool = false) {
    let event = CGEvent(keyboardEventSource: eventSource, virtualKey: code, keyDown: down)!
    if modifier { event.type = .flagsChanged }
    event.flags = flags; event.post(tap: .cghidEventTap)
}
func requireFocus() {
    guard !interrupted, ownsFixture() else { emit(["error": "test interrupted or owned fixture lost focus"]); exit(3) }
}
pump(80); requireFocus()
if let sourceID = plan["source"] as? String {
    let list = TISCreateInputSourceList([kTISPropertyInputSourceID: sourceID] as CFDictionary, false).takeRetainedValue() as! [TISInputSource]
    guard let source = list.first, TISSelectInputSource(source) == noErr else { exit(2) }
    pump(160)
}
let codes: [Character: CGKeyCode] = ["a":0,"b":11,"c":8,"d":2,"e":14,"f":3,"g":5,"h":4,"i":34,"j":38,"k":40,"l":37,"m":46,"n":45,"o":31,"p":35,"q":12,"r":15,"s":1,"t":17,"u":32,"v":9,"w":13,"x":7,"y":16,"z":6,"'":39,"[":33,"]":30,";":41,",":43,".":47,"/":44," ":49]
for action in plan["actions"] as? [[String: Any]] ?? [] {
    requireFocus()
    if let wait = action["wait"] as? Int { pump(wait); continue }
    if action["undo"] as? Bool == true {
        post(55, true, .maskCommand, modifier: true); pump(20)
        post(58, true, [.maskCommand, .maskAlternate], modifier: true); pump(20)
        if !interrupted && ownsFixture() {
            post(6, true, [.maskCommand, .maskAlternate]); post(6, false, [.maskCommand, .maskAlternate]); pump(80)
        } else { interrupted = true }
        // Always release our modifiers, including after an interruption.
        post(58, false, .maskCommand, modifier: true); post(55, false, [], modifier: true)
        pump(700); continue
    }
    let sequence: [CGKeyCode]
    if let text = action["text"] as? String {
        guard text.allSatisfy({ codes[$0] != nil }) else { exit(2) }
        sequence = text.map { codes[$0]! }
    } else if let key = action["key"] as? UInt16 { sequence = [key] } else { exit(2) }
    for code in sequence {
        requireFocus(); post(code, true); post(code, false); pump(action["gap"] as? Int ?? 70)
    }
}
pump(150); requireFocus()
emit(["source": sourceID(), "clipboardUnchanged": clipboard() == before && NSPasteboard.general.changeCount == revision])

// Owned AppKit editor for run_typing_latency.py. No text leaves the test directory.
import Carbon
import Cocoa

guard CommandLine.arguments.count == 2 else { exit(2) }
let root = URL(fileURLWithPath: CommandLine.arguments[1])
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let menu = NSMenu()
let application = NSMenuItem(title: "Upyr Probe", action: nil, keyEquivalent: "")
application.submenu = NSMenu(title: "Upyr Probe")
menu.addItem(application)
let edit = NSMenu(title: "Edit")
for (name, action, key) in [("Copy", #selector(NSText.copy(_:)), "c"), ("Paste", #selector(NSText.paste(_:)), "v"), ("Select All", #selector(NSText.selectAll(_:)), "a")] {
    edit.addItem(withTitle: name, action: action, keyEquivalent: key)
}
let editItem = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
editItem.submenu = edit; menu.addItem(editItem); app.mainMenu = menu
let window = NSWindow(contentRect: NSRect(x: 160, y: 180, width: 780, height: 260), styleMask: [.titled, .closable], backing: .buffered, defer: false)
window.title = "Upyr isolated typing latency probe"
let scroll = NSScrollView(frame: NSRect(x: 16, y: 16, width: 748, height: 220))
let field = NSTextView(frame: scroll.bounds)
field.isRichText = false
field.font = NSFont.monospacedSystemFont(ofSize: 20, weight: .regular)
field.isAutomaticSpellingCorrectionEnabled = false
field.isAutomaticTextReplacementEnabled = false
field.isAutomaticQuoteSubstitutionEnabled = false
field.isAutomaticDashSubstitutionEnabled = false
field.setAccessibilityLabel("Upyr isolated test editor")
scroll.documentView = field; window.contentView?.addSubview(scroll)
var nonce = "", last = "", ready = true
var received: [[String: Any]] = []
// Opt-in diagnostics only inside this owned synthetic document, never global input.
let traceURL = root.appendingPathComponent("events.jsonl")
let tracingInput = ProcessInfo.processInfo.environment["UPYR_PROBE_TRACE"] == "1"
if tracingInput { _ = FileManager.default.createFile(atPath: traceURL.path, contents: nil) }
let trace = tracingInput ? try? FileHandle(forWritingTo: traceURL) : nil
@Sendable func record(_ object: [String: Any]) {
    guard let trace, let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]) else { return }
    try? trace.write(contentsOf: data + Data([10]))
}
@MainActor func publish() {
    let object: [String: Any] = ["pid": getpid(), "window": window.windowNumber, "ready": ready,
        "nonce": nonce, "text": field.string, "cursor": field.selectedRange().location,
        "enters": field.string.filter { $0 == "\n" }.count, "received": received]
    if let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]) {
        try? data.write(to: root.appendingPathComponent("state.json"), options: .atomic)
    }
    record(["kind": "state", "at_ns": DispatchTime.now().uptimeNanoseconds, "nonce": nonce,
            "text": field.string, "selection": [field.selectedRange().location, field.selectedRange().length]])
    last = field.string
}
let observer = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
    var units = [UniChar](repeating: 0, count: 8)
    var length = 0
    event.cgEvent?.keyboardGetUnicodeString(maxStringLength: units.count, actualStringLength: &length, unicodeString: &units)
    record(["kind": "key", "at_ns": DispatchTime.now().uptimeNanoseconds,
            "cg_text": String(utf16CodeUnits: units, count: length),
            "event_ns": event.cgEvent?.timestamp ?? 0, "code": event.keyCode,
            "characters": event.characters ?? "", "flags": event.modifierFlags.rawValue,
            "before": field.string, "selection": [field.selectedRange().location, field.selectedRange().length],
            "origin": event.cgEvent?.getIntegerValueField(.eventSourceUnixProcessID) ?? -1])
    received.append(["key": event.keyCode, "at_ns": DispatchTime.now().uptimeNanoseconds,
                     "origin": event.cgEvent?.getIntegerValueField(.eventSourceUnixProcessID) ?? -1])
    return event
}
_ = observer
window.makeKeyAndOrderFront(nil)
window.makeFirstResponder(field)
MainActor.assumeIsolated { publish() }
let timer = Timer.scheduledTimer(withTimeInterval: 0.002, repeats: true) { _ in
    MainActor.assumeIsolated {
    guard let data = try? Data(contentsOf: root.appendingPathComponent("control.json")),
          let control = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return }
    if control["stop"] as? Bool == true { ready = false; publish(); app.terminate(nil); return }
    if let new = control["nonce"] as? String, new != nonce {
        nonce = new
        field.string = control["seed"] as? String ?? ""
        field.setSelectedRange(NSRange(location: (field.string as NSString).length, length: 0))
        received = []; publish()
    } else if field.string != last { publish() }
    }
}
_ = timer
app.run()

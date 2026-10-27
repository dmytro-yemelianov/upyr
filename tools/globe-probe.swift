// globe-probe: exercise Upyr's integrated GlobeSwitch observer with synthetic events.
// Adapted from dmytro-yemelianov/globe-switch (MIT).
//
// Synthetic CGEvent taps reach Upyr's event tap but never macOS's own 🌐 handling,
// so this exercises the integrated observer alone. Needs Accessibility access for the terminal.
//
//   globe-probe status                              permission, current layout, TextInputSwitcher pid
//   globe-probe press <count> <gapMs> [typeAfterMs] tap 🌐 <count> times; optionally tap shift after
//   globe-probe chord                               fn held while shift is tapped: must not switch
//   globe-probe hold                                fn held 900 ms: must not switch
//   globe-probe scroll                              fn held while the wheel scrolls: must not switch
//   globe-probe type [count] [typeAfterMs]          tap 🌐, then type into our own field: no lost keys
//   globe-probe convert [plain|busy|paste|auto|terminal|terminal-cancel|terminal-interleave|terminal-english|terminal-mixed|terminal-punctuation|terminal-bracket|terminal-undo|auto-undo|terminal-undo-interleave]
//   globe-probe select <inputSourceID>              select a layout directly, e.g. com.apple.keylayout.ABC
import Carbon
import Cocoa

setvbuf(stdout, nil, _IONBF, 0)

func currentID() -> String {
    let src = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
    guard let p = TISGetInputSourceProperty(src, kTISPropertyInputSourceID) else { return "?" }
    return Unmanaged<CFString>.fromOpaque(p).takeUnretainedValue() as String
}

func short(_ id: String) -> String { id.replacingOccurrences(of: "com.apple.keylayout.", with: "") }

func switcherPID() -> String {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/bin/pgrep")
    p.arguments = ["-x", "TextInputSwitcher"]
    let pipe = Pipe()
    p.standardOutput = pipe
    try? p.run()
    p.waitUntilExit()
    let s = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
    return s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "dead" : s.trimmingCharacters(in: .whitespacesAndNewlines)
}

var changes: [(Date, String)] = []
DistributedNotificationCenter.default().addObserver(
    forName: NSNotification.Name(kTISNotifySelectedKeyboardInputSourceChanged as String),
    object: nil, queue: nil
) { _ in changes.append((Date(), currentID())) }

func pump(_ ms: Int) { RunLoop.current.run(until: Date().addingTimeInterval(Double(ms) / 1000)) }

let hid = CGEventSource(stateID: .hidSystemState)

func postFlags(_ keyDown: Bool) {
    let e = CGEvent(keyboardEventSource: hid, virtualKey: 63, keyDown: keyDown)!
    e.type = .flagsChanged
    e.flags = keyDown ? .maskSecondaryFn : []
    e.post(tap: .cghidEventTap)
}

func tapGlobe() {
    postFlags(true)
    usleep(60_000)
    postFlags(false)
}

// A harmless modifier-only "keystroke" (shift tap) to stand in for "start typing" without inserting text.
func tapShift() {
    for down in [true, false] {
        let e = CGEvent(keyboardEventSource: hid, virtualKey: 56, keyDown: down)!
        e.type = .flagsChanged
        e.flags = down ? .maskShift : []
        e.post(tap: .cghidEventTap)
        usleep(30_000)
    }
}

let args = CommandLine.arguments
switch args.count > 1 ? args[1] : "status" {
case "status":
    print("postEventAccess=\(CGPreflightPostEventAccess()) current=\(short(currentID())) switcher=\(switcherPID())")

case "select":
    let want = args[2]
    let list = TISCreateInputSourceList([kTISPropertyInputSourceID: want] as CFDictionary, false).takeRetainedValue() as! [TISInputSource]
    if let s = list.first { TISSelectInputSource(s) }
    pump(300)
    print("current=\(short(currentID()))")

case "press":
    let count = Int(args[2])!, gap = Int(args[3])!
    let typeAfter = args.count > 4 ? Int(args[4]) : nil
    pump(200)
    var before = currentID()
    print("start current=\(short(before)) switcher=\(switcherPID())")
    for i in 1...count {
        changes.removeAll()
        let pidBefore = switcherPID()
        let t0 = Date()
        tapGlobe()
        if let ta = typeAfter { pump(ta); tapShift() }
        pump(gap)
        let after = currentID()
        let firstMs = changes.first.map { Int($0.0.timeIntervalSince(t0) * 1000) }
        let trail = changes.map { "\(Int($0.0.timeIntervalSince(t0) * 1000))ms→\(short($0.1))" }.joined(separator: ", ")
        let verdict = after != before ? "SWITCHED" : (changes.isEmpty ? "MISSED" : "BOUNCED")
        print("#\(i) switcher=\(pidBefore) \(short(before))→\(short(after)) \(verdict) first=\(firstMs.map { "\($0)ms" } ?? "-") events=[\(trail)]")
        before = after
    }
    pump(1500)
    print("end current=\(short(currentID())) switcher=\(switcherPID()) late-events=\(changes.count)")

case "chord":
    // fn held while shift is tapped: must NOT switch.
    pump(200)
    let before = currentID()
    changes.removeAll()
    postFlags(true); usleep(60_000)
    tapShift()
    postFlags(false)
    pump(1000)
    print("chord \(short(before))→\(short(currentID())) \(currentID() == before && changes.isEmpty ? "NO-SWITCH(ok)" : "SWITCHED(bad)")")

case "hold":
    // fn held longer than the tap window: must NOT switch.
    pump(200)
    let before = currentID()
    changes.removeAll()
    postFlags(true); usleep(900_000); postFlags(false)
    pump(1000)
    print("hold \(short(before))→\(short(currentID())) \(currentID() == before && changes.isEmpty ? "NO-SWITCH(ok)" : "SWITCHED(bad)")")

case "scroll":
    // fn held while the wheel scrolls one line: must NOT switch.
    pump(200)
    let before = currentID()
    changes.removeAll()
    postFlags(true); usleep(60_000)
    CGEvent(scrollWheelEvent2Source: hid, units: .line, wheelCount: 1, wheel1: -1, wheel2: 0, wheel3: 0)?.post(tap: .cghidEventTap)
    usleep(30_000)
    postFlags(false)
    pump(1000)
    print("scroll \(short(before))→\(short(currentID())) \(currentID() == before && changes.isEmpty ? "NO-SWITCH(ok)" : "SWITCHED(bad)")")

case "overlay":
    // A normal-level window above the target, without keyboard focus, models
    // desktop overlays. It expires by itself even if the parent probe aborts.
    final class PassiveOverlay: NSPanel {
        override var canBecomeKey: Bool { false }
        override var canBecomeMain: Bool { false }
    }
    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)
    let panel = PassiveOverlay(contentRect: NSRect(x: 180, y: 180, width: 500, height: 120), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
    panel.level = .normal
    panel.backgroundColor = .clear
    panel.isOpaque = false
    panel.alphaValue = 0.02
    panel.ignoresMouseEvents = true
    panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
    panel.orderFrontRegardless()
    let end = Date().addingTimeInterval(5)
    while let event = app.nextEvent(matching: .any, until: end, inMode: .default, dequeue: true) { app.sendEvent(event) }
    panel.orderOut(nil)
    exit(0)

case "convert":
    let mode = args.count > 2 ? args[2] : "plain"
    let app = NSApplication.shared
    app.setActivationPolicy(.regular)
    app.finishLaunching()
    // NSTextField's standard editing shortcuts are dispatched through the
    // application's Edit menu, just as they are in a normal AppKit app.
    let menu = NSMenu()
    let applicationItem = NSMenuItem(title: "Probe", action: nil, keyEquivalent: "")
    applicationItem.submenu = NSMenu(title: "Probe")
    menu.addItem(applicationItem)
    let edit = NSMenu(title: "Edit")
    edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
    edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
    let editItem = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
    editItem.submenu = edit
    menu.addItem(editItem)
    app.mainMenu = menu
    var pasteEvents = 0
    var externalInput = false
    var keyTrace: [String] = []
    var traceStart = Date()
    var traceUptime = ProcessInfo.processInfo.systemUptime
    let testedUpyrPID = ProcessInfo.processInfo.environment["UPYR_PROBE_UPYR_PID"].flatMap(Int64.init)
    let pasteObserver = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
        if let cgEvent = event.cgEvent {
            let origin = cgEvent.getIntegerValueField(.eventSourceUnixProcessID)
            keyTrace.append("\(Int(Date().timeIntervalSince(traceStart) * 1000))(created:\(Int((event.timestamp - traceUptime) * 1000))):\(event.keyCode):\(origin == Int64(getpid()) ? "probe" : origin == testedUpyrPID ? "upyr" : "external")")
        }
        if let testedUpyrPID, let cgEvent = event.cgEvent {
            let origin = cgEvent.getIntegerValueField(.eventSourceUnixProcessID)
            if origin != Int64(getpid()) && origin != testedUpyrPID { externalInput = true }
        }
        if event.keyCode == 9 && event.modifierFlags.contains(.command) {
            pasteEvents += 1
        }
        return event
    }
    _ = pasteObserver
    func pumpEvents(_ ms: Int) {
        let until = Date().addingTimeInterval(Double(ms) / 1000)
        while let event = app.nextEvent(matching: .any, until: until, inMode: .default, dequeue: true) {
            app.sendEvent(event)
        }
    }
    let window = NSWindow(contentRect: NSRect(x: 200, y: 200, width: 420, height: 70), styleMask: [.titled], backing: .buffered, defer: false)
    window.title = "Upyr conversion probe"
    var field = NSTextField(frame: NSRect(x: 10, y: 15, width: 400, height: 28))
    field.stringValue = "ghbdsn"
    window.contentView?.addSubview(field)
    window.makeKeyAndOrderFront(nil)
    app.activate(ignoringOtherApps: true)
    window.makeFirstResponder(field)
    field.selectText(nil)
    pumpEvents(700)
    func ownsFocus() -> Bool {
        NSWorkspace.shared.frontmostApplication?.processIdentifier == getpid() && window.isKeyWindow
    }
    let focusWait = ProcessInfo.processInfo.environment["UPYR_PROBE_FOCUS_WAIT_SECONDS"].flatMap(Double.init) ?? 8
    let focusDeadline = Date().addingTimeInterval(focusWait)
    while !ownsFocus() && Date() < focusDeadline { pumpEvents(100) }
    guard ownsFocus() else { print("not run: the probe does not own keyboard focus"); exit(1) }
    func clipboardSnapshot() -> [[String: Data]] {
        (NSPasteboard.general.pasteboardItems ?? []).map { item in
            Dictionary(uniqueKeysWithValues: item.types.compactMap { type in
                item.data(forType: type).map { (type.rawValue, $0) }
            })
        }
    }
    func currentText() -> String { field.currentEditor()?.string ?? field.stringValue }
    let originalClipboard = clipboardSnapshot()
    func reportClipboardDifference() {
        let current = clipboardSnapshot()
        guard current != originalClipboard else { return }
        print("clipboard types before=\(originalClipboard.map { $0.keys.sorted() }) after=\(current.map { $0.keys.sorted() }); contains test replacement=\(NSPasteboard.general.string(forType: .string) == "привіт ")")
    }
    let initialRevision = NSPasteboard.general.changeCount
    if mode.hasPrefix("auto") || mode.hasPrefix("terminal") {
        let englishCase = ["terminal-english", "terminal-short", "terminal-shortest"].contains(mode) || mode.hasPrefix("terminal-slash")
        let sourceID = englishCase ? "com.apple.keylayout.Ukrainian-PC" : "com.apple.keylayout.ABC"
        let sources = TISCreateInputSourceList([kTISPropertyInputSourceID: sourceID] as CFDictionary, false).takeRetainedValue() as! [TISInputSource]
        guard let english = sources.first, TISSelectInputSource(english) == noErr else {
            print("not run: the automatic-correction probe needs the ABC layout")
            exit(1)
        }
        pumpEvents(200)
        // Discard any startup key that arrived while this window was opening,
        // in both the editor and Upyr's event tracker.
        for down in [true, false] {
            let escape = CGEvent(keyboardEventSource: hid, virtualKey: 53, keyDown: down)!
            escape.flags = []
            escape.post(tap: .cghidEventTap)
        }
        pumpEvents(100)
        field.stringValue = ""
        field.currentEditor()?.string = ""
        externalInput = false
        keyTrace.removeAll()
        traceStart = Date()
        traceUptime = ProcessInfo.processInfo.systemUptime
        if mode == "terminal-overlay" {
            let overlay = Process()
            overlay.executableURL = URL(fileURLWithPath: args[0])
            overlay.arguments = ["overlay"]
            try overlay.run()
            pumpEvents(350)
            let top = ((CGWindowListCopyWindowInfo(.optionOnScreenOnly, 0) as? [[String: Any]]) ?? []).first { window in
                let bounds = window[kCGWindowBounds as String] as? [String: Any]
                return (window[kCGWindowLayer as String] as? Int) == 0
                    && (bounds?["Width"] as? Double ?? 0) >= 64
                    && (bounds?["Height"] as? Double ?? 0) >= 48
            }
            guard (top?[kCGWindowOwnerPID as String] as? Int32) == overlay.processIdentifier else {
                print("not run: the passive overlay is not above the target")
                exit(1)
            }
            print("passive-overlay=above-target")
        }
        if mode.hasPrefix("terminal-slash") {
            // Return is delivered only to this owned, empty native test field.
            // It establishes a new line in Upyr; no shell or TUI receives it.
            for down in [true, false] {
                let enter = CGEvent(keyboardEventSource: hid, virtualKey: 36, keyDown: down)!
                enter.flags = []
                enter.post(tap: .cghidEventTap)
            }
            pumpEvents(100)
            if mode == "terminal-slash-undo-refocus" {
                // Same window, new actual editor. No pointer/key event resets
                // the tracked line; undo must bind to the field being edited.
                field.isHidden = true
                field = NSTextField(frame: NSRect(x: 10, y: 15, width: 400, height: 28))
                window.contentView?.addSubview(field)
            }
            window.makeFirstResponder(field)
            field.stringValue = ""
            field.currentEditor()?.string = ""
            pumpEvents(100)
        }
        let phrase: String
        let expectedText: String
        switch mode {
        case "terminal-slash-undo", "terminal-slash-undo-refocus":
            phrase = "/c"
            expectedText = ".с"
        case "terminal-slash", "terminal-slash-fast":
            phrase = "/command"
            expectedText = phrase
        case "terminal-slash-midline":
            phrase = "x /command"
            expectedText = "ч .сщььфтв"
        case "terminal-english":
            phrase = "it's not so good for english so far "
            expectedText = phrase
        case "terminal-short":
            phrase = "not so good for "
            expectedText = phrase
        case "terminal-shortest":
            phrase = "so good for "
            expectedText = phrase
        case "terminal-mixed":
            phrase = "it's not nfr lj,ht zr [jnskjcz , "
            expectedText = "it's not так добре як хотілося б "
        case "terminal-punctuation":
            phrase = "lj,ht "
            expectedText = "добре "
        case "terminal-bracket":
            phrase = "[jnskjcz "
            expectedText = "хотілося "
        case "terminal-undo", "auto-undo":
            phrase = "ghbdsn "
            expectedText = "ghbdsn "
        case "terminal-undo-punctuation":
            phrase = "lj,ht "
            expectedText = "lj,ht "
        case "terminal-undo-after-input":
            phrase = "ghbdsn "
            expectedText = "привіт ф"
        case "terminal-undo-after-navigation":
            phrase = "ghbdsn "
            expectedText = "привіт "
        case "terminal-letter-dot":
            phrase = "hjpevs. "
            expectedText = "розумію "
        default:
            phrase = "ghbdsn "
            expectedText = mode == "terminal-cancel" ? "ghbdsn a" : "привіт "
        }
        let keyCodes: [Character: CGKeyCode] = ["a":0,"b":11,"c":8,"d":2,"e":14,"f":3,"g":5,"h":4,"i":34,"j":38,"k":40,"l":37,"m":46,"n":45,"o":31,"p":35,"q":12,"r":15,"s":1,"t":17,"u":32,"v":9,"w":13,"x":7,"y":16,"z":6,"'":39,"[":33,"]":30,";":41,",":43,".":47,"/":44," ":49]
        for key in phrase.map({ keyCodes[$0]! }) {
            guard ownsFocus() else { print("not run: focus changed while typing"); exit(1) }
            guard !externalInput else { print("not run: external input reached the probe"); exit(1) }
            for down in [true, false] {
                let event = CGEvent(keyboardEventSource: hid, virtualKey: key, keyDown: down)!
                event.flags = []
                event.post(tap: .cghidEventTap)
            }
            if ["terminal-cancel", "terminal-interleave", "terminal-undo-interleave"].contains(mode) && key == 49 {
                if ["terminal-interleave", "terminal-undo-interleave"].contains(mode) { pumpEvents(45) }
                for down in [true, false] {
                    let next = CGEvent(keyboardEventSource: hid, virtualKey: 0, keyDown: down)!
                    next.flags = []
                    next.post(tap: .cghidEventTap)
                }
            }
            if mode == "terminal-feedback-already" && key == 49 {
                pumpEvents(10)
                let list = TISCreateInputSourceList([kTISPropertyInputSourceID: "com.apple.keylayout.Ukrainian-PC"] as CFDictionary, false).takeRetainedValue() as! [TISInputSource]
                if let source = list.first { TISSelectInputSource(source) }
            }
            pumpEvents(mode == "terminal-slash-fast" ? 0 : 70)
        }
        pumpEvents(1200)
        if mode.contains("undo") {
            guard ownsFocus(), !externalInput else { print("not run: lost the owned undo field"); exit(1) }
            if ["terminal-undo-after-input", "terminal-undo-after-navigation"].contains(mode) {
                let code: CGKeyCode = mode == "terminal-undo-after-input" ? 0 : 123
                for down in [true,false] {
                    let event = CGEvent(keyboardEventSource: hid, virtualKey: code, keyDown: down)!
                    event.flags = []; event.post(tap: .cghidEventTap)
                }
                pumpEvents(150)
            }
            func modifier(_ code: CGKeyCode, _ down: Bool, _ flags: CGEventFlags) {
                let event = CGEvent(keyboardEventSource: hid, virtualKey: code, keyDown: down)!
                event.type = .flagsChanged; event.flags = flags; event.post(tap: .cghidEventTap)
            }
            modifier(55,true,.maskCommand); pumpEvents(20)
            modifier(58,true,[.maskCommand,.maskAlternate]); pumpEvents(20)
            for down in [true,false] {
                let event = CGEvent(keyboardEventSource: hid, virtualKey: 6, keyDown: down)!
                event.flags = [.maskCommand,.maskAlternate]; event.post(tap: .cghidEventTap)
            }
            pumpEvents(80) // Exercise waiting for the user's shortcut modifiers.
            modifier(58,false,.maskCommand); modifier(55,false,[])
            pumpEvents(900)
        }
        guard !externalInput else { print("not run: external input reached the probe"); exit(1) }
        let textMatches = ["terminal-interleave", "terminal-undo-interleave"].contains(mode)
            ? ((currentText() == "ghbdsn a" && currentID() == "com.apple.keylayout.ABC")
                || (currentText() == "привіт ф" && currentID() == "com.apple.keylayout.Ukrainian-PC"))
            : currentText() == expectedText
        let needsABC = ["terminal-cancel","terminal-undo","terminal-undo-punctuation","auto-undo"].contains(mode)
        let needsUkr = ["terminal-slash-undo","terminal-slash-undo-refocus","terminal-undo-after-input","terminal-undo-after-navigation"].contains(mode)
        let layoutMatches = (!needsABC || currentID() == "com.apple.keylayout.ABC") && (!needsUkr || currentID() == "com.apple.keylayout.Ukrainian-PC")
        let wantsIndicator = mode.hasPrefix("terminal-feedback") || mode == "terminal-overlay" || ["terminal-slash", "terminal-slash-fast"].contains(mode)
        let indicatorVisible = !wantsIndicator || ((CGWindowListCopyWindowInfo(.optionOnScreenOnly, 0) as? [[String: Any]]) ?? []).contains { window in
            let owner = window[kCGWindowOwnerPID as String] as? Int64
            let bounds = window[kCGWindowBounds as String] as? [String: Any]
            let height = bounds?["Height"] as? Double ?? 0
            let layer = window[kCGWindowLayer as String] as? Int ?? 0
            return owner == testedUpyrPID && height >= 60 && layer > 0
        }
        let corrected = textMatches && layoutMatches && indicatorVisible && pasteEvents == (mode.hasPrefix("terminal") ? 0 : mode == "auto-undo" ? 2 : 1)
        let restored = clipboardSnapshot() == originalClipboard && (!mode.hasPrefix("terminal") || NSPasteboard.general.changeCount == initialRevision)
        if !restored { reportClipboardDifference() }
        if !corrected || ProcessInfo.processInfo.environment["UPYR_PROBE_TRACE"] == "1" {
            print("trace start Unix milliseconds: \(Int(traceStart.timeIntervalSince1970 * 1000))")
            print("key trace (time:key code:source): \(keyTrace.joined(separator: ", "))")
        }
        if !corrected { print("observed test field: \(String(reflecting: currentText()))") }
        print("automatic=\(corrected ? "ok" : "FAILED") focus=\(ownsFocus() ? "ok" : "STOLEN") clipboard=\(restored ? "ok" : "CHANGED")")
        if wantsIndicator { print("flag-indicator=\(indicatorVisible ? "ok" : "MISSING")") }
        exit(corrected && ownsFocus() && restored ? 0 : 1)
    }
    // The Space down starts the shortcut, and key-up triggers conversion.
    for down in [true, false] {
        let event = CGEvent(keyboardEventSource: hid, virtualKey: 49, keyDown: down)!
        event.flags = down ? [.maskCommand, .maskAlternate] : []
        event.post(tap: .cghidEventTap)
        pumpEvents(40)
    }
    if mode == "plain" {
        pumpEvents(1000)
        let restored = clipboardSnapshot() == originalClipboard
        if currentText() != "привіт" { print("observed test field: \(String(reflecting: currentText()))") }
        print("conversion=\(currentText() == "привіт" ? "ok" : "FAILED") focus=\(ownsFocus() ? "ok" : "STOLEN") clipboard=\(restored ? "ok" : "CHANGED")")
        exit(currentText() == "привіт" && ownsFocus() && restored ? 0 : 1)
    }
    // busy expects a deliberately long copy delay; paste waits for a confirmed
    // edit, then tests that the trailing clipboard delay cannot postpone 🌐.
    if mode == "busy" {
        let until = Date().addingTimeInterval(0.5)
        while NSPasteboard.general.changeCount == initialRevision && Date() < until { pumpEvents(10) }
        guard NSPasteboard.general.changeCount != initialRevision,
              NSPasteboard.general.string(forType: .string) == "ghbdsn" else {
            print("copy=FAILED; no pending copy phase to test")
            exit(1)
        }
    }
    if mode == "paste" {
        let until = Date().addingTimeInterval(3)
        while currentText() != "привіт" && Date() < until { pumpEvents(10) }
        guard currentText() == "привіт" else { print("conversion=FAILED"); exit(1) }
    }
    guard ownsFocus() else { print("not run: focus changed before the Globe tap"); exit(1) }
    changes.removeAll()
    let before = currentID()
    let started = Date()
    tapGlobe()
    pumpEvents(180)
    let elapsed = changes.first.map { max(0, Int($0.0.timeIntervalSince(started) * 1000) - 60) }
    let responsive = currentID() != before && elapsed.map { $0 < 150 } == true
    pumpEvents(mode == "busy" ? 1300 : 2000)
    let expected = mode == "busy" ? "ghbdsn" : "привіт"
    let restored = clipboardSnapshot() == originalClipboard
    print("globe=\(responsive ? "ok" : "SLOW") after-release=\(elapsed ?? -1)ms content=\(currentText() == expected ? "ok" : "CHANGED") focus=\(ownsFocus() ? "ok" : "STOLEN") clipboard=\(restored ? "ok" : "CHANGED")")
    exit(responsive && currentText() == expected && ownsFocus() && restored ? 0 : 1)

case "type":
    // Tap 🌐, then type a letter <typeAfterMs> later into a text field of our own, <count> times.
    // Every letter must arrive and our window must stay frontmost: nothing may take the keyboard,
    // the optional indicator included.
    let count = args.count > 2 ? Int(args[2])! : 5
    let typeAfter = args.count > 3 ? Int(args[3])! : 40
    let app = NSApplication.shared
    app.setActivationPolicy(.regular)
    app.finishLaunching()
    // RunLoop.run doesn't hand key events to windows; the application has to dispatch them.
    func pumpEvents(_ ms: Int) {
        let until = Date().addingTimeInterval(Double(ms) / 1000)
        while let event = app.nextEvent(matching: .any, until: until, inMode: .default, dequeue: true) {
            app.sendEvent(event)
        }
    }
    let window = NSWindow(contentRect: NSRect(x: 200, y: 200, width: 360, height: 60), styleMask: [.titled], backing: .buffered, defer: false)
    let field = NSTextField(frame: NSRect(x: 10, y: 15, width: 340, height: 28))
    window.contentView?.addSubview(field)
    window.makeKeyAndOrderFront(nil)
    app.activate(ignoringOtherApps: true)
    window.makeFirstResponder(field)
    pumpEvents(800)
    var indicatorSeen = false
    // Synthetic keys go to whichever app is in front. If that isn't us (the user is busy in another
    // app, and macOS 14+ won't hand over focus), stop rather than type into their window.
    func frontApp() -> String { NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? "?" }
    guard NSWorkspace.shared.frontmostApplication?.processIdentifier == getpid(), window.isKeyWindow else {
        print("not run: \(frontApp()) is in front, and the probe won't type into another app")
        exit(1)
    }
    for i in 1...count {
        let before = field.stringValue.count
        tapGlobe()
        pumpEvents(typeAfter)
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == getpid() else {
            print("#\(i) STOLEN by \(frontApp()) after the tap; stopped without typing")
            exit(1)
        }
        for down in [true, false] {
            let e = CGEvent(keyboardEventSource: hid, virtualKey: 0, keyDown: down)! // the A key
            e.post(tap: .cghidEventTap)
        }
        pumpEvents(150)
        let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
        indicatorSeen = indicatorSeen || windows.contains {
            ["Upyr", "upyr", "upyr-background"].contains($0[kCGWindowOwnerName as String] as? String ?? "")
        }
        let arrived = field.stringValue.count - before
        let front = NSWorkspace.shared.frontmostApplication?.processIdentifier == getpid()
        print("#\(i) layout=\(short(currentID())) typed=\(arrived == 1 ? "ok" : "LOST") front=\(front ? "ok" : "STOLEN")")
        pumpEvents(400)
    }
    print("text=\"\(field.stringValue)\" indicator=\(indicatorSeen ? "seen" : "not seen")")

default:
    print("usage: status | press <count> <gapMs> [typeAfterMs] | chord | hold | scroll | type [count] [typeAfterMs] | select <id>")
}

# Application layout preferences plan

- [x] Inspect application focus, input-source switching and settings persistence.
- [x] Implement activation observer, rule validation and pause/reload lifecycle.
- [x] Add native/pixel Applications controls and localization.
- [x] Test rules, saving/merging, settings layouts and platform compilation.
- [x] Build, back up and update the installed app; add the Raycast rule.
- [x] Follow actual AX keyboard focus for nonactivating panels; deduplicate
  focus sessions and observe the underlying app to detect panel dismissal.
- [x] Report macOS Secure Input independently of Accessibility permission.
- [x] Validate and install the focus-observer correction.
- [ ] Verify real Raycast activation and manual override, record evidence.

Initial user verification failed. Read-only diagnostics found Secure Input
enabled and owned by loginwindow while configuration and Accessibility were
correct. Lock/unlock did not release it. During the user's manual typing check,
a text-free global monitor received zero key events, while AX focus events
continued arriving. A macOS sign-out/sign-in remains a user action.

On 2026-11-15 the user opened Raycast during passive observation. AX reported
Raycast's PID while NSWorkspace still reported Paradev; the updated installed
app logged a successful switch to ABC. Dismissal returned AX focus to Paradev.
Manual override/reopen behavior has a regression test, but its full live matrix
remains deferred. No focus-changing input was automated during diagnosis.

Validation: 305 library tests + 3 CLI tests passed (2 unrelated tests ignored),
macOS and Windows Clippy passed, privacy guard and formatting passed. The
installed update preserved the configuration byte-for-byte and its previous
bundle was backed up locally.

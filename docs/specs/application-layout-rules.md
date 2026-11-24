# Application layout preferences

When Raycast opens, including with Command+Space, select or retain an English
input source. Implement this as a configurable macOS application rule, observed
through application activation and Accessibility focus notifications. Panels
can receive keyboard focus without activating their application. Do not register or consume the
launcher shortcut. Other applications can prefer English or Ukrainian too.

- Match exact case-insensitive bundle IDs or display names; bundle ID wins.
- Apply once on entering an application's keyboard focus, startup, or a change to the rules. A manual layout
  choice lasts until focus leaves and returns. Observe configured apps plus the
  underlying foreground app, so closing/reopening a nonactivating panel starts
  a new focus session. Ignore duplicate focus notifications. No keyboard polling, typing history,
  delayed retries or forced restoration in an unrelated app.
- Keep the existing English variant if already active. If the requested family
  is unavailable, leave the current source alone and report the failure.
- Work independently of automatic correction; honor the global Pause control.
- Empty defaults preserve existing behavior and config compatibility. Add only
  the requested Raycast rule to the user's installed configuration.
- Add a Ukrainian/English Applications settings tab in native and pixel editors:
  editable app identity, layout choice, add/remove, and a Raycast preset.
- Validate identities, duplicates and bounds; preserve rules through ordinary
  saves and concurrent settings edits.

Validate policy/config round trips, both settings frontends, and real Raycast
activation from Ukrainian and English before claiming live behavior. Preserve
the installed app and configuration in a backup when updating this Mac.

Diagnostics must distinguish granted Accessibility from macOS Secure Input,
which can independently block global keyboard observation. Report the latter
without disabling it or attempting to intercept protected keystrokes.

# Manual update check

Add **Check for updates** to Settings → About in the pixel GUI and the native
desktop editor. Ukrainian and English share the same behavior. A click starts
one worker; opening settings, launching Upyr, polling UI state, and editing text
never start a request. No schedule, persisted opt-in, background startup check,
automatic download, installer, or configuration migration is added.

Read the public Upyr GitHub releases API over verified HTTPS, without credentials,
typed text, configuration, device IDs, or telemetry. Limit the request to 15
seconds and the response to 2 MiB; forbid redirects. Check the newest semantic
version among the 100 returned public releases with an asset for this platform.
Recognize `vX.Y.Z` and `preview-X.Y.Z`; previews are explicitly labelled. Compare
version precedence, not dates or strings. Same-version rebuilds are not updates.

Show idle, checking, newer version, no newer version, and failure states. Prevent
duplicate checks while one is running. Errors permit a manual retry. Only a
separate **Open release** action opens the validated project's release page.
Nothing is downloaded or installed by the checker.

Update privacy claims and narrowly exempt this settings module from the network
guard. Keep the engine, keyboard monitor, WASM and website network restrictions.
Verify release selection, malformed/empty responses, worker state, duplicate
clicks, UI labels, privacy regression tests, builds and linting. Live network
verification is explicit and excluded from the ordinary test suite.

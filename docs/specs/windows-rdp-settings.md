# Windows settings in RDP sessions

Windows 11 Pro over RDP was reported to keep the Upyr background process alive
while settings briefly opened and closed, and automatic switching did not work.
The precise remote error is pending. Source inspection found that both settings
frontends required OpenGL; the existing installer smoke test only exercised CLI
commands. A fresh configuration also intentionally has automatic correction off.

Windows GUI settings must use system Direct3D 12 through wgpu, allowing WARP on
machines without a compatible hardware adapter. macOS/Linux retain their current
renderer. No external graphics packages or automatic downloads are required.
Settings startup errors must remain visible for tray launches and readable when
launched from a terminal. The installed pixel and standard windows must stay
responsive through renderer initialization and close successfully in Windows CI.

RDP clients must send physical scancodes for automatic layout switching: Unicode
input is translated using the client's layout before reaching Upyr. Document
the client setting and report the remote-session indicator in `upyr doctor`.
Do not silently enable correction or change the user's RDP preferences.

Validation on the actual HetzerWin session remains required before claiming
that its reported failures are resolved. A passing Windows CI window test is
separate evidence, not an interactive RDP test.

References:
- [Windows App keyboard modes](https://learn.microsoft.com/en-us/windows-app/input-keyboard-mouse-touch-pen?tabs=macos#keyboard-modes)
- [Microsoft WARP](https://learn.microsoft.com/en-us/windows/win32/direct3darticles/directx-warp)

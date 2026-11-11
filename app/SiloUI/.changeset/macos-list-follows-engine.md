---
"silo-ui": patch
---

The list of macOS computers keeps up with changes made outside the app: it refreshes every few seconds while a computer is being prepared, started or stopped, every 30 seconds otherwise, and when the window is focused again. A computer made with `silo create --macos` now appears, and one stuck at "Preparing" becomes usable without relaunching.

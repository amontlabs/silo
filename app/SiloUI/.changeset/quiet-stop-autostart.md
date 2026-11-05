---
"silo-ui": patch
---

A fork recipe's `quiet` list now also turns off desktop autostart entries and user services on Linux, and LaunchAgents and LaunchDaemons on macOS, that start the named program, and `silo status --json` shows the boot count of a macOS fork.

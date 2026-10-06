---
"silo-ui": patch
---

- Status checks of connected devices reuse one SSH connection per device instead of reconnecting for every request, so Connections stays responsive with several devices.
- Replies from another device are noticed as soon as they arrive.

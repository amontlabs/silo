---
"silo-ui": minor
---

Drive another Mac's computers from this one over Connections: `silo --device <name>` runs fork, diff, fold, requests, steps and tree on that device, `silo list --all-devices` and `silo requests --all-devices` give one list across devices (a device that cannot be reached is reported, not fatal), and a request is kept, thrown away or opened on the device it is on. The MCP tools take `device` and `allDevices`.

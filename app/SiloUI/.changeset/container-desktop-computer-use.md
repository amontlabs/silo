---
"silo-ui": minor
---

Container computers now have a desktop and computer use, the same as Linux VMs: the desktop starts with the computer unless you turn it off (`silo desktop off`, the computer's settings, or `silo create --no-desktop` for a Linux or container computer), the viewer, `fork --gui` and the computer-use tools work, and an agent can drive it. A container's browser runs without its own sandbox, so a web page that compromises the browser reaches everything in that computer, including its GitHub access, its secrets and its `silo` client; the computer's page says so. Silo also stops a container computer whose size it cannot measure when the host is short of disk space.

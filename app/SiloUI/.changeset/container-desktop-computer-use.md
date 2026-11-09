---
"silo-ui": minor
---

Container computers now have a desktop and computer use, the same as Linux VMs: the desktop starts with the computer unless you turn it off (`silo desktop off`, or the computer's settings), the viewer, `fork --gui` and the computer-use tools work, and an agent can drive it. A container's browser runs without its own sandbox, so a web page that compromises the browser reaches everything in that computer; the computer's page says so. Silo also stops a container computer whose size it cannot measure when the host is short of disk space.

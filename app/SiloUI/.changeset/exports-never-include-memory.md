---
"silo-ui": patch
---

Exports never include a computer's memory. A running macOS computer is exported from its disks (it is paused only while they are copied), and a macOS checkpoint that saved memory is exported without it. A Linux VM checkpoint that was saved while the computer ran cannot be exported: Silo says so and names what to do instead (fork the checkpoint and export the fork, or export the computer itself). Two copies of an export from another device to the same file no longer share a temporary file.

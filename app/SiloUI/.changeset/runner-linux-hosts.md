---
"silo-ui": minor
---

A Linux host that runs `silod` can be a CI runner machine. The Linux engine archives (x86_64 and aarch64) now include `silo-runner`; `silo-runner service register` installs a systemd user service that starts after the engine, and `silo-runner check -offline` checks `/dev/kvm` and the Linux budget without contacting GitHub. On a Linux host, a `linuxBudget` field left out defaults to the host's: three quarters of its memory and one computer per CPU. The `silo-linux` recipe now downloads the runner for the computer's architecture (arm64 or x64) and checks its SHA-256. See the CI runner guide's Linux hosts section.

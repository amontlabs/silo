---
"silo-ui": minor
---

`silo --device <name> export <computer> -o FILE` and `silo --device <name> import FILE` now work with a computer on another connected device. The file is copied over the existing Connections link, with a size and checksum check, and the other device keeps nothing afterwards. Silo asks about logins on the computer as it does for a local export.

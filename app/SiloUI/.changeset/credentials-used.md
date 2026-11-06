---
"silo-ui": minor
---

`silo status` and `silo diff` now show which Silo-managed secrets a fork used, per host and method, and how many attempts were blocked, for Linux and macOS computers. The new engine method `computers.credentials` reads the events the network proxy records; they hold names, hosts and methods, never values.

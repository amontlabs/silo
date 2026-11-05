---
"silo-ui": patch
---

Making a fork main no longer fails with an ordering error. `silo` can list, publish and unpublish a computer's ports, forks publish the ports of their source, and `silo up` makes `ssh main.silo` work.

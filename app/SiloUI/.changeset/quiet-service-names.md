---
"silo-ui": patch
---

A recipe's `quiet:` entry that names one of its `services:` now also stops that service's running process in a fork (for `python3 /opt/app/poller.py`, the `poller.py` process), not only its start-at-boot entry.

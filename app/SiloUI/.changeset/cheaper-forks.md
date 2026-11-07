---
"silo-ui": minor
---

Forks are cheaper. A fork starts from the computer as it is now: Silo saves its disks only (flushed from the running computer, never its memory) and `silo fork -n N` shares one such capture among the N forks. `silo up` no longer saves a warm checkpoint, removes the one an earlier version left, and the `warm_max_age` setting is gone: delete that line from `.silo/silo.yaml`. `silo status` no longer shows a `generation`.

A Linux computer can start no desktop. A recipe can say `desktop: false` (not together with `services:`), the Desktop switch in a computer's settings turns it off or on, and `silo desktop on|off <computer>` does the same on a running computer. Its forks inherit the choice. `silo fork --gui`, the computer-use tools and `silo status` say `<name> has no desktop; turn it on with silo desktop on <name>`. A macOS computer is its own display and always has one.

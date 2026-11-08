---
"silo-ui": minor
---

Container computers (a Linux host's opt-in, less isolated kind) now have SSH access, an editor connection, a terminal and the in-guest client, through the same screens, commands and `silo ssh-setup` as a virtual machine. Silo serves SSH itself, on the host, as the computer's `silo` account (including SFTP and terminals). A container computer made by an earlier development build is refused at start with a message to recreate it.

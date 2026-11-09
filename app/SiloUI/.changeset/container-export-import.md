---
"silo-ui": minor
---

A container computer can now be exported to a `.silo-backup` file and imported under a new name, on the same Linux device or another one, from the app and with `silo export` and `silo import`, exactly like a virtual machine computer, and the same works across Connections with `--device`. The login warning is the same and nothing is left out of the file. A container's export does not carry the guest image, so both devices need the same Silo guest image; importing it on a device that cannot run containers is refused with a plain message. Checkpoints of a container still save the disks only.

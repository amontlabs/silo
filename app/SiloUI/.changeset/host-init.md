---
"silo-ui": minor
---

Any Mac or Linux machine can host computers for another device. `silo host init` checks that the machine can run computers, starts the engine as a service, puts `silo` on PATH, turns on Connections, accepts controller keys given with `--authorize` and prints the address to add it by; `silo host status` shows how it is set up. On the controlling device, `silo devices add <user@host>` connects it (with `--host-key` to trust its SSH key without a prompt), and `silo devices list`, `remove` and `key` manage connected devices. Settings, Connections now shows when the `silo` command could not be put on PATH. A connected device can revoke only its own key on the host; `silo host revoke` removes any controller key there, including lines Silo did not write.

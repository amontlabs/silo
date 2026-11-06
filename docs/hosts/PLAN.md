# Remote hosts: build plan

Goal: any machine prepared with the `silo` CLI hosts any number of computers, using the kinds it
can really run. A Mac hosts Linux and macOS VMs (Apple allows 2 macOS VMs at once per Mac). A
Linux server hosts Linux VMs over KVM. A host is managed from another machine through
Connections, and each host keeps its own secrets and GitHub sign-in. Containers, CI runners and
sending secrets between machines are out of scope.

Design source: "Silo: remote hosts, light forks, CI, containers", point 1 (6 Nov 2026).

One pull request per phase, merged when CI is green and the reviews pass.

| Phase | What | Live check |
|---|---|---|
| 1 | Connections works on main: silod handles the `--remote-bridge` forced command, runs from its bundle when SSH starts it through the `~/.local/bin` link, and reads a reply the owner wrote just before closing | Mac: the Dev bridge answers through the link and through a real sshd forced command |
| 2 | silod builds and runs on Linux: Apple code behind `cfg`, a plain run loop, the manifest beside the binary, XDG paths, `SO_PEERCRED`, a file secret store for headless hosts, no GTK in the daemon; silod and the CLI tested on Linux in CI | Linux: silod starts, `silo list` answers |
| 3 | Linux engine packaging: `silod --service` installs a systemd user unit, `silo-engine-<v>-linux-x64` and `-linux-arm64` archives, a CI job that starts a real Linux computer on a KVM runner | KVM runner: create, start, exec, fork, drop |
| 4 | `silo host init`: checks the machine can run VMs, installs the engine as a service that starts on boot, turns on Connections, authorizes a controller key, prints the address. `silo host status`. The controller adds the host by address from the CLI or the app | Linux and Mac: init, then add from another engine |
| 5 | `exec`, `fork -- <cmd>` and the computer-use tools reach computers on other machines (the engine already routes them; the CLI and MCP refuse) | Controller to host: exec, fork with a command, a screenshot |
| 6 | Threat model: Linux host section (KVM, the socket, the file secret store, the bridge). End-to-end two-machine run and docs | Mac to Linux host, whole flow |

Live checks use the Dev build only, never the production app or its data. Test computers are
named `e2e-hosts-*` and dropped at the end of each check.

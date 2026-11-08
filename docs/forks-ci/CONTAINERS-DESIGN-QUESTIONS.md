# Container computers: design questions (8 Nov 2026)

Owner's decision (BUILD_LOG Decisions, "Container computers"): build an opt-in container computer kind for
Linux hosts, never the default, with the same API and capabilities as a VM computer: create, start, stop,
exec, fork, checkpoints, diff and keep, secrets (placeholders swapped by a proxy), GitHub access, ports and
URLs, desktop and computer use, export and import, Connections; same CLI, MCP tools, app screens, behaviour.
Measurements and the hardening baseline: [evidence/c1-containers](evidence/c1-containers/README.md) and the
brief's baseline (rootless crun, caps dropped, no-new-privileges, seccomp deny-list, Landlock via an init
shim, own netns with veth and nftables default-deny, outbound only through a host-side proxy, read-only root
with a per-computer CoW upper layer, masked /proc and /sys, cgroup limits, no Docker/Podman socket, no host
folders, never the engine socket). Prefer maintained existing tools over custom infrastructure (AGENTS.md
"Reuse established tools"; research and record the choice in docs/).

Questions, for the simplest design that is easy to explain and obvious in hindsight:

1. **Where the kind plugs in.** Linux VMs go through MicroSandbox (`engine/src/runtime/`, `MsbRunner`),
   macOS VMs through `engine/src/macos_computers/`. Is a container computer a third backend behind the same
   computer, checkpoint, guest exec and file traits the engine already has, so the API and app need only a
   new `kind`? What is the smallest seam, and which engine paths assume a VM (guest bridge, `/.msb`,
   heartbeat, SSH alias, desktop service, computer use driver, quiet forks, change capture)?
2. **The secret proxy (prerequisite).** Placeholders are swapped today inside MicroSandbox's VM networking.
   Containers need a host-side proxy. Options: an existing maintained proxy that can substitute header
   values for allowed hosts after verifying TLS (and mint a per-computer CA), or MicroSandbox's own proxy code
   run as a host process. Should VMs move to the same host proxy too (one mechanism), or keep theirs?
3. **Disk, checkpoints and forks.** Overlay upper layers per computer over the pinned guest image rootfs.
   Checkpoint = a copy (reflink where the filesystem allows) of the upper layer; fork = a new upper layer from
   a checkpoint; diff and keep reuse the change manifest by listing upper layers (both directions; C1 found
   deletions of parent-only files leave no whiteout). Is that right, and is there a maintained tool for
   snapshotting overlay layers (e.g. what containerd's snapshotters do) worth reusing?
4. **Network.** Rootless netns with a veth pair needs a root step (C1); slirp4netns or pasta (passt) avoid
   root. Which gives default-deny plus "outbound only through the host proxy" plus published ports most simply?
5. **Desktop and computer use.** The guest desktop works under the baseline (C1); Chrome only with
   `--no-sandbox`. Same desktop service and Cua Driver as the VM guest? What does `--no-sandbox` mean for the
   threat model, and is there a way to keep Chrome's sandbox (a nested user namespace allowed only for it)?
6. **Phases.** Propose an order of pull requests, each shippable and tested (unit and a KVM CI live check),
   from the host proxy to full parity, with what each one delivers.

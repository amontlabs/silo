# Container computers: build plan

Owner's decision (8 Nov 2026, [BUILD_LOG](BUILD_LOG.md) Decisions): an opt-in container computer kind for
Linux hosts, never the default, chosen knowingly for a dev-only machine with nothing sensitive on it, with the
same API and capabilities as a VM computer. Measurements: [evidence/c1-containers](evidence/c1-containers/README.md).
Design questions: [CONTAINERS-DESIGN-QUESTIONS.md](CONTAINERS-DESIGN-QUESTIONS.md); advised by Fable 5.1 and
the advisor (gemini-4-argon), 8 Nov 2026. Where they differed, the choice and the reason are below.

## Design

- **What a container computer is.** A Linux computer whose runtime is crun instead of MicroSandbox. The
  computer configuration carries the runtime; one new module, `engine/src/containers/`, answers the same
  operations the engine asks MicroSandbox for (create, start, stop, exec, secrets, snapshot, export, import,
  SSH), chosen by one match where the engine calls `msb`. Everything above that (guest exec, files, change
  capture, keep, computer use, desktop, secrets material, checkpoints, backups) is reused. The API adds
  `ComputerKind::Container`; the CLI gets `silo create --container`; the app shows the kind with its warning.
  (Fable; the advisor proposed a sibling owner module, which would duplicate the layers above `msb`.)
- **Network and secrets: one stack, two carriers.** `silo-macos-gateway` already runs MicroSandbox's network
  stack (the Public policy, header substitution after upstream TLS verification, a per-computer CA, DNS,
  published ports) as a host process for macOS VMs. It keeps that name and also runs on Linux; for a
  container its frames come from a TAP device created inside the container's own user and network namespace
  (no root step, no veth, no nftables). The namespace holds only the TAP, so the gateway is the only way out.
  Linux VMs keep MicroSandbox's in-process stack: moving them would change boundaries 2 and 3 for every
  existing computer for no user gain. (Both.)
- **Disk, checkpoints, forks.** The pinned guest image is unpacked once per digest with skopeo and
  `umoci unpack --rootless` (both Apache-2.0). Each computer runs on an overlayfs upper directory over a chain
  of sealed layers: a checkpoint seals the current upper into the chain and starts an empty one; a fork is an
  empty upper over the parent's chain up to that checkpoint. Nothing is copied, so checkpoints and forks are
  instant on any filesystem. The chain has a length cap; a maintenance step flattens the oldest layers.
  Diff and keep run the existing change capture through guest exec, as for VMs; they do not read layers.
  (Fable; the advisor's copy of the upper layer per checkpoint costs time proportional to the computer's
  changes on filesystems without reflink.)
- **No memory checkpoints.** Containers have disk-only checkpoints; memory kinds are refused with a clear
  error (CRIU would be a special case). Forks never carry memory anyway (owner decision 3).
- **Hardening baseline** (owner's brief): rootless crun, all capabilities dropped, no-new-privileges, a seccomp
  deny-list (io_uring, nested user namespaces, keyctl, bpf, perf_event, userfaultfd), Landlock applied by
  Silo's init shim (which also reaps zombies as PID 1), read-only root with the overlay upper, masked `/proc`
  and `/sys` paths, cgroup v2 limits through `systemd-run --user --scope -p Delegate=yes`, no Docker or Podman
  socket, no host folders, never the engine socket.
- **Desktop and computer use.** The same desktop service, Selkies and Cua Driver as the VM guest. Chrome runs
  with `--no-sandbox`: the baseline blocks the nested user namespace its sandbox needs, and allowing it for
  Chrome alone cannot be done safely. The kind's label, the app and the threat model say that a web page that
  compromises the browser reaches everything in the computer. (Both.)
- **Host prerequisites,** checked and named by `silo host init`, never set by Silo: `/etc/subuid` and
  `/etc/subgid` entries, `/dev/net/tun`, Ubuntu's `kernel.apparmor_restrict_unprivileged_userns=0`, cgroup v2
  with user delegation.

## Planned work not yet in a phase

- **Writable-layer size limit.** No rootless bound exists (a project quota and a loop file both need
  administrator rights; a per-computer read-only remount was built in CT4 and removed after review: it fails with
  open writers, has no way back after a restart, is reset by a checkpoint and misses unreadable directories). A
  container's size is reported, not a bound. CT4 protects the host instead: below a margin of free space on the
  engine's file system (5 GiB, `SILO_CONTAINER_DISK_MARGIN_MIB`) it refuses to start, checkpoint or fork a
  container computer and stops the running ones over their stated size, largest first (see the threat model).

## Phases

One pull request each, with unit tests and a live check on the GitHub KVM runner (`linux-host.yml`), merged
when CI is green and the code and Fable reviews (with a threat-model review) pass.

| Phase | What | Live check | Status |
|---|---|---|---|
| CT1 | `silo-macos-gateway` on Linux: lift the build and `cfg` gates, TAP carrier, CA in the Linux credential file store | A bare user+net namespace with a TAP: curl through the gateway reaches a public host; a private address is refused; a placeholder header is substituted for an allowed host | Built ([#92](https://github.com/amontlabs/silo/pull/92), [evidence](evidence/ct1-gateway/README.md)); the binary keeps its name; the gateway fails closed, so a failed update stops it (CT3 adds restart) |
| CT2 | Container runtime core: `engine/src/containers/`, image unpack, overlay chain, crun config (`oci-spec` crate), cgroup delegation, create/start/exec/stop/drop, `kind: container` in API, CLI and app list; `silo host init` checks and names the host prerequisites, including `kernel.apparmor_restrict_unprivileged_userns=0` (CT1's CI sets it by hand) | `linux-host-e2e.sh` with a container computer | Built (this branch; [BUILD_LOG](BUILD_LOG.md#8-nov-2026-phase-ct2-built-the-container-runtime-core)) |
| CT3 | Network, secrets, GitHub access, ports and URLs, for containers | The CT1 checks from inside a container; a published port answers | Built ([#95](https://github.com/amontlabs/silo/pull/95), [BUILD_LOG](BUILD_LOG.md#8-nov-2026-phase-ct3-built-in-part-network-features-for-container-computers)); SSH, the editor, the terminal and the in-guest client moved to CT3b |
| CT3b | SSH access, the editor handoff, the terminal and the in-guest client for containers: `ssh serve` and `exec --tty` answered by the engine's own russh server through `runtime::command_for`; the engine's user is the working account in the container (keep-id), so the in-guest spool works without stat virtualization | `silo ssh-setup` then `ssh NAME.silo-dev uname -a`; an sftp put and get; `ssh -tt ... tty`; the terminal command on a pty; the editor proxy in `--stdio` mode; the in-guest `silo` answering a read call as the `silo` account | Built (this branch; [BUILD_LOG](BUILD_LOG.md#8-nov-2026-phase-ct3b-built-ssh-the-editor-the-terminal-and-the-in-guest-client-for-container-computers)) |
| CT4 | Checkpoints, forks, quiet forks, diff, keep, undo, retention; a container survives an engine restart; a host short of disk space is protected | Checkpoint a running container, restore, fork `-n 4`, exec, diff lists a change, keep, undo, quiet fork, engine restart, fill the host past the margin | Built (this branch; [BUILD_LOG](BUILD_LOG.md#9-nov-2026-phase-ct4-built-checkpoints-forks-and-restart-for-container-computers)) |
| CT5 | Desktop and computer use; the app's container kind and warning | Desktop on, viewer answers, Cua Driver lists windows, a screenshot artifact | Built (this branch; [BUILD_LOG](BUILD_LOG.md#9-nov-2026-phase-ct5-built-the-desktop-and-computer-use-for-container-computers)) |
| CT6 | Export, import, Connections, threat-model section, vocabulary | Export, import under a new name, exec; the two-account run with a container | Built ([#104](https://github.com/amontlabs/silo/pull/104), [BUILD_LOG](BUILD_LOG.md#9-nov-2026-phase-ct6-built-export-import-and-connections-for-container-computers)); a container exports and imports like a VM, on the device or across Connections, in format 6; only the memory in checkpoints is missing |

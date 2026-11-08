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

## Phases

One pull request each, with unit tests and a live check on the GitHub KVM runner (`linux-host.yml`), merged
when CI is green and the code and Fable reviews (with a threat-model review) pass.

| Phase | What | Live check | Status |
|---|---|---|---|
| CT1 | `silo-macos-gateway` on Linux: lift the build and `cfg` gates, TAP carrier, CA in the Linux credential file store | A bare user+net namespace with a TAP: curl through the gateway reaches a public host; a private address is refused; a placeholder header is substituted for an allowed host | Built ([#92](https://github.com/amontlabs/silo/pull/92), [evidence](evidence/ct1-gateway/README.md)); the binary keeps its name; the gateway fails closed, so a failed update stops it (CT3 adds restart) |
| CT2 | Container runtime core: `engine/src/containers/`, image unpack, overlay chain, crun config (`oci-spec` crate), cgroup delegation, create/start/exec/stop/drop, `kind: container` in API, CLI and app list; `silo host init` checks and names the host prerequisites, including `kernel.apparmor_restrict_unprivileged_userns=0` (CT1's CI sets it by hand) | `linux-host-e2e.sh` with a container computer | Planned |
| CT3 | Network, secrets, GitHub access, ports and URLs, SSH, the in-guest client, for containers | The CT1 checks from inside a container; a published port answers; `ssh` works | Planned |
| CT4 | Checkpoints, forks, quiet forks, diff, keep, undo, retention | Checkpoint, fork `-n 4`, exec, diff lists a change, keep, undo | Planned |
| CT5 | Desktop and computer use; the app's container kind and warning | Desktop on, viewer answers, Cua Driver lists windows, a screenshot artifact | Planned |
| CT6 | Export, import, Connections, threat-model section, vocabulary | Export, import under a new name, exec; the two-account run with a container | Planned |

# Forks, sharing, CI runner, container computers: build plan

Design source: the owner's brief of 7 Nov 2026 ("Build the next Silo features: forks and sharing,
CI runner, container computers"). Working method: [`docs/v1/`](../v1/BUILD_LOG.md), one pull
request per phase, squash-merged through GitHub when CI is green and the reviews pass (code
review and, for any phase that touches a boundary, a threat-model review). Progress, decisions
and evidence are in [BUILD_LOG.md](BUILD_LOG.md).

Remote hosts ([`docs/hosts/PLAN.md`](../hosts/PLAN.md)) are treated as done enough on the owner's
call (7 Nov); its Mac live checks stay open there.

## Model

- **Machine:** anything with Silo installed. A Mac offers Linux and macOS VMs (Apple allows 2
  macOS VMs at once per Mac); a Linux server offers Linux VMs, and later containers (part 3).
- **Computer:** a VM, its own world, where the agent is admin.
- **Checkpoint:** a saved point of a computer you can go back to.
- **Fork:** a new computer started from a computer or a checkpoint. It starts fresh (cold boot from
  the checkpoint's disks), quiet, with no secrets unless named.

## Phases

| Phase | Part | What | Status |
|---|---|---|---|
| F1 | 1 | Fork measurements, before: one script that times a fork and measures its memory on a Mac and on a Linux KVM host (cold fork, `-n 4` from the warm checkpoint, macOS fork, RAM per fork) | Merged, PR #84 |
| F2 | 1 | Cheaper forks: a fork starts from the source as it is, captured disks-only; forks without a desktop. Measured after with the F1 script | Merged, PR #88 |
| F3 | 1 | Sharing: `silo export` / `silo import`, the guest image by reference, a warning for logins found on the computer, keep an imported computer into main | Merged, PR #86 |
| F4 | 1 | Sharing a computer that lives on another machine | Planned |
| R1 | 2 | CI runner on the Mac: delete `silo-action/`, the Go helper on actions/scaleset, a Linux and a macOS runner recipe, a fresh computer per job, delete on success, keep on failure, private repositories only, example workflow | Merged, PR #83 |
| R2 | 2 | Linux hosts as runner machines | Planned |
| C1 | 3 | Container computers with crun: measurements only, reported to the owner before any build | Merged, PR #87; [evidence](evidence/c1-containers/README.md), the owner decides on a build |

Later, not in this plan: outbound rules per runner label and per-job network logs (after roadmap
milestone 2); one runner helper for several machines.

Not built, on purpose: containers inside a VM, sharing as a change set, memory forks, live share
links, macOS user accounts as light computers.

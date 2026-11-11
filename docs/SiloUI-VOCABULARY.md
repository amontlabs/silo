# Silo vocabulary

Silo gives agents computers. This page fixes the words the app, its code, its
saved data and its documentation use, and records the identifiers that keep an
older name on purpose.

## Terms

| Term | Meaning | Examples |
| --- | --- | --- |
| **computer** | A virtual computer Silo creates and runs for an agent. Its saved settings are its **configuration**. | New computer, Start the computer, `ApplicationComputer`, `ComputerConfiguration`, `computerId` |
| **container computer** | A Linux computer that runs as a container on a Linux host instead of a VM. It shares the host's kernel, so it is less isolated than a VM. Never the default. It has everything a VM computer has (create, start, exec, files, checkpoints, forks, diff and keep, secrets, GitHub access, ports and URLs, desktop and computer use, export and import, Connections) except the memory in its checkpoints. | Container, Less isolated than a VM, `runtime: "container"`, `ComputerKind::Container` |
| **layer** | One folder of files that a container computer's disk is made of. A checkpoint seals the writable layer, a fork shares the sealed ones, an export writes them. Internal: the app and the command line say "disks" and "checkpoint". | `containers/layers`, `silo-ctr pack` |
| **export file** | A `.silo-backup` file that holds a whole computer, a VM or a container, to share or keep. **Import** makes a new, stopped computer from it, on this device or another. A container's file does not carry the guest image, so both devices need the same one. | Export, Import, `silo export`, `silo import`, `.silo-backup` |
| **device** | A physical Mac or Linux machine that runs Silo. The local one is **this device**; elsewhere the UI shows the device's name. | This device, Computers on linux-box, `Device`, `deviceId` |
| **Connections** | The feature that links devices so one Silo manages computers on another. A computer on another device is a **remote computer**. | Connections settings, Connect device, `remote_computer_action` |
| **workspace** | Only the persistent `/workspace` folder and disk inside each computer. | Workspace files, `workspaceStorageGiB`, `read_workspace_storage` |
| **sandboxed** | The isolation property of a computer, never the object itself. | "Each computer is sandboxed from your device." |
| **computer use** | Agents controlling a computer's graphical desktop. | Computer use, `computerUseAutoApproval` |
| **fork** | A new computer made from another computer's state. Its disks are a copy; nothing runs in it that was not run there. | `silo fork`, Fork |
| **checkpoint** | A saved state of a computer that you can go back to or fork from. | `silo checkpoint`, `silo restore` |
| **steps** | The commands run in a computer through Silo, in order. | `silo steps` |
| **diff** | Everything a fork changed: code, packages, settings, files and databases. | `silo diff` |
| **merge** | Bring a fork's changes into the computer it came from. Items only the fork changed come in; an item both changed is a conflict and you choose a side. Code goes through Git, so a conflict arrives as a branch. A database is never combined. | `silo merge`, Merge, the merge tool |
| **fast-forward** | The kind of merge where main has not changed since the fork, so nothing can conflict and every change is applied. Main keeps running and no computer is swapped. | "Nothing can conflict" |
| **replace main** | Take a fork wholesale and discard what main did since: the fork becomes main and the old main is kept under another name. It restarts main. Always a separate, explicit action. | `silo replace-main`, Replace main, the replace_main tool |
| **undo** | Take a merge or a replace back. | `silo undo`, Undo |
| **decline** | Turn a fork's merge request down: the request closes and the fork is dropped. Denying an agent's approval is a different action and keeps the fork. | `silo requests decline`, Decline |
| **approval** | A request from a computer's agent to do something that widens what it has. Silo's app shows it; you allow or deny it. The agent cannot answer it. | Pending, `silo approvals`, `approval_required` |
| **recipe** | The text description of a computer: system, packages, setup, services, ports. Silo stores it and a computer remembers the one it was built from. | `silo recipe`, [recipes](SiloUI-RECIPES.md) |
| **Silo's tools** | The `silo` program and its MCP server inside a computer, which let its agent use the same commands you have, within what the engine allows. | `silo tools`, [Silo's tools in a computer](SiloUI-IN-GUEST-CLIENT.md) |
| **network policy** | What a computer may reach: open, an allowlist of hosts, or closed. | `silo network policy` |
| **owner** | You, at this device. A computer's agent is a **computer** caller and another device is a **device** caller; the engine tells them apart by how the call arrived. | [What an agent may do](SiloUI-POLICY.md) |

**VM** remains acceptable in technical documentation where the virtualization
itself matters (KVM, guest images, kernels). Code identifiers use `computer`.

**host** and **guest** remain only as the virtualization pair: the device side
and the computer side of one boundary (`hostPort`, `host_push`, guest scripts,
`workspaceHostBytes`). Any other use of host means device and uses `device`.

Silo has no SSH-only computer entry. The earlier unreachable `kind: "ssh"`
configuration was removed with this vocabulary; saved entries of that kind are
dropped by the migration.

## Names that keep older wording

These identifiers live outside Silo's source or in data that existing
installations and other programs read. They do not change.

| Identifier | Reason |
| --- | --- |
| Production names in `src-tauri/src/channel.rs`: application data directories, `~/.silo`, Keychain services and accounts, `silo-remote`, SSH alias prefixes, key comments, the VS Code profile | Production names never change. See [build channels](SiloUI-BUILD-CHANNELS.md). |
| `~/.silo/desktop-remote/` and the remote target id `silo-remote:<deviceId>:<computerId>` | Production path; the target id is an opaque key already stored in settings and editor URIs. |
| MicroSandbox CLI, API and JSON terms: `sandbox`, `--sandbox`, `ListedSandbox`, `InspectedSandbox`, `msbhome/sandboxes/<name>/`, snapshot names | MicroSandbox's own vocabulary. Only the adapter that calls `msb` uses it. |
| MicroSandbox labels `silo.managed`, `silo.machine-id`, `silo.workspace-storage-gib`, `silo.runtime-storage-gib`, `silo.github-protocol`, `silo.restore-attempt`, and the patched flag `--expected-machine-id` | Stored in every existing computer's MicroSandbox database and matched by the vendored patches. |
| `<folder>.code-workspace` and the fallback `workspace.code-workspace` in the editor folder of the private state folder | VS Code's own workspace file; its settings belong to the file name existing installations already use. |
| The `/workspace` mount and `owned-volumes/workspace_<hash>` | Guest path and MicroSandbox layout derived from it. |
| `<name>-<hash>.localhost` published-site origins | Changing an origin discards browser state for published sites. |
| The `silo.runtime=container` label and `"runtime": "container"` in a computer's saved configuration | Stored in every container computer's record and written into its export file (format 6), where an older Silo reads it as a newer format. |

## Words that went

Fold, keep and promote are gone from what people and agents read; so are "divergent" (the
reason a merge needs your choices is `choices-needed`) and keeping a fork without asking when its
tests pass (the tests run inside the fork, so the fork can fake them). `silo fold` is a hidden alias
that says what to type instead.

The wire names `changes.keep`, `changes.promote` and `changes.requests.keep`, the journal files,
the `before-keep` checkpoint kind and the `.silo-backup` format keep their spelling. Only the words
a person or an agent reads were renamed.

## Saved data

The first launch after this change runs a one-time migration that rewrites
saved data to these names before anything else reads it. Normal code reads only
the new names. The renamed keys and files are listed in the documentation of the
[migration module](../app/SiloUI/src-tauri/engine/src/runtime_migration/vocabulary.rs).

Two Silo devices must run the same remote protocol version. The protocol version
changed with this vocabulary, so an updated device refuses an older one and
names the device that needs the update.

Exports are written in the oldest format that holds what they carry: format 4, 5
when a computer's guest image or a fork's record travels by reference, and format
6 when a container computer is in the file. Silo reads formats 4 to 6 and refuses
any other, as it already did for earlier format changes; an older Silo refuses a
format it does not know as a newer one.

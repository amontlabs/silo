# Silo vocabulary

Silo gives agents computers. This page fixes the words the app, its code, its
saved data and its documentation use, and records the identifiers that keep an
older name on purpose.

## Terms

| Term | Meaning | Examples |
| --- | --- | --- |
| **computer** | A virtual computer Silo creates and runs for an agent. Its saved settings are its **configuration**. | New computer, Start the computer, `ApplicationComputer`, `ComputerConfiguration`, `computerId` |
| **device** | A physical Mac or Linux machine that runs Silo. The local one is **this device**; elsewhere the UI shows the device's name. | This device, Computers on linux-box, `Device`, `deviceId` |
| **Connections** | The feature that links devices so one Silo manages computers on another. A computer on another device is a **remote computer**. | Connections settings, Connect device, `remote_computer_action` |
| **workspace** | Only the persistent `/workspace` folder and disk inside each computer. | Workspace files, `workspaceStorageGiB`, `read_workspace_storage` |
| **sandboxed** | The isolation property of a computer, never the object itself. | "Each computer is sandboxed from your device." |
| **computer use** | Agents controlling a computer's graphical desktop. | Computer use, `computerUseAutoApproval` |

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

## Saved data

The first launch after this change runs a one-time migration that rewrites
saved data to these names before anything else reads it. Normal code reads only
the new names. The renamed keys and files are listed in the documentation of the
[migration module](../app/SiloUI/src-tauri/engine/src/runtime_migration/vocabulary.rs).

Two Silo devices must run the same remote protocol version. The protocol version
changed with this vocabulary, so an updated device refuses an older one and
names the device that needs the update.

Exports use format 4. Silo refuses exports in other formats, as it already did
for earlier format changes.

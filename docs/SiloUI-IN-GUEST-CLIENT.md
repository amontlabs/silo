# The in-guest client

A resident agent (OpenClaw, Hermes) that lives inside a computer, usually `main`, can fork that
computer and ask for review from inside with `silo --in-guest`. It reaches the engine through a
per-computer bridge folder and a token the engine minted for that computer. The engine runs only
a short list of methods for it and refuses the rest, whatever the client sends.

What it does not do: keep, promote, undo, drop, change secrets or GitHub access, run commands
in a computer, or touch a computer that is neither itself nor one of its own forks.

## Choices

Silo's guidance asks for a recorded choice before a new transport, credential broker or proxy
([AGENTS.md](../AGENTS.md)). Silo's own code here is the policy (which methods, which computers);
the transport and the guest binary are existing pieces.

| Need | Choice | Why |
| --- | --- | --- |
| Guest-to-host transport | The runtime's per-computer `/.msb` share (virtiofs `msb_runtime`), which Linux computers already have read-write and the host already reads (heartbeat) and writes (scripts, certificates). The guest writes `req/<id>.json`, the engine answers `resp/<id>.json`. | No new listener, route or kernel device. The guest cannot reach the Mac's loopback or any private address by design ([threat model, boundary 2](THREAT-MODEL.md#2-network-egress-public-addresses-only)), so a TCP endpoint on the Mac would need a network exception. |
| Credential | A token per computer, 32 bytes from the OS random source, only its SHA-256 stored in `<app data>/guest-client/tokens.json` (`0600`). | A bearer token is enough: the folder identifies the computer, the token proves the installer meant this computer to call. |
| Guest client | The same `silo` binary in `--in-guest` mode, built for the guest. | One CLI, one set of flags and output. |

Alternatives, and why not:

- **A TCP or Unix-socket listener reachable from the guest** (the gateway address, or the engine
  socket forwarded in). Needs a patch to MicroSandbox's network policy (the gateway address is
  deliberately not public) or a new port forward, and a macOS computer needs its own gateway change.
  The folder needs neither.
- **virtio-vsock.** Silo configures no vsock routes; MicroSandbox's own control channel is
  virtio-serial (CBOR) and is host-initiated. A new vsock route is a patch to a pinned runtime for
  the same effect.
- **SSH from the guest to the Mac** or a reverse SSH tunnel. Needs the Mac's SSH server open to the
  guest, which is the opposite of the egress rule, and a key the guest could use for a shell.
- **`msb exec` stream opened by the host.** Host-initiated, so the engine would hold a long
  exec per computer and parse its output. Polling a folder is simpler and survives restarts.
- **A third-party gateway (an MCP gateway, a credential broker).** None keeps to "this computer
  and its forks" or knows Silo's fork tree; the allow-list below is the product policy.

Gaps accepted: polling adds up to about 300 ms to each call; Linux computers only, virtual machines and containers (macOS computers
have no host folder, so `guestClient.enable` refuses them); and the `/.msb` share has not had a
security review ([boundary 6](THREAT-MODEL.md#6-the-workspace-disk-and-host-folders)), so the
engine bounds every read it makes there.

## Pieces

| Piece | Where |
| --- | --- |
| Typed methods `guestClient.enable`, `.revoke`, `.status`, and the frame types | `app/SiloUI/api/src/methods/guest_client.rs` |
| Tokens and the bridge folder | `app/SiloUI/src-tauri/engine/src/guest_client/` |
| What each call may do, and who made which computer | `app/SiloUI/src-tauri/engine/src/policy/` |
| The loop that serves the folders, and the three methods | `app/SiloUI/src-tauri/silod/src/guest_bridge.rs`, `methods/guest_client.rs` |
| `silo --in-guest ...` and `silo guest-client ...` | `app/SiloUI/cli/src/guest_client.rs`, `guest_install.rs` |

## Install

On the Mac, with the computer running, from the project folder whose `.silo/silo.yaml` should
apply:

```sh
silo guest-client install main --agent openclaw --binary ~/.silo/guest/silo
```

1. The CLI resolves the settings for the agent (`fork_secrets`) and calls
   `guestClient.enable` with them as the token's policy. The engine has no project folder of its
   own, so the policy is stored with the token and read from there; run `install` again after
   changing the settings (it mints a new token and the old one stops working).
2. The engine creates `sandboxes/<name>/runtime/silo-bridge/{req,resp}` and returns the token once.
3. With `--binary`, the binary goes first (below); a failed upload stops the install before any token exists.
   The CLI then writes `/etc/silo/guest-client.json` as root in the computer (`0640`, group `silo`, or
   `0600` when there is no such group; `--group` names another). If writing the file fails the
   token is revoked. The binary is staged under a fresh name on the host and uploaded to `/workspace` like any file; root then refuses a link or a non-regular file, copies the bytes into a `0700` folder under `/root`, checks that copy's sha256 against the hash the host computed of the local binary, and installs it at `/usr/local/bin/silo` from there. Root changes nothing under `/workspace`; the account removes the uploaded file.

`silo guest-client status main` and `silo guest-client revoke main` read and remove the token.

The guest binary is the `silo` crate built for the guest's architecture as a static musl binary.
On an Apple silicon Mac, `app/SiloUI/scripts/build-guest-silo.sh` builds it with Zig as the C
compiler and linker (needs `rustup target add aarch64-unknown-linux-musl` and `zig`) and prints its
path. It is not built by Silo's release scripts yet; give `--binary` any build you made. Without `--binary` the computer must already have a `silo`.

## What the client runs

`silo --in-guest <command>` (or `SILO_IN_GUEST=1`):

| Command | Engine method(s) it calls |
| --- | --- |
| `list`, `tree`, `status` | `computers.tree` |
| `fork <this computer> [--name N] [--secret S]` | `checkpoints.fork` on itself, then `computers.start` on the new fork |
| `diff <fork>` | `changes.diff` |
| `fold <fork> --request` | `changes.diff`, `changes.request` |
| `requests`, `requests show <id>` | `changes.requests.list`, `changes.requests.show` |
| `steps <computer>` | `steps.list` |

Everything else the CLI offers answers "not available from inside a computer" before anything is sent.

## What the engine allows

Each call from the folder runs as the computer's own call (`Caller::Computer`) through the
engine's one choke point, `methods::dispatch`, where the policy decides it as it would for any
other way a computer reaches Silo. [The policy](SiloUI-POLICY.md) lists what an agent inside a
computer may do freely, what asks the owner (the call is answered `approval_required`), and what
is never allowed (`permission_denied`). Every refused or held call, and every call that can change
something, is written to the audit log with the computer's id.

The bridge adds its own limit before the call: at most 16 jobs a computer has not acknowledged.
There is no cap on how many computers it makes or runs; the engine refuses a start, a fork or a
checkpoint only while the device is short of memory or disk (`device.resources` shows how much is
left, and the policy page has the floors). A job over half an hour old that the client never acknowledged is
acknowledged for it once it has finished.

A computer made from inside is the maker's once it exists: a new Linux computer (whose id the
engine chooses) once its job finished without error, a new macOS computer as soon as the call
answers with it, and a fork once its job finished without error and a computer of that name exists
with the source as its parent. A fork that failed, or never got a job within two minutes, is dropped. The
record is the engine's (`<app data>/policy/computers.json`), kept across restarts and new tokens;
a token minted by an earlier version hands the forks and jobs it recorded to it the first time the
bridge serves the computer.

The token's `fork_secrets` no longer limits a fork: the policy lets a fork have any secret its
maker has, and asks the owner for anything more.

## Token lifecycle

- **Mint**: `guestClient.enable` for a Linux computer; 32 bytes from the operating system's random
  source (`getentropy`), shown as 64 hex digits after `silo_gc_`. It replaces any earlier token;
  the computers the computer made stay its own.
- **Hash only**: the engine stores `sha256(token)`; `guestClient.status` never returns the token.
  It does show the `fork_secrets` the token was minted with, and `silo guest-client status`
  warns when the settings now say something else.
- **Revoke**: `guestClient.revoke`, or any promote, undo or delete that involves the computer
  (`changes.promote` revokes the fork's and its target's tokens; `changes.undo` revokes both
  computers of the promote it takes back, read from the promote journal). A revoke that fails
  fails the promote or undo before anything changes. The agent in the old main loses access on
  purpose; run `install` again for the computer that should have it.
- **Delete**: a token whose computer is no longer listed is removed the next time the bridge looks
  at the computers, so it ends after the delete succeeded, not before.
- **Forks**: a fork's own bridge folder is checked against the fork's id, and a token verifies only
  for the computer it was minted for. A copy of the token file in a fork is therefore refused, and
  every fork, made from inside or on the host, has the file scrubbed when it first starts.

## Folder handling and modes

The guest controls everything under the runtime folder, so the engine opens `runtime`, then
`silo-bridge`, `req` and `resp`, each with `openat(O_NOFOLLOW | O_DIRECTORY)` through the
[`rustix`](https://crates.io/crates/rustix) directory-descriptor calls (chosen over `cap-std`
because the workspace already builds `rustix`, and it needs only `openat`, `unlinkat`, `renameat`
and `statat`), afresh in every pass. A guest that replaces one of them with a link makes the
bridge stop serving that computer; it does not reach outside the folder. Each request file is
opened without following links and checked with `fstat` (regular, at most 256 KiB). In `req/` only
`<32 hex>.json` names are served; a `<id>.json.tmp` still being written is left for ten seconds and
then removed, and anything else is removed. No more than 256 entries of `req/` or `resp/` are looked
at in a pass, and every one counts: more than that in `resp/` counts as full, and in `req/` the rest
waits for the next pass. In `resp/` the sweep removes every answer older than ten minutes and
anything that is not `<id>.json` or `<id>.tmp`. At most 64 answers may wait unread, after which new
requests are removed without an answer. A request with a bad token or one that cannot be read gets
no answer at all, so a guest without a valid token cannot fill `resp/`; the client then reports that
Silo did not answer.

Modes and owners: the runtime folder itself is `0700` on the host and the guest sees it as the host
user's, so a guest user other than root could not reach `silo-bridge`. The engine sets the runtime's
stat override (`user.msb.override_stat`, 20 bytes: version 1, uid, gid, mode, rdev, as in
MicroSandbox's `passthroughfs/stat_override.rs`) on the runtime folder to root, `0711`, so any guest user
can search it but not list it, and on `silo-bridge` (`0755`), `req/` (`1733`) and `resp/` (`0711`) so
the guest sees them owned by root. Every other entry in the runtime folder (`root-disk.json`, `rootfs`, scripts,
certificates) is shown as root's too, `0700` for folders and `0600` for files, so the search permission reaches nothing
but the bridge. MicroSandbox writes `root-disk.json` with the host umask (`0666`) and replaces it
at start, restore and rename, so the bridge worker applies the overrides again on every pass (about
every 300 ms), and a guest user could in principle touch a file during the moments before that.
Making the runtime create it `0600` is an upstream request
([draft](v1/upstream/msb-virtiofs-unlinked-checkpoint.md)); a separate share for the bridge would
need a mount a running computer cannot get without a restart, so it was not chosen. The override changes what the guest is shown, not what the host file allows, and it is
set only on the runtime folder, its entries and the bridge folders; nothing on the host follows a path the guest names.
Host files otherwise show the host user's identity, so an answer would not be readable to a non-root
agent. The engine therefore reads the override the guest wrote on the request file (the agent user's
uid and gid) and writes the answer with an override naming that user, mode `0600`. The client writes
requests `0600`. A guest user other than the requester cannot read the answer, and `1733` keeps users
from removing each other's requests. If the host file system keeps no extended attributes, the
setting is skipped and answers show the runtime's default owner (root's), which is the root-only case.
Checked in a running Linux guest on MicroSandbox 0.7.6: `/.msb` shows as `drwx--x--x root root`, and the
`silo` user reads its answers.

## Container computers

A container computer has the in-guest client too, with the same token, allow-list and folder
parsing. Only the share differs, because a container sees real file owners and modes where a virtual
machine's share is virtualized:

- **The share.** The computer's runtime folder (`sandboxes/<name>/runtime`) is bound read-write at
  `/.msb` (no exec, no devices), so the client finds `/.msb/silo-bridge` as in a virtual machine.
  The Landlock ruleset the init shim applies lets the account write there.
- **Owners.** No stat override is written or read. The container's id mapping makes the working
  account (`silo`, 1001) the engine's own user ([threat model](THREAT-MODEL.md#container-computers)),
  so the agent's `0600` request files are owned by the engine on disk and the engine's `0600` answers
  are readable by the agent. The engine computes a request's owner from the file's real owner
  through that mapping (`containers::idmap::KeepId`, used by `guest_client::Spool` when the
  bridge folder belongs to a container; the worker names the kind, the spool does not look for
  it). A request written by any other id (another guest user, root) has no owner, and the engine
  usually cannot read it at all.
- **Root is a difference from a virtual machine.** A virtual machine's answer is made readable to
  the user that asked, root included. A container's answer to a request the engine could read from
  root has no owner to give it to, so it stays the account's `0600` file, and a process that is
  root in the container without `CAP_DAC_OVERRIDE` cannot read it. The root commands Silo starts
  (`silo exec --user root`, account setup) have that capability and read it; a root process
  without it (a service that dropped it, a shell opened another way) gets "permission denied" on
  the answer. Giving the file to the container's root needs a helper in the holder's namespace
  (a file cannot be handed to a sub-id otherwise), and a looser mode would show the answer to
  every guest user that learns the request id, so neither is done: the client is meant for the
  working account.
- **Sealing.** `sandboxes/<name>` is `0700` on the host, the runtime folder `0711`, and
  `silo-bridge`, `req/` and `resp/` get `0755`, `1733` and `0711`, restored on every pass of the
  worker; the other guest users can search the runtime folder but not list it. The agent owns the
  folders, which a virtual machine's guest does not, so it can change their modes; the bridge's
  checks do not depend on them.
- **A container made by an earlier build** (before the id mapping) is refused at start with a
  message to recreate it.

The live check installs the in-guest `silo` and runs a read call (`silo --in-guest list`) as the
`silo` account.

## Checkpoints of a running computer

The engine removes every request and answer it has served. A running computer's full checkpoint
serializes the share's entries the guest has looked up, and MicroSandbox refuses it ("control operation
rejected by peer") when the host removed one of them, so a fork or a promote of a running computer
failed after its first bridge call, until the computer restarted. Before a full checkpoint of a
running computer that has a bridge folder, the engine runs `sync; echo 2 > /proc/sys/vm/drop_caches`
in it as root, which makes the guest forget its cached entries; the call is best effort, and a
checkpoint that still fails reports its own error. The same runs before the full capture a Restore
makes of a running computer, before it pauses. While either capture runs the bridge worker neither takes
requests nor answers (a per-computer hold), so no file is removed after the guest dropped its entries.
A guest process that holds a removed file open pins it; the capture then fails and the message says to
restart the computer. Disk-only checkpoints of a stopped computer are not affected. The upstream issue is
drafted in [msb-virtiofs-unlinked-checkpoint.md](v1/upstream/msb-virtiofs-unlinked-checkpoint.md).

## Open items

- Auto-keep for requests made from inside runs host-side only, in a later slice.
- Mac computers: no transport yet.
- The VM check is `docs/v1/checks/phase5_inguest_check.sh`, with its output in
  `docs/v1/evidence/phase-5/inguest-check.txt`.

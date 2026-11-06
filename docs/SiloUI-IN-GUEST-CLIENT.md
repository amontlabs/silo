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
| Credential | A 256-bit random token per computer, only its SHA-256 stored in `<app data>/guest-client/tokens.json` (`0600`). | A bearer token is enough: the folder identifies the computer, the token proves the installer meant this computer to call. |
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

Gaps accepted: polling adds up to about 300 ms to each call; Linux computers only (macOS computers
have no host folder, so `guestClient.enable` refuses them); and the `/.msb` share has not had a
security review ([boundary 6](THREAT-MODEL.md#6-the-workspace-disk-and-host-folders)), so the
engine bounds every read it makes there.

## Pieces

| Piece | Where |
| --- | --- |
| Typed methods `guestClient.enable`, `.revoke`, `.status`, and the frame types | `app/SiloUI/api/src/methods/guest_client.rs` |
| Tokens, scope checks, bridge folder | `app/SiloUI/src-tauri/engine/src/guest_client/` |
| The loop that serves the folders, and the three methods | `app/SiloUI/src-tauri/silod/src/guest_bridge.rs`, `methods/guest_client.rs` |
| `silo --in-guest ...` and `silo guest-client ...` | `app/SiloUI/cli/src/guest_client.rs`, `guest_install.rs` |

## Install

On the Mac, with the computer running, from the project folder whose `.silo/silo.yaml` should
apply:

```sh
silo guest-client install main --agent openclaw --binary ~/.silo/guest/silo
```

1. The CLI resolves the settings for the agent (`fold`, `fork_secrets`) and calls
   `guestClient.enable` with them as the token's policy. The engine has no project folder of its
   own, so the policy is stored with the token and read from there; run `install` again after
   changing the settings (it mints a new token and the old one stops working).
2. The engine creates `sandboxes/<name>/runtime/silo-bridge/{req,resp}` and returns the token once.
3. The CLI writes `/etc/silo/guest-client.json` as root in the computer (`0640`, group `silo`, or
   `0600` when there is no such group; `--group` names another) and, with `--binary`, installs the
   binary at `/usr/local/bin/silo`. If writing the file fails the token is revoked.

`silo guest-client status main` and `silo guest-client revoke main` read and remove the token.

The guest binary is the `silo` crate built for the guest's architecture, for example
`cargo build -p silo-cli --release --target aarch64-unknown-linux-musl` from `app/SiloUI` with the
musl target and a linker installed. It is not built by Silo's release scripts yet; give `--binary`
any build you made. Without `--binary` the computer must already have a `silo`.

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

`silo_engine::guest_client::ALLOWED` is the list; any other method is refused with
`permission_denied`, including every keep, promote, undo, drop, secret, GitHub, exec and file
method. Per call:

| Method | Allowed when |
| --- | --- |
| `computers.list`, `computers.tree`, `changes.requests.list` | always; the result is cut to this computer and its forks |
| `checkpoints.fork` | `computerId` is the caller. `secrets` may name only what the agent's `fork_secrets` lists. The token file is added to `quiet.scrub`. |
| `checkpoints.create` | `computerId` is the caller |
| `computers.start`, `computers.stop`, `computers.credentials`, `changes.capture`, `changes.baseline`, `changes.diff` | the computer is a fork of the caller (any depth), never the caller itself |
| `steps.list` | the caller or a fork of it |
| `changes.request` | `forkId` is a fork of the caller and `targetId` is absent or the caller |
| `changes.requests.show` | the request is about a fork of the caller |
| `operations.job.get`, `operations.job.acknowledge` | the job was started by this computer's client since the engine started |

A `deviceId` other than the local device is refused. An agent with `fold: ask` therefore forks and
requests, and never keeps; `fold: auto-code` and `auto_keep` are not applied to calls from inside
(a keep stays a host decision), so a request from inside waits for `silo requests keep` or the app.

## Token lifecycle

- **Mint**: `guestClient.enable` for a running or stopped Linux computer; replaces any earlier token.
- **Hash only**: the engine stores `sha256(token)`; `guestClient.status` never returns the token.
- **Revoke**: `guestClient.revoke`, or any promote or undo that involves the computer (`changes.promote`
  revokes the fork's and its target's tokens, `changes.undo` the computer's). The agent in the old
  main loses access on purpose; run `install` again for the computer that should have it.
- **Forks**: a fork's own bridge folder is checked against the fork's id, and a token verifies only
  for the computer it was minted for. A copy of the token file in a fork is therefore refused, and
  forks made from inside also have the file scrubbed.
- A deleted computer's token stays in the file until revoked but matches nothing.

## Open items

- Auto-keep for requests made from inside runs host-side only, in a later slice.
- Mac computers: no transport yet.
- The runtime path of the `/.msb` share on the host (`sandboxes/<name>/runtime`) and the mode the
  guest sees on `req/` and `resp/` need a VM check; see the steps in `docs/v1/BUILD_LOG.md`.

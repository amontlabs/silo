# The MVP API contract

## The choice

An agent's part, 28 November 2026. The Zig engine, the Swift Mac app and the `silo` command all need
to stay in step as each MVP feature is built, without duplicating logic in Swift or rewriting the
wire protocol after the app and command line exist.

The options:

- Settle one versioned Zig API contract up front (recommended). Define every operation, read,
  numbered event, error code and export format once in Zig (`v = 1`), and derive the host Unix
  socket protocol, the C interface for the Swift Mac app and the `silo` command from it. It costs
  writing and reviewing the contract before feature code starts.
- Add calls piecemeal as each feature is built. Each feature appends ad-hoc messages and C
  functions. It costs breaking the Swift client and JSON output between features, and risks putting
  receipt assembly or export formatting inside Swift.

Recommended: settle one versioned Zig API contract up front. The Swift app stays views-only, `silo`
and the app always agree byte for byte on receipts and exports, and every feature builds against a
known contract.

## Transport and C interface

- **Host Unix socket (`run/engine.sock`):** Only local processes running as your macOS user may
  connect (`getpeereid` checked before reading a byte). Every frame carries a 64-byte header
  (`magic` `SILO`, `version` `u16`, `kind` `u16`, `body_size` `u32` capped at 64 KiB, `body_crc`
  `u32` CRC-32C, `session` `u64`, `number` `u64`, 32 reserved zero bytes) and a body. Any malformed
  frame closes the connection before reaching the core.
- **No computer socket in MVP:** Computers cannot reach `run/engine.sock`. The `computer` caller
  scope exists in the protocol types so scoped tools inside computers can be added after MVP
  without changing the wire layout.
- **C interface (`libsilo_client` / `silo_client.h`):**
  - `silo_client_init` and `silo_client_deinit` exist from version 1.
  - Handles are opaque numeric ids checked on every call (`invalid_handle`, `handle_closed`).
  - Owned buffers are freed through `silo_buf_free` on the Zig side; results returned to Swift are
    owned copies.
  - Every function returns a status struct `{ code, message }` rather than a global error slot.
  - Swift stays reactive with zero logic by calling `silo_engine_snapshot` (full state at
    `history_id` and `event_seq = N`) and then `silo_engine_follow` from `(history_id, N)`. If `N`
    left the engine's ring or `history_id` changed, the stream delivers `resync` and Swift takes a
    fresh snapshot.
- **Command line (`silo <group> <action> [--json]`):** Links the same Zig client. Default output is
  plain text for a person; `--json` prints one versioned JSON object per line (`"v":1`). Exit codes
  are `0` done, `1` refused with named error, `2` wrong arguments, `3` engine unreachable.

## Operations

Names follow `one-name-per-concept` and `mechanism-not-use`: the same group and action name is used
in Zig, in the C interface (`silo_<group>_<action>`), in the Swift app and in
`silo <group> <action>`, named by mechanism rather than by one use.

| Group | Action | Kind | Inputs -> Output | Main refusals |
|---|---|---|---|---|
| `engine` | `status` | read | -> `{ version, history_id, event_seq, recorder_acked_seq, recorder_state, computers_count }` | `engine_unreachable` |
| | `snapshot` | read | -> full state at `{ history_id, event_seq }` | `engine_unreachable` |
| | `follow` | stream | `{ history_id, after_seq }` -> numbered `Event` stream or `resync` | `version_unsupported` |
| `computers` | `setup` | granting | `{ name, network_mode }` -> `{ computer_id, op_id }` | `already_set_up`, `disk_too_small` |
| | `list`, `get` | read | `[computer_id]` -> `Computer` rows (`id`, `name`, `role`, `state`, `network_mode`, `ssh_alias`, `last_checkpoint_id`, `parent_checkpoint_id`) | `unknown_computer` |
| | `start`, `stop`, `pause`, `resume` | protective / control | `{ computer_id }` -> `{ op_id }` | `unknown_computer`, `invalid_state`, `operation_under_way` |
| | `drop` | destructive | `{ computer_id }` -> `{ op_id }` | `unknown_computer`, `cannot_drop_main`, `computer_running` |
| | `ssh_info` | read | `{ computer_id }` -> `{ ssh_alias, user, config_installed, ssh_command }` | `unknown_computer` |
| `network` | `set_mode` | granting (widening) / protective (narrowing) | `{ computer_id, mode: open\|limited\|closed }` -> `{ op_id }` | `unknown_computer`, `invalid_input` |
| | `allow_host`, `revoke_host` | granting / protective | `{ computer_id, host }` -> `{ op_id }` | `unknown_computer`, `invalid_host` |
| | `list_hosts` | read | `{ computer_id }` -> `{ allowed[], seen[], denied[] }` | `unknown_computer` |
| `secrets` | `add` | granting (host-only) | `{ name, value_bytes, allowed_hosts[] }` -> `{ secret_id, name, allowed_hosts[] }` | `duplicate_secret_name`, `empty_hosts`, `invalid_host` |
| | `list` | read | -> `[{ secret_id, name, allowed_hosts[], attached_computers[] }]` | |
| | `attach`, `detach` | granting / protective | `{ secret_id, computer_id, env_var }` -> `{ placeholder }` (on attach) | `unknown_secret`, `unknown_computer`, `invalid_input` |
| | `delete` | destructive | `{ secret_id }` -> `{ op_id }` | `unknown_secret` |
| `checkpoints` | `create` | protective | `{ computer_id, label? }` -> `{ checkpoint_id, op_id }` | `unknown_computer`, `disk_too_small` |
| | `list` | read | `{ computer_id }` -> `[{ checkpoint_id, created_at, trigger, label, parent_id }]` | `unknown_computer` |
| | `restore` | destructive | `{ computer_id, checkpoint_id }` -> `{ op_id }` | `unknown_computer`, `unknown_checkpoint`, `operation_under_way` |
| | `fork` | granting | `{ computer_id, checkpoint_id, name }` -> `{ fork_computer_id, op_id }` | `unknown_computer`, `unknown_checkpoint`, `table_full`, `disk_too_small` |
| | `diff` | read | `{ computer_id, from_checkpoint_id, to_checkpoint_id }` -> `[{ path, kind, size_before, size_after }]` | `unknown_computer`, `unknown_checkpoint` |
| `links` | `list_pending` | read | -> `[{ link_id, computer_id, url, callback_port?, requested_at }]` | |
| | `approve`, `deny` | granting / protective | `{ link_id }` -> `{ op_id, port_id? }` | `unknown_link`, `port_busy`, `expired` |
| `ports` | `list` | read | `[computer_id]` -> `[{ port_id, computer_id, guest_port, host_port, kind: oneshot_callback\|forward, state }]` | `unknown_computer` |
| | `open` | granting | `{ computer_id, guest_port, kind }` -> `{ port_id, host_port, op_id }` | `unknown_computer`, `port_busy`, `not_supported_in_mvp` (for `forward`) |
| | `close` | protective | `{ port_id }` -> `{ op_id }` | `unknown_port` |
| `record` | `query` | read (stream) | `{ computer_id, since?, until?, kinds[]?, is?, limit? }` -> JSONL lines (`v = 1`) | `unknown_computer`, `invalid_input` |
| | `head` | read | `{ computer_id }` -> `{ seq, hash, time }` | `unknown_computer` |
| | `verify` | read | `{ computer_id }` -> `{ ok, checked_lines, first_bad_seq? }` | `unknown_computer` |
| `receipts` | `list` | read | `{ computer_id, since?, until? }` -> `[ReceiptSummary]` | `unknown_computer` |
| | `get` | read | `{ receipt_id }` or `{ computer_id, from_checkpoint_id, to_checkpoint_id }` -> `Receipt` | `unknown_computer`, `unknown_receipt` |
| | `export` | read | `{ receipt_id, format: markdown\|jsonl\|bundle, include_record, include_diffs, dest_path? }` -> owned text or written path | `unknown_receipt`, `invalid_input`, `write_failed` |

## Cross-cutting rules

- **Idempotency (`call-ids`):** Every mutating call carries `(session, number)`. Repeating the same
  `(session, number)` returns the first reply without running the action again; reusing a number
  with different content fails with `content_differs`.
- **Shared refusals:** Any call may return `engine_unreachable`, `version_unsupported`,
  `unauthenticated`, `invalid_input`, `session_evicted`, `number_too_low`, `content_differs`,
  `source_full`, `table_full`, `record_behind`, `outcome_unknown` or `not_allowed`.
- **Links and ports separation (`mechanism-not-use`):** `links` is the computer-to-Mac channel for
  asking your Mac's browser to open a URL (`BROWSER` / `xdg-open`). `ports` is the Mac-to-computer
  channel for exposing a computer's TCP port on your Mac. Approving a link that carries a loopback
  `redirect_uri` atomically opens a `oneshot_callback` port that forwards one response without
  recording the callback query or code and then closes. Persistent `forward` ports wait for
  `ports-reached-from-your-mac` and its isolated preview browser after MVP, keeping the same `ports`
  API shape.
- **Secret handling (`secret-values`):** `secrets.add` sends secret bytes directly over the
  authenticated host socket and zeroes the caller buffer after send. The real value never enters
  `State`, the engine log, any `Event`, the record, a receipt or an export. `secrets.list` returns
  only names, allowed hosts and attached computer ids.
- **Receipt and export format (`receipts.export`):** Built in Zig so the Swift app's Copy and
  Export buttons and `silo receipts export` produce identical output. Every export carries `"v":1`,
  the computer id and checkpoint/time bounds, outside facts (model calls, tool calls read from
  traffic, file changes between checkpoints, hosts reached and bytes sent, any gaps or missing
  sources), and the agent's own summary in a separate block marked `unverified` so any agent you
  hand the export to can tell Silo's facts from the agent's claims.

## polarzero's words, 28 November 2026

polarzero pointed out that `signin` was named after one use rather than the underlying mechanism,
and chose to split it into `links` and `ports` with `links` plus `oneshot_callback` ports in MVP
while leaving persistent `forward` ports for after MVP with isolated browser previews.

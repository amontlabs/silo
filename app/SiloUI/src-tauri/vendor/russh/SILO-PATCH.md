# Silo patch to russh 0.65.1

Vendored from the crates.io 0.65.1 archive (`src/` and `README.md`; Apache-2.0, see the
repository's `LICENSE` and the upstream project at https://github.com/Eugeny/russh). Its
`Cargo.toml` is the published one without the examples, tests, benches and dev-dependencies.
This is a local patch, not a claim of upstream acceptance; remove it when russh lets a server
hold back a channel's window.

## Why

A russh server grants the client more window (`CHANNEL_WINDOW_ADJUST`) as soon as the data
arrives, before it calls `Handler::data`, and neither `Handler::adjust_window` (it only picks
the next target, after the packet is queued) nor `Session` can delay or send one. The window
therefore bounds what is in flight, not what a slow command has yet to take. The Silo SSH server
(`engine/src/containers/ssh.rs`) cannot wait inside `data()` (the session loop also takes the
client's window updates), so without this patch it either grew a queue without bound or ended a
command that read slowly, which `sshd` does not do.

## What changed

- `server::Handler::manual_window(channel) -> bool` (default `false`, so other servers are
  unchanged): when `true`, received bytes count against the channel's window and no
  `WINDOW_ADJUST` is sent for them (`src/server/encrypted.rs`, `Encrypted::consume_window`).
- `server::Session::window_adjust(channel, bytes)` and `server::Handle::window_adjust(channel,
  bytes)` send the grant (`Encrypted::grant_window`). The handle's form goes through the
  priority queue (`Msg::WindowAdjust`), which is not held back while data waits for the client's
  window.

The upstream unit tests stay in `src/` but are not run: cargo cannot test a non-member crate that
needs dev-dependencies, and they are dropped from the manifest (`scripts/test_ci_coverage.py` exempts
russh for that reason). The patch is covered by the SSH server's own tests, which drive a real russh
client against it (a command that reads slowly is held back by the window and never ended).

## Watching russh by hand

While russh is vendored, Dependabot and RustSec (`cargo audit`, `cargo deny`) no longer see it: the crate is a path
dependency, with no registry version to compare. Until the patch is upstream (draft:
`docs/v1/upstream/russh-manual-window.md`), russh releases and advisories are watched by hand
(https://github.com/Eugeny/russh/releases and the RustSec database), and a new release is vendored again with this
patch on top.

Run the SSH server's tests on Linux through the Silo manifest:

    cargo test --manifest-path app/SiloUI/Cargo.toml -p silo-engine --lib containers::ssh

# russh: let a server hold back a channel's window (`Handler::manual_window`)

## Summary

`russh::server::Session` sends `CHANNEL_WINDOW_ADJUST` itself, before it calls `Handler::data`, whenever the
remaining window drops below `target / 2`. The window therefore bounds the data in flight, not the data a
consumer has yet to take. A server that feeds a slow consumer (a process's standard input, a socket, a disk) has two
choices, both bad: buffer without limit, or wait inside `data()`. Waiting stalls the session loop, which is also what
takes the client's window updates, so a transfer in both directions through the same connection can deadlock (the
consumer's output cannot leave because the loop is not reading the client's window, and the consumer will not read
more until its output leaves).

## Proposal

An opt-in, per channel, with no change for existing servers:

- `Handler::manual_window(&mut self, channel: ChannelId) -> bool`, default `false`. When it returns `true` for a
  channel, received bytes count against that channel's window and russh sends no `WINDOW_ADJUST` for them
  (`Handler::adjust_window` is not consulted).
- `Session::window_adjust(channel, bytes)` and `Handle::window_adjust(channel, bytes)` grant window back, as the
  server's consumer takes the data. The handle's form goes through the priority queue, so it is never held behind
  output that is waiting for the client's window.

A server then keeps its buffer to the window it granted (an in-memory queue of at most one window per channel), and
the client is slowed by the protocol itself, as with OpenSSH.

## What we run

Silo vendors russh 0.65.1 with a patch of about 40 lines (`manual_window` in `server/mod.rs`, the window counting in
`server/encrypted.rs` and `session.rs`, `Msg::WindowAdjust` and the two `window_adjust` calls in
`server/session.rs`). It serves the SSH server of Silo's container computers, which runs commands, terminals, SFTP and
port forwards with their input flow-controlled this way. The patch is in
`app/SiloUI/src-tauri/vendor/russh/` (`SILO-PATCH.md`); we can open it as a pull request against russh.

## Why it matters

Without it a russh server cannot behave as `sshd` does: `sshd` stops reading a channel when its child does not read,
the client's window runs out, and the client waits. Ending the connection or the command (the only bounded option
left to a russh server) breaks `rsync`, `git` and large `scp` at random.

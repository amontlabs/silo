# Remote hosts build log

Plan: [PLAN.md](PLAN.md).

## Phase 1: Connections works (6 Nov 2026)

- **Found:** every Connections call failed on main. The forced SSH command runs
  `~/.local/bin/silo-remote[-dev] --remote-bridge`, the link points at silod, and silod rejected
  the flag (`unsupported silod argument: --remote-bridge`). `remote::run_bridge` had no caller since
  the engine moved from the app into silod, before this repository's history starts.
- **Fixed:**
  - silod handles `--remote-bridge` (`silod/src/lib.rs`, `app::run_remote_bridge`).
  - Started through the link, macOS found no main bundle (`silod has no bundle identifier`).
    silod now replaces itself with the real file in its bundle for `--remote-bridge` and
    `--remote-guest`.
  - About 1 bridge call in 6 failed with `Invalid argument (os error 22)`: macOS refuses to set a
    timeout on a socket whose peer already closed, while the owner's reply was still buffered.
    The bridge now ignores that refusal and reads the reply.
  - A controller that reaches an old bridge names it ("runs a Silo version that cannot accept
    connections") instead of the generic SSH help.
- **Live (Mac, Dev build):** before the fix, `silod --remote-bridge` exited 1 with the error above.
  After: the handshake and `computers.list` answer through the link and through a user-run sshd
  with Silo's exact restricted `authorized_keys` line; a command sent over that key still runs
  only the bridge.

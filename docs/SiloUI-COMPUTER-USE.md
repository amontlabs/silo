# Driving a fork's desktop from the host

Status: implemented in phase 5 (slice 5d, AMO-70). Unit-tested against a fake engine only; the
two VM checks at the end have not run yet. Background and the choice of Cua Driver:
[computer use plan](SiloUI-COMPUTER-USE-PLAN.md).

An agent on the host can start a fork, wait until its desktop is usable, and drive it with
screenshots, clicks and keys, through the `silo` MCP server. Nothing new runs in the engine:
every step is an engine method that already existed.

## Linux image

The Cua Driver is part of the published guest image (`ubuntu-24.04-v5`, the
`guest-ubuntu-24.04-v5` release of `amontlabs/silo`, pinned by
`app/SiloUI/guest-image/image-lock.json`). A Linux computer created from it has
`/usr/local/bin/cua-driver` and the launcher `/usr/local/libexec/silo-cua-driver-mcp`; the
guest helper registers and checks it at every boot. No local build is needed. To test a changed
image, `npm --prefix app/SiloUI run guest:image:build` builds it with Docker (see
[guest images](SiloUI-GUEST-IMAGES.md)); a computer only uses an image that is published and
pinned in `image-lock.json`.

## The driver session

`silo mcp` keeps one driver MCP session per fork it drives (`cli/src/driver.rs`). The session is
a command started with `guest.exec.start` as the working account (Linux:
`/usr/local/libexec/silo-cua-driver-mcp`, which finds the live desktop session; macOS:
`/usr/local/bin/cua-driver mcp`) with its standard input open. JSON-RPC lines go in through
`guest.exec.input` and come back through `guest.exec.get`. When the driver exits, the next
request starts it again. Nothing is installed in the computer for this and no files are left in
it. The session ends when the command does; `silo mcp` does not stop it when it exits (the
driver stops with its fork).

The tools follow the driver contract of the pinned release
(`cli/tests/fixtures/cua-driver-0.38.0-tools.json`, extracted from `contract/manifest.json` of
`cua-driver-rs-v0.38.0`; a test fails when a call stops matching it, so a driver upgrade means
refreshing the fixture and the table in `usetools::driver_call`):

| Tool | Driver call |
| --- | --- |
| `computer_use_screenshot` | `get_desktop_state {max_image_dimension}`; the driver scales the PNG so its longest side is at most `max_side` (default 1280) and maps coordinates taken from it back to the screen |
| `computer_use_click` | `click {x, y, button, count 1 or 2, target: desktop, delivery_mode: foreground}` |
| `computer_use_type` | `type_text {text, target}` |
| `computer_use_key` / `_hotkey` | `press_key {key, modifiers}` / `hotkey {keys}` |
| `computer_use_scroll` | `scroll {x, y, direction, amount, by}` |
| `computer_use_move` / `_drag` | `move_cursor {x, y}` / `drag {from_x, from_y, to_x, to_y, button}` |
| `computer_use_wait` | a pause on the host, up to 30 seconds |
| `computer_use_tools` / `_call` | the driver's own `tools/list`; any driver tool by name |
| `computer_use_ready` | one look: `ready`, what it waits for, computer-use state and approval |
| `fork_gui` | fork, start, wait until ready (below) |

The host tools drive forks. They refuse `main` (like `drop`): fork it with `fork_gui` and drive
the fork. They only reach computers of this device. Who may call them is decided by the MCP
client's own tool prompts; the `ask`/`auto` mode below is something else.

## Fork, start, wait

`silo fork <source> --gui [--computer-use ask|auto] [--gui-timeout SECS]` and the MCP
`fork_gui` fork the computer, set the approval mode, start the fork, and poll `desktop.status`
every two seconds until the desktop session runs and computer use reports `ready`. A Linux
fork whose desktop service is stopped gets `desktop.action start` once. The wait ends with an
error when the desktop session or the driver reports `failed`, or after the timeout (default
300 s); the fork keeps running and `silo status <fork>` shows where it stands. Progress lines
name what it waits for (`desktop starting, computer use installing`). The result is the fork's
usual JSON plus `computerUse` (`state`, `approval`, `appliedApproval`, `driverVersion`,
`reason`, `session`). `--gui` takes a computer, not a folder.

## Approval: `ask` and `auto` for agents inside a fork

`ask` and `auto` are the computer-use approval of the agents that run inside a computer (Claude
Code and Codex there ask before using the driver, or do not). They do not gate the host tools
above. A fork starts with its source's mode. `--computer-use auto` (CLI) or `computer_use:
auto` (`fork_gui`) sets the fork's mode before its first start. It is a choice the user makes for
forks nobody watches:

- only forks take it; `main` keeps what the user chose in the app (`ask` by default), and
  `silo` refuses to change it;
- a person typing `--computer-use auto` has made the choice. An agent needs
  `computer_use_auto: true` in `.silo/silo.yaml`: that applies to MCP clients and to the CLI
  when `--agent` or `SILO_AGENT` names an agent. The file is one an agent can edit, so this is a
  convenience that stops a casual `auto`, not a gate; the real choice is the one the user makes
  by setting it;
- the mode is recorded on the fork (the engine's per-computer computer-use settings) and shown
  by `silo status` as `computer use: ready, approval auto, driver 0.38.0`;
- in `auto` the agent controls the desktop and, through it, anything the working account can
  reach, including the computer's shared folders; the fork's VM is the boundary.

Keeping an `auto` fork into main brings the fork's changed settings files with it, including
the `mcp__cua-driver` rule in `~/.claude/settings.json` and the approval line in
`~/.codex/config.toml`. After a keep, the engine applies the target's own recorded mode again
(`computer_use::reapply_after_keep`, Linux computers; the guest helper rewrites both files).
A macOS target is not re-applied yet: the VM check below looks at its files after a keep.

## Change review

Computer use writes driver recordings, helper receipts and temporary files. The change-capture
ignore list (`engine/src/changes/ignore.rs`) ignores the ones the guest helpers write, so a
fork driven only through its desktop shows no changes from them: `/var/lib/silo-computer-use`,
`/var/lib/silo-desktop/streamer.json`, `/tmp/silo-cu.*` and `/tmp/cua-driver*` (also under
`/private/tmp` on macOS), and on macOS `/Users/*/Library/Application Support/Silo/computer-use-receipt.json`
and the `Recordings` folder next to it; `/var/log` and `*/Library/Logs` were already ignored. The
driver's own folders (`*/.local/share/cua-driver`, `*/.config/cua-driver/recordings`) are guesses.
TODO from VM evidence: list what `silo diff` shows after a session (X session files, xfce and
dconf state) and add narrow entries; the broad `*/.config/xfce4/xfconf` and
`*/.config/dconf/user` are not ignored because they hold settings a person may have changed.

## macOS permissions

The macOS guest script writes the Accessibility and Screen Recording rows for
`/Applications/CuaDriver.app` into the system TCC database (`/Library/Application
Support/com.apple.TCC/TCC.db`) and the Screen Recording consent ledger, with SIP off. The rows
name the app by bundle id and code requirement, not the machine, and live on the data volume,
so a fork's disk clone carries them; offline personalization changes the account, SSH keys and
host keys and does not touch TCC. The receipt (`computer-use-receipt.json`) is kept by a fork,
so its first boot does not run the grant again; only an import clears it and re-applies. No code
change was needed for forks. Verified live on 2026-11-03 for copies of a template; not yet for a
fork of a running macOS computer.

## VM checks to run

Linux (x86_64 host `ssh 10.77.77.2`, or an Apple Silicon Mac with a Linux `main`):

1. `silo fork main --gui --computer-use auto --json` returns within the timeout with
   `computerUse.state` `ready`, `approval` `auto`.
2. MCP: `computer_use_tools` lists the driver's tools and matches the fixture. The screenshot
   comes back as an image no wider than 1280 with a line giving its size. On a screen wider than
   1280 click a spot read off the screenshot and check, with a second screenshot, that it landed
   there (the driver is meant to map scaled coordinates back; if it does not, scale by
   `screen_width / screenshot_width` in `usetools`). Open a terminal, `computer_use_type` a line,
   `computer_use_hotkey` `["ctrl","l"]`, `computer_use_scroll`.
3. Kill the driver process in the fork, then call a tool: the session starts again.
4. `silo diff <fork> --json` after the session shows no change from the driver, helpers or
   screenshots; record what it does show and narrow the ignore list from it.
5. `computer_use_screenshot` with `computer: main` is refused. `silo status main` still says
   `approval ask`. With `computer_use: auto` and no `computer_use_auto` in `.silo/silo.yaml`,
   `fork_gui` is refused.
6. Keep an `auto` fork into a `main` set to `ask` (`silo fold <fork>`), then read `main`'s
   `~/.claude/settings.json` and `~/.codex/config.toml` inside it: no `mcp__cua-driver` rule and
   no `approve` line, and `silo status main` says `approval ask`.

macOS (Apple Silicon host):

1. `silo fork <macos main> --gui --json` returns with `computerUse.state` `ready`; the wait must
   not end at "desktop running, computer use unavailable".
2. `silo exec <fork> -- /usr/local/bin/cua-driver permissions status --json` lists Accessibility
   and Screen Recording as granted without a prompt in the viewer.
3. `computer_use_screenshot`, `_click` and `_type` work (the session runs
   `/usr/local/bin/cua-driver mcp` through `guest.exec.start`).
4. Keep an `auto` macOS fork into its `ask` main and read the Claude Code and Codex files as in
   Linux step 6; if the rule or line is there, the macOS re-apply is still to do.
5. If the permissions are not granted in the fork, run `silo-computer-use.zsh` apply in the
   guest once and record why the clone lost them.

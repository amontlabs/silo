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
request starts it again. Any failure (timeout, lost or oversized output, a write that fails)
closes the session: its standard input is closed and its state cleared, so the next request
starts a new driver. After the driver exited, only requests that change nothing (`tools/list`,
`get_desktop_state`) are repeated on the new one; a click or a key that may have been carried out
is reported, not replayed. `silo mcp` closes every session when it stops, and a session when its
computer is dropped or a promote runs. A driver answer is limited to 256 KiB per line, 16 content
blocks, and PNG, JPEG or WebP images.

A screenshot never travels on the driver's output: `get_desktop_state` is called with
`screenshot_out_file` set to `/tmp/silo-cu.shot-<pid>-<n>.png` in the computer, and the host
reads the file in parts of 500 KB with `guest.exec` (`tail | head`), then removes it. Nothing
else is installed in the computer for this.

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

The host tools drive forks. They refuse any computer that is not a fork, judged from the
engine's tree (a source computer exists and the name is not `main`): `main` and an archived
`main-before-*` are refused like `drop` refuses `main`. Fork `main` with `fork_gui` and drive the
fork. With `device` they drive a fork on a connected device: the driver runs there, each
`guest.exec.*` call goes to that device's engine over Connections (the session polls every
250 ms instead of 50 ms, since each read is an SSH exchange), and the session is keyed by the
device and the computer, so it is closed there when the fork is dropped. A call that names a
device needs it in the agent's `devices:` entry in `.silo/silo.yaml`, like the other MCP changes
on another device; `computer_use_ready` only reads. `fork_gui` and `silo --device X fork --gui`
fork, start and wait on that device the same way. Who may call the tools is decided by the MCP
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

## Container computers

A container computer on a Linux host has computer use as a VM does: the same Cua Driver, the same launcher
(`/usr/local/libexec/silo-cua-driver-mcp`), `silo mcp`, approvals and `fork --gui` (see
[Container computers](SiloUI-DESKTOP.md#container-computers) for how its desktop starts). The setup that runs
after every boot also writes the container's Chrome launchers, which add `--no-sandbox` (the container's
seccomp profile denies the nested user namespace Chrome's sandbox needs). An agent that drives Chrome
drives a browser whose pages are not isolated from the rest of the computer: a page that compromises it
can act with the computer's GitHub access, use its secrets (the gateway swaps a placeholder for the real
value on the hosts a secret is allowed for) and use Silo's tools in the computer (its `silo`). The launchers cover
`google-chrome`, `google-chrome-stable`, `chromium` and `chromium-browser`, each starting the first
installed of Google Chrome and Chromium whatever name was called, and they stay after `silo desktop off`;
any other Chromium-based program an agent starts with `--no-sandbox` (Playwright, Puppeteer, Electron) is
exposed the same way. Approval is the user's guard: `ask` keeps the user in the loop, and `auto` should
not be used on a container that holds anything that matters.

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
  when `--agent` or `SILO_AGENT` names an agent. The setting is project-wide: once set, every
  agent that makes forks may use `auto` for them. The file is one an agent can edit, and Silo does
  not try to tell an agent's shell from a person's, so this is a convenience that stops a casual
  `auto`, not an access control; the real choice is the one the user makes by setting it;
- the mode is recorded on the fork (the engine's per-computer computer-use settings) and shown
  by `silo status` as `computer use: ready, approval auto, driver 0.38.0`;
- in `auto` the agent controls the desktop and, through it, anything the working account can
  reach, including the computer's shared folders; the fork's VM is the boundary.

Keeping an `auto` fork into main brings the fork's changed settings files with it, including
the `mcp__cua-driver` rule in `~/.claude/settings.json` and the approval line in
`~/.codex/config.toml`. After a keep, the engine applies the target's own recorded mode again
(`computer_use::reapply_after_keep`, by name or id; the guest helper rewrites both files and,
when the mode is `ask`, removes the auto lines even for an agent that is not installed). A
macOS target forgets its recorded computer-use version, so the guest update runs again with the
computer's own approval (`reapply_macos_computer_use_approval`). A promote moves main's approval
to the computer taking the main role and puts the fork's own back on undo; the next start applies
it.

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
2. `computer_use_screenshot`, `_click` and `_type` work (the session runs
   `/usr/local/bin/cua-driver mcp` through `guest.exec.start`).
3. Then `silo exec <fork> -- /usr/local/bin/cua-driver permissions status --json` lists
   Accessibility and Screen Recording as granted without a prompt in the viewer. No LaunchAgent
   starts the Cua Driver daemon; the first `cua-driver mcp` session does, so before any session
   the answer is `daemon_running: false` and `unknown`, which is expected.
4. Keep an `auto` macOS fork into its `ask` main and read the Claude Code and Codex files as in
   Linux step 6 (the guest update runs again after the keep). Promote an `auto` fork over an
   `ask` main: `silo status main` says `ask`; undo and the fork says `auto` again.
5. If the permissions are not granted in the fork, run `silo-computer-use.zsh` apply in the
   guest once and record why the clone lost them.

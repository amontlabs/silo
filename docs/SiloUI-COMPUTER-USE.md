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

## The bridge

`silo mcp` drives a computer's desktop with one `guest.exec` as the working account per call
(`cli/src/bridge.rs`). The command is a short Python program that starts the driver's MCP server
the way agents in the computer do (Linux: `/usr/local/libexec/silo-cua-driver-mcp`, which finds
the live desktop session; macOS: `/usr/local/bin/cua-driver mcp`), sends one `tools/call` or
`tools/list`, and prints the result. A result larger than 600 KB (a screenshot) is left in
`/tmp/silo-cu.bridge-<pid>`, read back in 500 KB parts through further `guest.exec` calls, and
removed. The computer needs `python3`.

Tools of `silo mcp`:

| Tool | What it does |
| --- | --- |
| `fork_gui` | fork (default `main`), start the fork, wait until ready; returns the name and computer-use status |
| `computer_use_ready` | one look: `ready`, what it waits for, computer-use state and approval |
| `computer_use_screenshot` | a PNG of the desktop, scaled so the longest side is at most `max_side` (default 1280) |
| `computer_use_click` `_type` `_key` `_scroll` `_move` `_drag` | actions; coordinates are pixels of the latest screenshot and are mapped back to the screen |
| `computer_use_wait` | a pause on the host, up to 30 seconds |
| `computer_use_tools` | the driver's own tool list (`tools/list`) |
| `computer_use_call` | any driver tool by name with its own arguments |

The driver tool names behind the actions (`screenshot`, `click`, `double_click`, `type_text`,
`press_key`, `scroll`, `move_cursor`, `drag`) and their argument names are in one table,
`usetools::driver_call`. They were written from Cua Driver's documentation, not checked against
the pinned binary: the VM check must run `computer_use_tools` first and fix the table if a name
differs. `computer_use_call` works whatever the table says.

## Fork, start, wait

`silo fork <source> --gui [--computer-use ask|auto] [--gui-timeout SECS]` and the MCP
`fork_gui` fork the computer, set the approval mode, start the fork, and poll `desktop.status`
every two seconds until the desktop session runs and computer use reports `ready`. A Linux
fork whose desktop service is stopped gets `desktop.action start` once. The wait ends with an
error when the desktop session or the driver reports `failed`, or after the timeout (default
300 s); the fork keeps running and `silo status <fork>` shows where it stands. Progress lines
name what it waits for (`desktop starting, computer use installing`). The result is the fork's
usual JSON plus `computerUse` (`state`, `approval`, `appliedApproval`, `driverVersion`,
`reason`, `session`).

## Approval: `auto` for unattended forks

A fork starts with its source's approval mode. `--computer-use auto` (CLI) or `computer_use:
auto` (MCP) sets the fork's mode to `auto` before its first start, so an agent inside the fork
can drive its desktop without asking. It is a choice the user makes for forks nobody watches:

- only forks take it; `main` always keeps what the user chose in the app (`ask` by default),
  and `silo` refuses to change it;
- a person typing `--computer-use auto` has made the choice; an agent asking through MCP needs
  `computer_use_auto: true` in `.silo/settings.yaml`, otherwise the call is refused;
- the mode is recorded on the fork (the engine's per-computer computer-use settings) and shown
  by `silo status` as `computer use: ready, approval auto, driver 0.38.0`;
- in `auto` the agent controls the desktop and, through it, anything the working account can
  reach, including the computer's shared folders; the fork's VM is the boundary.

## Change review

Computer use writes screenshots, driver caches, helper receipts and desktop session state. The
change-capture ignore list (`engine/src/changes/ignore.rs`) ignores them, so a fork driven only
through its desktop shows no changes: `*/.cache/cua-driver`, `*/.local/share/cua-driver`,
`*/.config/cua-driver/recordings`, `/var/lib/silo-computer-use`, `/var/lib/silo-desktop`,
`/tmp/silo-cu.*` (helper staging and the bridge's result files), `/tmp/cua-driver*`,
`*/.config/xfce4/xfconf`, `*/.config/dconf/user`, and on macOS `*/Library/Application
Support/Silo` and the driver's own folders; `/var/log` and `*/Library/Logs` were already ignored.
The driver paths are inferred from the guest helpers and the driver's conventions, not read from
the binary: the VM check lists what `silo diff` shows after a session and extends the list.

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
2. MCP: `computer_use_tools` lists the driver's tools; compare with the table in
   `usetools::driver_call` and fix names. `computer_use_screenshot` returns an image (scaled when
   the screen is wider than 1280); `computer_use_click`, `_type`, `_key` change the desktop (open
   a terminal and type a line), and a second screenshot shows it.
3. `silo diff <fork> --json` after the session shows no changes from the driver, logs or
   screenshots; extend the ignore list for any path it shows.
4. `silo status main` still says `approval ask` after the forks above. With `computer_use: auto` and no
   `computer_use_auto` in settings, the MCP `fork_gui` refuses.
5. `python3` exists in the guest; the screenshot comes back in parts when it exceeds 600 KB.

macOS (Apple Silicon host):

1. `silo fork <macos main> --gui --json` returns with `computerUse.state` `ready`; the wait must
   not end at "desktop running, computer use unavailable".
2. `/usr/local/bin/cua-driver permissions status --json` in the fork (through `silo exec`) lists
   Accessibility and Screen Recording as granted without a prompt on the viewer.
3. `computer_use_screenshot` returns the desktop; `computer_use_click` and `_type` work. `silo
   exec` needs `/usr/bin/python3`: if the guest has none, the bridge must be ported to the
   driver's own `call` CLI.
4. If the permissions are not granted in the fork, run `silo-computer-use.zsh` apply in the
   guest once and record why the clone lost them.

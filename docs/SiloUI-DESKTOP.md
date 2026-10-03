# Optional Linux desktop

For computers created before guest image v4, Silo can install an Xfce desktop into
the same Ubuntu 24.04 computer used for terminal work. Creation opt-in and later
installation use the same recipe. The desktop is optional there; no removal
operation is provided. Computers from v4 on have the desktop built in; see
[Guest image v4](#guest-image-v4-the-desktop-is-part-of-the-computer).

## Lifecycle

The guest helper saves JSON through a unique temporary file in the destination
directory, flushes and fsyncs the file, replaces the destination, then fsyncs the
directory before reporting success. A file-sync failure preserves the previous
JSON; a directory-sync failure reports an error after publication. This uses
Python's [standard file operations](https://docs.python.org/3/library/os.html#os.fsync)
and the existing Selkies patcher's sequence: Linux [fsync](https://man7.org/linux/man-pages/man2/fsync.2.html)
requires a separate directory sync to persist the renamed entry. No new storage
dependency is required. Fixture tests inject both sync failures and inspect
the data and permissions at each boundary; they do not simulate a power loss.

`Start desktop with computer` defaults on. A managed computer boot starts the installed
desktop when that setting is enabled. Switching it off leaves a running desktop
alone. Switching it on starts the desktop immediately when the computer is running.
Manual mode leaves the desktop stopped after computer boot until explicitly started.
Closing a viewer disconnects only the view. Desktop stop closes graphical
applications, preserving independent terminal jobs; computer stop ends both.
An explicit desktop stop in automatic mode lasts for the current boot.

The guest helper is `/usr/local/bin/silo-desktop`, with `status`, `start`, `stop`,
`restart`, `boot`, and `autostart true|false` operations. Management requires
root. Its `connection` operation returns sensitive guest viewer credentials and
must never be logged or exposed in ordinary UI. Guest metadata lives in
`/var/lib/silo-desktop`; transient process identity is tied to Linux boot ID
and process start time. The helper allows three startup attempts, then leaves a
failed state for explicit recovery. The Selkies backend retries a failed session
start (Xvfb, PulseAudio, Xfce) the same way: up to three attempts, each tearing
down what the previous one started, with 1 s and 2 s of backoff; only then is the
session `failed`, and `silo-desktop start` (or a new boot) starts it again.

`/run` is part of the computer's disk, so a restart, a restore or an imported disk still
carries the last session's runtime files. The first helper call of a new boot
(detected by the boot ID marker) empties `/run/silo-desktop/user`, and every
session start removes a PulseAudio `pid` file and `native` socket whose session is
not alive in this boot. Without this, a new boot whose PulseAudio got the same pid
as the previous boot's (boots are nearly deterministic) made PulseAudio exit with
"Daemon already running", the whole session ended `failed` (nothing retried it) and
computer use reported "The Linux desktop was not running". Reproduced on the first
restart of a built-in computer; intermittent on restart and on import.

## Selkies settings, screen size and recipe 3

`launch_selkies_streamer` in `guest/desktop-service.py` starts Selkies 2.0.0 with
these settings. Syntax is from `src/selkies/settings.py` at tag 2.0.0: a bool is on
for `true` or `1` (case-insensitive) and everything else is off, and a `|locked`
suffix stops the client changing it. `enable_clipboard` is a string
(`true`, `in`, `out`, `false`).

| Setting | Value | Effect |
|---|---|---|
| `--enable-clipboard`, `--enable-binary-clipboard` | `true`, `true` | Text and image clipboard messages are accepted in both directions. Nothing syncs on its own: `--clipboard-seamless=false` is the unlocked server default, and the host also seeds the client setting. |
| `--file-transfers` | `none` | Selkies' own upload and download stay off. |
| `--audio-enabled`, `--audio-bitrate` | `true`, `64000` | Opus playback at 64 kbit/s, small enough to share one TCP tunnel with video on a remote computer. |
| `--microphone-enabled` | `false\|locked` | The server answers microphone data with `MICROPHONE_DISABLED` (`websockets_mode.py`). |
| `--ui-sidebar-show-audio-settings` | `false` | The guest sidebar shows no audio section. |
| `--enable-resize` | `true` | The screen follows the viewer window. |
| `--use-css-scaling` | `true\|locked` | The client sends the window size in logical pixels and stretches the canvas, so a Retina window does not multiply the pixels the guest encodes. |
| `--mode`, `--enable-dual-mode` | `websockets`, `false\|locked` | The client cannot switch to WebRTC. |

**Screen size.** Xvfb 21.1 caps RandR at the size it was started with, so it starts
as `Xvfb :1 -screen 0 4096x4096x24 +extension RANDR -noreset` (the upstream native
command uses 8192x4096; Selkies limits client requests to 4080 pixels).
`-noreset` keeps a resized screen when the last X client disconnects. Right after
Xvfb starts, `set_desktop_start_size` runs `xrandr` as the `silo` account: it
adds a 1440x900 mode (`--newmode` with CVT reduced-blanking timings, then
`--addmode screen`) when Xvfb lists none, and applies it with
`--output screen --mode 1440x900 --fb 1440x900`, the same arguments Selkies'
`display_utils_xrandr.py` uses. If `xrandr` is missing, no output appears or the
size does not take effect, the attempt fails like any session start failure: the
session is torn down and retried up to three times, then reported `failed`, with
the reason in `/var/log/silo-desktop.log`. A computer with no viewer attached
therefore still has a 1440x900 desktop, which is what computer-use agents
expect. The streamer receipt's `resolution` (and the lock's) is the start size,
not the current size; a desktop resized by a viewer is healthy, and the
receipt check never compares against the live screen.

**Recipe 3.** `recipeVersion` in `desktop-streamer-lock.json` is 3. The service
accepts receipts 1, 2 and 3 and reports `updateRequired` for anything older than 3.

**Existing computers.** The host only installs `silo-desktop` when it adds a
desktop or runs `update-streamer`; starting or attaching a desktop never replaces
it. A computer with a recipe 2 receipt therefore keeps its old service and old
Selkies flags (no clipboard, no resize, default audio) until its owner updates it.
After the desktop is stopped, its status reports `updateRequired`, the viewer shows
"Update desktop", and that runs `setup-desktop.sh update-streamer`. This is the
recipe 1 to 2 path: it reinstalls the pinned Selkies package, keeps the viewer
credentials, rewrites the receipt as recipe 3 and installs the new service. It
does not rebuild the computer, touch files or restart a running desktop (it
refuses to run while the session is up). It needs network access for the pinned
package download, even on a v4 image that already contains it. New computers get
recipe 3 from the install or preinstalled-image path. Packages are unchanged, so
no guest image bump is needed; `xrandr` (`x11-xserver-utils`) is not listed in
`desktop-packages.txt`; it is expected through `xfce4-session`'s dependencies and
must be confirmed on a live v4 computer.

## Guest image v4: the desktop is part of the computer

Guest images from v4 on (published and pinned in the image lock; see
[guest images](SiloUI-GUEST-IMAGES.md)) already contain the Xfce packages, Selkies
2.0.0, the accessibility defaults (dconf `toolkit-accessibility=true` and the
`/etc/xdg/autostart/silo-accessibility.desktop` poller) and GNOME Text Editor as
the text default. The image describes itself in
`/usr/local/share/silo/guest-image.json` (`schemaVersion`, `version`,
`capabilities`, `streamerVersion`).

- **Host.** A new computer saved without a desktop setting gets one with
  `Start desktop with computer` on when the bundled image is v4 or later
  (`desktop::default_new_computer_desktops`, applied when the configuration is saved),
  and `desktop.builtIn: true`. Creation then runs the same install action as the
  explicit flow, so no user step is needed. A new built-in computer always starts its
  desktop with the computer, including when duplicated settings requested manual
  startup. `builtIn` is Silo's to decide: a value in a saved
  configuration is ignored (an existing computer keeps what it had, a computer on an older
  image is never built in). Existing computers and computers on older images keep the
  explicit "Add Linux desktop" flow.
- **Guest.** `setup-desktop.sh install` reads the marker and verifies the
  capability, the Selkies version, `/usr/bin/selkies`, every package in
  `src-tauri/guest/desktop-packages.txt` (shared with the image Dockerfile), the
  session commands (`xauth`, `Xvfb`, `xfce4-session`, `dbus-run-session` and
  others), the accessibility helper, autostart entry and dconf database, and that
  the Selkies web client can be patched. If all hold, it runs no apt and downloads nothing: it
  prepares the `silo` account, patches the Selkies web client, creates viewer
  credentials, writes the streamer receipt, `xstartup`, `silo-desktop`, the boot
  hook, `packages.txt` and `installed.json` (`"image":"preinstalled"`), then starts
  the desktop. If the marker is unreadable, disagrees with the pinned streamer or
  anything above is missing or damaged, it prints why and performs the full
  install below. On a v4-marked guest that install restores the complete v4 package
  set (including `gnome-text-editor`, not `mousepad`), reinstalls the pinned
  streamer and rewrites the accessibility defaults; on older guests it keeps the
  original recipe. An explicit `install` rerun revalidates a v4 desktop the same
  way and keeps existing connection credentials; legacy installs are only refreshed.
- **Session.** The Selkies backend starts `dbus-run-session -- startxfce4`, so
  `/etc/xdg/autostart` entries run in the session with `XDG_CURRENT_DESKTOP=XFCE`
  (also kept in `xstartup`, which the recipe test checks along with
  `dbus-run-session`).
- Verified offline (`--network none`, Xvfb) in a container from the locally built
  arm64 v4 image: the session shows `xfce4-session`, `xfwm4`, `xfce4-panel`, Selkies
  and `silo-accessibility` running as `silo`. The container needs `SYS_PTRACE`
  (the helper reads `/proc/PID/exe`); a MicroSandbox VM does not.

## Built-in computer use

A computer created from a v4 or later image (`desktop.builtIn`) has agent computer use
ready with no setup. Implementation: `src-tauri/src/computer_use.rs`,
`guest/silo-computer-use.py`, image recipe in `guest-image/Dockerfile`; the
mounted app is described in [ChatGPT app](SiloUI-CHATGPT-APP.md) and the design
in the [computer use plan](SiloUI-COMPUTER-USE-PLAN.md).

The guest receipt writer applies the final `0644` permissions before flushing
the temporary file, then syncs the parent directory after replacement. It returns
a receipt only after both syncs succeed, following the same
[Linux durability requirement](https://man7.org/linux/man-pages/man2/fsync.2.html)
as desktop preferences. Failure-injection tests inspect the complete receipt
and permissions before publication and reject success after a directory-sync
error. They use temporary paths without running a computer.

- **Image.** The pinned LCU archive (`guest/lcu-lock.json`, SHA-256 verified at
  build) is staged unextracted in `/usr/local/share/silo/lcu/`. LCU itself and
  any OpenAI file are not in the image.
- **Mount.** This device's published ChatGPT folder is mounted read-only at
  `/opt/silo/chatgpt` (see the ChatGPT app doc; restores pass it again).
- **During creation.** "Created" means ready: the first start is only a start.
  The create toast shows one line and a bar. Before it takes the device-wide
  operation gate, creation waits (`creation_inputs.rs`) for the background VM image
  import ("Waiting for the VM image", `preparation::ensure_image`) and, for a
  built-in computer, for ChatGPT for Linux ("Downloading ChatGPT for Linux · 62%", from
  the `chatgpt-app-status` cache). The waits hold no gate, so lifecycle operations
  and Quit are never queued behind them, and Quit ends them. If the download fails,
  the toast shows the reason with **Retry** and **Finish without computer use**;
  the latter creates the computer without the apply and computer use finishes at first
  start as for any computer. After the desktop configuration boot, creation runs the
  apply once more in a deliberate temporary boot with the desktop session up
  ("Setting up the desktop and computer use", approval mode, then the usual stop).
  If that fails the computer is still created and the Created toast says computer use
  finishes at first start; the boot apply retries as always.
- **LCU archive mount.** When Silo holds the verified pinned LCU archive
  (`preparation::lcu_folder()`), new computers also get it read-only at `/opt/silo/lcu`;
  the helper prefers that copy (hash must match the lock), then the staged image
  copy, then the download. Existing computers are unchanged.
- **After every boot** (`prepare_booted`, so start and restore) Silo pushes
  `/usr/local/libexec/silo-computer-use`, `/var/lib/silo-computer-use/pinned.json`
  (the tested app/LCU pair) and runs `silo-computer-use apply --boot --approval
  <mode>` to completion on a host background thread, inside the computer's operation turn
  and within a bound; the boot never waits for it or fails because of it, and a stop,
  delete or Quit cancels it. Pushing
  the helper each time keeps it current with Silo, which the image cannot. The same
  runs when the app becomes ready while the computer runs (after the automatic
  download), so a computer created before the app was published gains computer use
  without a restart.
- **`apply`** is idempotent and does nothing when the receipt matches the pinned
  pair and shows the approval mode applied completely. Otherwise: require the read-only mount and the
  app folder (else `needs-app`); use the staged archive if its hash matches the
  lock, else download the locked URL and verify it; extract it to local disk
  (never the shared folder: its Node symlink dangles there); run
  `scripts/install.sh --user silo --runtime-only --skip-system --offline
  --existing-app <folder> --yes`; run `lcu setup --agent all --allow-missing
  --session direct --yes --approval ask|auto` as `silo` (LCU before 0.8.8 lacks the
  flag: the helper detects that from `lcu setup --help` and uses `--agent auto`); read `lcu status --json`; wait for the
  desktop session (bounded: 300 s after a boot, else 90 s; after a boot a session
  that is `failed` or `stopped` while the desktop starts with the computer is started
  again with `silo-desktop start`, up to three times with 2, 4 and 8 s of backoff,
  before the setup fails with `desktop-session-not-running`) and run
  `lcu-session --user silo -- lcu doctor
  --non-interactive --require-ready` as `silo`. The result is
  `/var/lib/silo-computer-use/receipt.json`; `apply` also reports this run's approval
  outcome (`applied`, `partial` when `lcu setup` configured some agents and failed for
  others, else `failed`) from `lcu setup`'s per-agent lines; agents not installed yet are
  recorded by LCU as pending (the receipt lists `agents` and `pending`) and are not a
  failure; the log is
  `/var/log/silo-computer-use.log`. A reinstall happens only when LCU's recorded
  app path or version differs from the pinned pair.
- **Agents installed later.** Every supported agent is registered automatically; there is
  no action to run. `lcu setup --reconcile` registers a pending agent once its binary
  exists, with the saved approval, and is a quiet no-op otherwise (see the
  [research](research/lcu-agent-preregistration-2026-10-03.md)). The helper runs it at the
  end of every `apply` (every boot and app-ready, also when nothing else changed). While the
  computer runs, the guest has no init system (no systemd, no inotify tools in the image), so
  `apply` starts `silo-computer-use watch` (one instance under a lock) that checks the
  agents' install directories (`~/.local/bin`, `~/.bun/bin`, `~/.cargo/bin`,
  `~/.npm-global/bin`, `/usr/local/bin`) every 5 s while an agent is pending and runs
  `silo-computer-use reconcile` when a pending binary appears. It ends when nothing is
  pending. `/etc/profile.d/silo-computer-use.sh` is a fallback for the `silo` account's login
  shells. Reconcile is skipped while an `apply` holds the helper lock (that run reconciles).
- **Approval.** Per computer in `<storage>/computer-use/<id>.json`: the mode the user chose
  (default `ask`), the last mode applied and the last attempt. The host applies changes
  itself, one at a time per computer, on a background thread (see the
  [approval design](SiloUI-COMPUTER-USE-PLAN.md#approval-design-2026-10-02)); a stopped
  computer picks a changed mode up at its next boot. `ask` removes only LCU's own harness
  entries, `auto` adds them (Claude Code `permissions.allow`, Codex
  `default_tools_approval_mode`); native app permissions and the original runtime's own
  approvals are unchanged. The switch configures the agents' approval prompts and is not
  a security boundary inside the computer: agents there have root. A fork starts with its
  source's mode; an import starts with `ask`.
- **Desktop state.** `read_desktop_state` adds `computerUse` for built-in computers,
  also while stopped (`state: "computer-stopped"` keeps the approval and the last
  versions seen): `state` (`unavailable`, `preparing`,
  `installing`, `ready`, `failed`), `reason`, `compatibility` (`tested`,
  `untested`, `unknown`), `warning`, `approval`, `appliedApproval`, `approvalApply`,
  `approvalApplyReason`, `appVersion`, `runtimeVersion`, `lcuVersion`, `agents`. The running read costs one guest command that also
  returns the helper's `status`, which only reads the receipt. The legacy `lcu*`
  fields stay for computers created before v4.
- **Commands.** `set_computer_use_approval { computer, mode: "ask" | "auto" }`
  stores the mode and, when the computer runs, starts applying it in the background and
  returns at once (`approvalApply: pending`); the `setup-computer-use`
  desktop action reruns `lcu setup` (the panel's "Try again" after a failed setup; it
  works for every v4 computer, whether or not the desktop is reachable). Both route to the owning
  device. An older Silo there answers "Update Silo on that device to use
  computer use."

Legacy computers: `setup-lcu` keeps working for computers created before v4 with the 0.4.0
lock (`guest/lcu-legacy-lock.json`); it is refused for built-in computers.
Its receipt writer uses the same file-sync, replacement and directory-sync
sequence as desktop preferences. Failed file synchronization preserves the
previous receipt, and failed directory synchronization rejects completion.
Tests inject both failures and verify cleanup and retry using temporary paths.

LCU installation on computers created before v4 is separate and unchanged.

## Applications and external tools

Computers use `silo`, with home `/home/silo`, for terminal, SSH, editor and desktop
work, with passwordless sudo for administration. Installing the desktop later
reuses that account and preserves existing workspace files. Older computers move to
it at their next start ([older computers](SiloUI-WORKING-ACCOUNT-MIGRATION.md)).
Adding a desktop never changes accounts or file ownership.

Run graphical programs as the computer's desktop user with `DISPLAY=:1` and
`XAUTHORITY` pointing to `.Xauthority` in that user's home. The session provides
D-Bus and an accessibility bus. Root-owned files retain ordinary Linux access
rules, including on new computers when files were deliberately created with sudo.
Conflicting pre-existing VNC configuration is reported before installation,
rather than overwritten. See [working accounts](SiloUI-WORKING-ACCOUNT.md).

On computers created before v4, adding a desktop does not install agent tools;
[LCU](SiloUI-COMPUTER-USE-PLAN.md) setup remains an explicit action on a running
computer. Built-in desktops use the [automatic computer-use setup](#built-in-computer-use)
after boot; the [boot handler](../app/SiloUI/src-tauri/src/runtime.rs) schedules
it, and [desktop actions](../app/SiloUI/src-tauri/src/desktop.rs) keep legacy
`setup-lcu` separate from built-in `setup-computer-use`.
Silo previously installed [Luda](SiloUI-LUDA.md) (now historical). Existing
desktops that have Luda keep it untouched, and Silo ignores its status.
Silo does not install or authenticate the agents themselves. Tools running in a
remote SSH project must execute inside the guest and target this display;
selecting an SSH project does not redirect a macOS-only plugin. Human and
automated input share the ordinary Linux session without Silo arbitrating control.

The initial recipe includes a terminal, file manager, text editor and fonts.
Users install additional applications, including their preferred browser.

## Recipe and sources

### Historical account decision audit, 2026-09-21

The single-account requirement above supersedes this audit’s compatibility
recommendation. The technical reasons for using a normal account still apply.

The separate desktop account is a compatibility compromise with the existing
root-based terminal/SSH workflow, not a requirement to prevent data corruption.
The original research recommends a normal account for application compatibility;
the implementation plan also requires preserving existing identities, credentials
and ownership. Neither records a same-account corruption reproduction.

Concrete upstream constraints:

- [Chromium's Linux startup code](https://raw.githubusercontent.com/chromium/chromium/main/content/browser/zygote_host/zygote_host_impl_linux.cc)
  exits when running as root without `--no-sandbox`. A root desktop therefore
  requires a browser sandbox bypass; VM isolation does not replace browser
  process isolation inside the guest.
- [VS Code's Linux launcher](https://raw.githubusercontent.com/microsoft/vscode/main/resources/linux/bin/code.sh)
  rejects an ordinary root launch, checks for specific override arguments, and
  instructs users to provide `--no-sandbox` and an alternate user data directory.
- [KasmVNC 1.5.0's launcher](https://raw.githubusercontent.com/kasmtech/KasmVNC/v1.5.0/unix/vncserver)
  uses home-relative `.vnc`, `.kasmpasswd`, and default `.Xauthority` paths.
  Reusing an account requires respecting existing files at those paths. Its
  environment check does not itself demand a separate non-root account.

The installer already runs as root and modifies system packages for both
accounts. A separate session account cannot isolate package conflicts or an
interrupted apt operation. It does avoid writing desktop configuration into
the existing home. Same-account installation does not inherently require
changing workspace ownership or moving existing data. A naive port of this
recipe would overwrite existing `.vnc/kasmvnc.yaml` and `.vnc/xstartup`; those
specific collisions need preflight checks or dedicated paths, not necessarily
a separate UID. Session/display conflicts likewise need explicit handling.

The current split has real costs: desktop processes retain ordinary non-root
access rules for root-owned project files and use a different home for tools,
Git settings and credentials. Passwordless sudo does not automatically make
ordinary desktop file operations privileged. Conversely, unrestricted sudo
means this is not a security boundary against a malicious desktop process.
Desktop shutdown uses process groups, so independent terminal-job survival
does not intrinsically require a second UID.

Engineering recommendation: retain the non-root desktop for existing root-based
computers; do not replace it with an all-root desktop as a simplification. For a
unified workflow, use one normal working account for terminal, SSH and desktop,
with root for administration. Existing computers need an explicit migration of the
working environment, separate from installing desktop packages; do not silently
change ownership or move credentials during desktop installation.

This audit inspected repository code/history and upstream source. It did not
run a root-desktop A/B test or establish browser sandbox support on the pinned
guest runtime. Existing verification below explicitly excludes browser workloads.

- Xfce packages come from Ubuntu 24.04 repositories, using a minimal package set.
- KasmVNC is pinned to 1.5.0, with separate SHA-256-verified Noble packages for
  ARM64 and AMD64. [Release assets](https://github.com/kasmtech/KasmVNC/releases/expanded_assets/v1.5.0).
- Configuration follows the [versioned defaults](https://github.com/kasmtech/KasmVNC/blob/v1.5.0/unix/kasmvnc_defaults.yaml).
- (KasmVNC, superseded by Selkies recipe 3.) The viewer opens with `resize=scale`, selecting client-side local scaling while
  the guest keeps its fixed 1440×900 display (`allow_resize: false`). Window
  resizing must not change the guest screen or pointer coordinate space. See
  [KasmVNC local-scaling configuration](https://github.com/kasmtech/KasmVNC/discussions/249).
  The URL contains no authentication material; native cookies authenticate the
  local proxy.
- Noninteractive authentication follows the [versioned password tool](https://github.com/kasmtech/KasmVNC/blob/v1.5.0/unix/kasmvncpasswd/kasmvncpasswd.c).

Guest HTTP authentication is enabled. Silo's viewer transport owns loopback
forwarding and remote SSH tunneling. Do not manually publish guest port 6901
to an untrusted network. The guest password is generated randomly, stored in
root-only metadata, and passed to KasmVNC through stdin rather than arguments.
The installed package inventory is recorded in `/var/lib/silo-desktop/packages.txt`.
Package licenses remain available through Ubuntu's `/usr/share/doc` inventory;
KasmVNC source and license notices are available in the linked release project.

Installation rejects unsupported OS/architectures, insufficient free disk,
unmanaged conflicting VNC installations and an unrelated pre-existing desktop
account. Downloads are verified before installation. Interrupted package
operations are not transactional; the stage journal supports investigation
and retry. Existing installed desktops are not silently upgraded on boot.

## Verification, 2026-09-18

A disposable ARM64 computer on macOS, using the bundled MicroSandbox engine in an
isolated runtime home, successfully installed Xfce and KasmVNC, rendered the
1440×900 desktop, launched Mousepad, typed into it through independently
installed xdotool, and exposed X.Org display `:1`.
Unauthenticated HTTP returned 401; authenticated HTTP returned 200.
Manual/automatic preference changes and start/stop passed, including an
independent terminal job surviving desktop shutdown. A partial installation
retry succeeded without replacing the desktop account. A second pristine computer
installed the complete corrected recipe without manual fixes. With the patched
bundled runtime, automatic startup ran after computer boot, explicit stop reset on
automatic reboot, and manual mode remained stopped after reboot.

The rebuilt isolated macOS bundle at
`app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Desktop Verification.app`
rendered the live guest desktop through the authenticated native viewer.
Native input displayed ASCII text and `café` in Mousepad. Independent guest
XInput observation confirmed pointer button and keyboard events from KasmVNC.
Closing and reopening the viewer preserved Mousepad in the same desktop session.
The application's guarded Quit exited successfully, and subsequent read-only
runtime checks confirmed both disposable proof computers were stopped.

The CUA typing tool did not emit the requested CJK text, so that attempt does
not establish either working or broken CJK guest input. Full international
input, IME behavior, and clipboard coverage remain unverified.

A warm stop/start comparison with no viewer reported a 126,096 KiB decrease in
guest MemAvailable (about 123 MiB), and 238,819 KiB summed proportional memory
for desktop-user processes (about 233 MiB). These are different measurements,
not interchangeable budgets. They do not establish host memory cost, a cold
baseline, browser workload or active streaming cost.

The guest lifecycle test command is:

```sh
python3 -m unittest discover -s app/SiloUI/scripts -p test_desktop_service.py
```

Ten deterministic tests cover startup preference behavior, stale process
identity, session health reporting, credential-free status and bounded logs
that refuse symlinks. Live evidence is kept under ignored `app/SiloUI/src-tauri/target/verification/desktop/`.
These results do not establish AMD64/Linux or remote-owner compatibility.
The native viewer result applies to the inspected macOS verification bundle,
not an installed distribution or release readiness.

### Final application checks

The final isolated macOS ARM64 bundle passed windowed/fullscreen layout, toolbar
visibility, desktop stop confirmation, stop/start without stopping the computer, and
returning from fullscreen. Native layout uses the measured difference between
WKWebView's native frame and its CSS viewport; local scaling keeps the guest
at 1440×900. Temporary diagnostic logging was removed before the final build.

Commands and results:

- `npm --prefix app/SiloUI run typecheck`: passed.
- `npm --prefix app/SiloUI run lint`: passed.
- `npm --prefix app/SiloUI test`: 95 files, 855 tests passed.
- `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml --quiet`:
  437 passed, 11 opt-in tests ignored. Tests include actual loopback HTTP,
  WebSocket forwarding/disconnect, Unix-socket port reuse, subprocess lifecycle
  cleanup, native geometry, backup persistence and capability isolation.
- `python3 -m unittest discover -s app/SiloUI/scripts -p test_desktop_service.py`:
  10 passed.
- `npm --prefix app/SiloUI run test:release`: 34 passed against disposable
  release fixtures; no release was published.
- `npm --prefix app/SiloUI run desktop:build:debug -- --config
  '{"identifier":"org.silo.desktop-verification","productName":"Silo Desktop Verification"}'`:
  built successfully. This separate application identity used only disposable
  computer data; the user's normal Silo application data was not used.

These results do not establish live Linux/KVM, AMD64, remote-owner, browser
workload or complete IME compatibility. The remote implementation and AMD64
recipe require those environment-specific acceptance runs before claiming
cross-platform release readiness. Installed desktops are not automatically
upgraded by this first recipe.

## Native viewer input investigation, 2026-09-22

The macOS viewer has a concrete clipboard compatibility gap. This investigation
used repository source, pinned upstream source and an isolated WKWebView probe.
It did not launch or inspect a packaged Silo bundle, connect to a guest, read the
host clipboard, or reproduce the reported typing/Enter delays in a live session.
No production behavior changed.

### Confirmed mechanism

1. `desktop_viewer.rs` creates a nonpersistent child webview with the default
   browser user agent and navigates to `/?resize=scale`. It does not explicitly
   disable seamless clipboard. The installed Wry 0.55.1 implementation only
   overrides WKWebView's user agent when the caller supplies one.
2. Silo pins KasmVNC 1.5.0. Its [release submodule metadata](https://api.github.com/repos/kasmtech/KasmVNC/contents/kasmweb?ref=v1.5.0)
   pins the browser client to `475ecfa5356579ef222983c7ce4619a7576a3bce`.
   That client's [browser detection](https://github.com/kasmtech/noVNC/blob/475ecfa5356579ef222983c7ce4619a7576a3bce/core/util/browser.js)
   recognizes Safari by the literal `Safari` token. Its [settings and connection code](https://github.com/kasmtech/noVNC/blob/475ecfa5356579ef222983c7ce4619a7576a3bce/app/ui.js)
   disable seamless clipboard for recognized Safari; both safeguards depend on
   that token.
3. An isolated nonpersistent WKWebView on this host returned
   `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`.
   This fails both Safari safeguards. Executing the exact upstream detection,
   default-setting, connection-safeguard and clipboard-check functions with this
   measured user agent enabled seamless clipboard and called the mocked clipboard
   reader once. A Safari-token control and an explicit seamless-off control each
   produced zero clipboard reads.
4. The pinned client's [input code](https://github.com/kasmtech/noVNC/blob/475ecfa5356579ef222983c7ce4619a7576a3bce/core/rfb.js)
   checks the clipboard on canvas mousedown when its resend flag is set. Window
   focus/blur and canvas focus set that flag. [WebKit documents](https://webkit.org/blog/10855/async-clipboard-api/)
   that programmatic clipboard reads without explicit paste intent or same-origin
   clipboard content show a native macOS Paste context menu.

This establishes a path from ordinary clicks to a native Paste prompt. Kasm's
[earlier upstream fix](https://github.com/kasmtech/noVNC/pull/110) explicitly
disabled seamless clipboard for Safari and Firefox to prevent this experience.
The embedded WKWebView identity escapes that existing protection.

The matching upstream report is [KasmVNC #219](https://github.com/kasmtech/KasmVNC/issues/219):
ordinary left and right clicks open a menu containing only Paste; dismissing it
helps until the pointer leaves and re-enters the viewer. In the linked fix,
the maintainer explicitly identifies clipboard reads during clicks as the trigger.
This is an input correctness defect, not evidence of insufficient computer resources
or a frame-rate tuning problem.

Other upstream macOS input reports have different triggers:
[KasmVNC #341](https://github.com/kasmtech/KasmVNC/issues/341) reports broken mouse
and keyboard input after the macOS screenshot shortcut, on Chrome with KasmVNC
1.3.4; [#236](https://github.com/kasmtech/KasmVNC/issues/236) reports an Alt-key
translation failure on Chrome with KasmVNC 1.2.0. Neither establishes the cause
of this user's delayed ordinary typing/Enter in Silo's WebKit viewer.

### Coverage and next action

The [later published-app test report](SiloUI-LUDA-AGENT-TESTS.md#silo-user-flow-coverage)
already records repeated native Paste prompts and no clean keyboard acceptance
result. Earlier limited ASCII/Unicode success above does not establish reliable
click, focus, clipboard or keyboard behavior across sessions. The frontend viewer
tests mock native attachment and do not exercise guest input.

The selection and delayed text/Enter symptoms still need a live event trace;
this investigation does not attribute them to the clipboard defect. Packaged-app
acceptance must cover click, drag/release, text, Enter, explicit paste and focus
transitions before claiming those symptoms are resolved.

Ignored evidence is in `app/SiloUI/src-tauri/target/verification/desktop-input/`:
`probe.swift`, `webkit-probe.json`, pinned source files, `reproduce.mjs` and
`reproduction.json`. The sandboxed Swift attempt timed out because WebKit services
could not start; the permitted unsandboxed rerun passed. Commands that passed:

```sh
swift -module-cache-path /private/tmp/silo-desktop-input-swift-cache app/SiloUI/src-tauri/target/verification/desktop-input/probe.swift
node app/SiloUI/src-tauri/target/verification/desktop-input/reproduce.mjs
```

### Clipboard and the host bridge (Selkies)

The 2026-09-22 investigation above described the KasmVNC client, which the
viewer no longer runs; the viewer now loads Selkies 2.0.0. KasmVNC's URL
parameters (`resize=scale&clipboard_seamless=false`) mean nothing to Selkies,
which reads only `token`, `offscreen_worker` and `socket_worker` from the URL.
Every other client setting comes from `localStorage` under the key
`<origin and path, with characters outside [a-zA-Z0-9._-] replaced by _>_<name>`
(`getStorageAppName` in the client's `lib/util.js`), with booleans stored as
`"true"` or `"false"`. The viewer URL is now the bare origin, and the
initialization script writes `<prefix>_clipboard_seamless = "false"` before the
client reads its settings. The prefix is computed from `location` at run time
because the proxy port changes on every attach. The seeded setting keeps the
guest page from writing this device's clipboard on its own.

**Host bridge** (`desktop_bridge.rs`, `desktop_viewer_bridge.js`). The guest
serves both the Selkies server and the client JavaScript in the viewer, so the
page is untrusted and the bridge is shaped so that only Silo's own window, menus
and Rust code can start a transfer:

- *Host to page.* Rust calls `Webview::eval` on the unprivileged
  `guest-desktop-shell-<uuid>` child with
  `window.__silo.invoke(<method>, <args>)`. The method and arguments are
  serialized with `serde_json`; no payload is concatenated into script. The
  child keeps no Tauri capabilities. `__silo` is a frozen, non-configurable
  property that dispatches only to a fixed method list.
- *Page to host.* The proxy answers `/__silo/v1/<op>` itself and never forwards
  anything under `/__silo` to the guest. A request needs the viewer's cookie, the
  `POST` method, and a single-use nonce that Rust issued for that viewer and
  operation (`clipboard`, `capabilities`). Nonces expire after the requested
  wait plus 5 seconds, a wrong nonce neither succeeds nor cancels the real one,
  and each operation has a byte cap (24 MiB clipboard, 4 KiB capabilities)
  checked against `Content-Length` before the body is read. Rejections are
  logged with rate limiting. The accepted body goes to the waiting Rust caller.
- *Page helper.* The script wraps `window.selkiesTransport` (the
  WebSocket-mode transport, absent in WebRTC mode; the helper reports that in
  `capabilities` and refuses to send), caches the last `clipboard,`,
  `clipboard_binary,` or chunked `clipboard_start`/`clipboard_data`/
  `clipboard_finish` payload without ever forwarding it unprompted, sends only
  allow-listed Selkies frames (`cw`, `cws`/`cwd`/`cwe`, `cb`, `cbs`/`cbd`/`cbe`,
  `kd`/`ku`, `r,WxH,primary`, `REQUEST_CLIPBOARD`), and posts the same-origin
  `setMute`, `setVolume`, `resetResolutionToWindow` and `pipelineControl`
  messages. Mute and volume are re-applied when Selkies reports a pipeline
  change, because its gain node exists only once audio flows. The script still
  locks the web clipboard APIs and now also makes `getUserMedia` and
  `getDisplayMedia` always reject.
- *Rust API for later phases* (`desktop_viewer::with_bridge(app, label, |bridge| ...)`,
  from a worker thread and never the main thread): `send_guest_text`,
  `send_guest_image`, `press_guest_paste`, `press_guest_copy`,
  `request_guest_clipboard(timeout, press_copy) -> Empty | Text | Image`,
  `set_audio_muted`, `set_audio_volume`, `set_audio_active`,
  `reset_resolution(w, h)`, `reset_resolution_to_window`, `capabilities(timeout)`.

**Native shortcuts** (`viewer_shortcuts.rs`). Triggers never come from in-page key
events. On macOS an `NSEvent` local monitor, scoped to viewer windows by their
`NSWindow`, consumes Command+C and Command+V before WKWebView sees them, and the
Edit menu has matching *Paste into Computer* and *Copy from Computer* items that
are enabled only while a viewer has focus. On Linux a GTK key handler on the
viewer window handles Ctrl+Shift+V and Ctrl+Shift+C (plain Ctrl+C and Ctrl+V stay
guest shortcuts); the viewer window has no native menu bar, so the toolbar
buttons are its menu equivalent. Other windows
keep their normal Copy and Paste. The handlers are live and start the transfers
described under *Clipboard behaviour* below.

### Clipboard behaviour

`viewer_clipboard.rs` holds the orchestration; the shortcuts, the Edit menu
items and the toolbar buttons all call it, and it touches the device clipboard
only for those explicit actions (the connect-time push from Selkies is cached,
never written).

- **Paste into computer** reads the device clipboard on a blocking worker. Text
  wins when both exist (at most 1 MiB, never truncated); otherwise an image is
  sent as PNG (at most 16 MiB encoded, 32 Mpixel). It sets the computer's
  clipboard, then presses Ctrl+V there. An empty device clipboard sends nothing.
- **Copy from computer** presses Ctrl+C in the computer and waits up to 1 s for
  its new selection, falling back to the last one the page saw. Text is limited
  to 1 MiB; images (PNG, JPEG, WebP, BMP, 16 MiB) are decoded and validated by
  `write_image_from_encoded` before replacing the device clipboard. An empty or
  undecodable answer leaves the device clipboard unchanged. HTML flavours are not
  forwarded.
- **Triggers.** macOS: Command+V and Command+C while a viewer is focused, plus
  Edit, *Paste into Computer* and *Copy from Computer*. Linux: Ctrl+Shift+V and
  Ctrl+Shift+C (plain Ctrl+C and Ctrl+V remain guest shortcuts). Both OSes: the
  *Paste into computer* and *Copy from computer* buttons in the viewer toolbar.
- **Feedback.** The toolbar shows "Pasted into / Copied from <computer>" (with
  "image" for images) or the problem: clipboard empty, too large, desktop not
  connected, another transfer running, or the failure text. The button command
  `desktop_viewer_clipboard` (granted to `desktop-shell-*` windows only, bound to
  the window's own computer) returns a typed report; shortcut-started transfers
  send the same report as the `desktop-clipboard` event to the shell window.
- **Older desktops.** A recipe 2 desktop has clipboard transfer disabled on the
  server and cannot report that, so the toolbar (which knows the desktop's
  `updateRequired`) shows "Update the desktop to use the clipboard" instead of
  trying. A shortcut on such a desktop shows the same message.

Live checks outstanding: a Dev build against a recipe 3 computer (text and image
both ways, Command+C and Command+V with the guest focused, no WebKit Paste popup)
and Ctrl+Shift+C/V on Ubuntu 24.04 under Wayland and X11.

Probe items remaining (need a live Dev build): whether `Webview::eval` reaches
the `add_child` webview once its page has loaded on both engines; whether the
macOS local monitor sees Command+V while the guest webview is first responder
(expected, since local monitors run before `sendEvent:` dispatch); whether the
GTK handler precedes WebKitGTK key handling; and whether Selkies applies the
seeded `clipboard_seamless` when the server also pushes a default.

## Device clipboard module

`src-tauri/src/clipboard.rs` is the device-side clipboard used by the viewer
integration ([plan](SiloUI-VIEWER-INTEGRATION-PLAN.md), Phases 1 and 2). It has
no JavaScript surface and adds no Tauri capability.

### Choice

| Option | Result |
|---|---|
| `tauri-plugin-clipboard-manager` | Rejected. It wraps `arboard` but exposes text and RGBA images to the webview through commands and capabilities, and offers no control over Linux selection ownership. |
| `arboard` 3.6.1 directly | Chosen. Dual MIT or Apache-2.0, maintained by 1Password, Send + Sync with no macOS main-thread requirement, text and image support, X11 and Wayland backends. |
| Custom NSPasteboard, X11 and Wayland code | Rejected. The platform protocols are what `arboard` already implements. |

`arboard` is built with `default-features = false` and `image-data` plus
`wayland-data-control`. `image` 0.25 was already locked through Tauri and
`arboard`; Silo enables its `png`, `jpeg`, `webp` and `bmp` features, which adds
the pure-Rust `zune-jpeg` and `image-webp` decoders. The new locked packages are
`arboard`, `wl-clipboard-rs`, `x11rb`, `clipboard-win` and their dependencies;
all are MIT, Apache-2.0 or similar permissive licenses, and the repository
keeps no per-crate license manifest (`THIRD-PARTY-NOTICES.md` covers only the
bundled runtimes).

### Behavior

- `DeviceClipboard` is the interface the bridge depends on (`read_text`,
  `write_text`, `read_image`, `write_image_from_encoded`); `ClipboardService`
  applies the policy over a `RawClipboard`, and `system()` returns the process-wide
  `arboard` instance. A fake lives in `clipboard::fake` for tests.
- Text reads reject content over the caller's byte cap instead of truncating.
  Image reads and writes enforce an encoded-byte cap and a pixel cap; writes check
  the cap and the declared dimensions before decoding.
- Writes accept PNG, JPEG, WebP and BMP. The declared MIME type must match the
  sniffed content. Errors are typed: `Empty`, `TooLarge`, `Unsupported`,
  `Unavailable` and `Decode`. Image reads return PNG.
- Calls block on the platform clipboard; run them on a blocking worker.

### Linux

`arboard` serves a selection from a background thread owned by its `Clipboard`,
and ownership ends when that handle drops. The module keeps one handle in a
static for the life of the process, so a paste works after a write returns and
shutdown never waits on the clipboard. Selection data is lost when the app
exits unless a clipboard manager keeps it.

Wayland uses the `wlr-data-control` or `ext-data-control` protocol through
`wl-clipboard-rs`. GNOME's compositor (the Ubuntu 24.04 default) does not
implement either, so `arboard` falls back to X11 through XWayland; that path
needs Xwayland to be running. **Probe** on Ubuntu 24.04 GNOME Wayland and X11
sessions before relying on either.

### Gaps

- Only the unit tests with the fake run in CI; the ignored
  `clipboard::tests::live_text_and_image_round_trip` touches the real clipboard
  and is run by hand.
- Linux behavior was not exercised on a Linux host for this change.
- Only the `CLIPBOARD` selection is used; `PRIMARY` is not read or written.
- HTML, rich text and file lists are not handled.

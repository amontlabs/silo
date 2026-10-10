# Built-in Linux desktop and computer use: plan

Status: approved 2026-10-01; the backend integration (sections 3 to 5) is implemented
and verified live (2026-10-02, see the evidence in [ChatGPT app](SiloUI-CHATGPT-APP.md#integration-2026-10-02)). This replaces the optional,
per-computer desktop installation described in [Linux desktop](SiloUI-DESKTOP.md)
for new computers, and replaces Luda with [LCU](https://github.com/0xpolarzero/lcu).

## Goal

A new computer has a Linux desktop and agent computer use ready with no setup: a
user creates a computer, starts an agent (Claude Code, Codex, Pi, OMP or Hermes), and
the agent can use the desktop immediately. The only prompts are the harness's
own approvals, which a per-computer switch can turn off for computer use. That switch
configures the agents' approval prompts; it is a convenience, not a security
boundary (see [Approval](#approval-design-2026-10-02)).

## Decisions

- The desktop is part of every new computer, baked into a published v4 guest image.
  It starts with the computer because LCU needs a running Xfce session.
- Silo never publishes OpenAI files. Every device that runs Silo downloads
  the official ChatGPT Linux `.deb` from OpenAI by itself, in the background,
  and keeps one read-only copy shared by all computers on that device. Owner
  decision 2026-10-02: no consent prompt, notice or setting. The app tells the
  user in one sentence (bundled help) and shows a failed device, with Retry,
  in a Settings, Connections section that is otherwise absent. Downloading never blocks creating or
  starting a computer; a failure retries with backoff and can be retried by hand.
- Silo pins a tested pair: an LCU release and a ChatGPT app version with
  per-architecture SHA-256. The owner updates the pair by hand after testing.
  No automatic tracking of new ChatGPT releases.
- The pinned app copy is private to LCU: mounted read-only at a Silo path, not
  `/usr/lib/chatgpt`, not installed through dpkg, with no apt source and no
  launcher. A user who wants ChatGPT in a computer installs it normally; that copy
  never affects LCU.
- Per-computer approvals switch, off (ask) by default, driven by the host and never
  defended against the guest (see [Approval](#approval-design-2026-10-02)).

## Evidence (2026-10-01)

Measurements and tests are in [guest image size](SiloUI-GUEST-IMAGE-SIZE.md)
and were reproduced by parallel investigations; the summaries below are the
facts the plan depends on.

- **Shared storage.** MicroSandbox 0.7.4 stores an image once in a
  content-addressed cache; each computer adds a sparse overlay disk (about 4 MB when
  created). A read-only host directory mount (`-v DIR:GUEST:ro`) is enforced
  by the host: writes fail with EROFS even after a guest remount. About 215
  test boots with Silo-like sizing and the 1.5 GB app mounted showed no hangs.
- **v4 image size** (desktop, Selkies, ChatGPT and LCU system libraries,
  accessibility defaults; no OpenAI files). Final, as locked in
  `app/SiloUI/guest-image/image-lock.json` (`archiveBytes` and `unpackedBytes`;
  decimal): arm64 414.84 MB compressed / about 1.299 GB uncompressed; amd64
  423.48 MB / about 1.370 GB. The planning estimates, from the recipe
  measurement before publication, were arm64 389 MB / 1.24 GB and amd64
  400 MB / 1.31 GB. The v3 base is 86 / 89 MB gzip.
- **ChatGPT app.** OpenAI's apt repository
  (`https://persistent.oaistatic.com/codex-app-prod/linux/deb`, suite
  `stable`) is signed by key `3BFA0E4AE8B8CC16A2D9BA684A3B4A566C4660E4`
  ("Codex Linux Repository"); its index lists the latest version's SHA-256 and
  older versions remain downloadable from the pool. Version 26.928.31416:
  arm64 453 MB download, about 1.5 GB unpacked, 4,504 paths, no case
  collisions, no setuid files. macOS `/usr/bin/tar` extracts `data.tar.xz`
  from the `.deb` directly.
- **App versions are dates** (`26.928.31416`) and the CUA runtime is `0.0.x`;
  neither signals compatibility, so pinning is by tested pair.
- **No runtime approvals on Linux.** The ChatGPT CUA runtime's Linux action
  modules have no approval code; an end-to-end LCU 0.7.0 run from a read-only
  app (doctor, window list, screenshot, typing and Save with an independent
  file check) produced zero elicitation requests.
- **Harness approvals** (LCU's MCP server is named `lcu`; the `js` tool has no
  annotations):

  | Harness | Default | Setting that removes the prompt |
  | --- | --- | --- |
  | Claude Code | asks | `permissions.allow: ["mcp__lcu"]` (tested, 2.1.204) |
  | Codex | asks (`exec` fails under approval policy `never`) | `[mcp_servers.lcu] default_tools_approval_mode = "approve"` (tested, 0.159.3) |
  | OMP | no prompt (`yolo` default) | `tools.approval.js: allow`, `js_reset: allow` for users in other modes |
  | Pi | no permission system | none |
  | Hermes | no gate on plugin tools without a `pre_tool_call` hook | none |

- **Accessibility.** GTK and Qt expose trees by default. Firefox needs
  `org.a11y.Status.IsEnabled`, set durably by the dconf default
  `toolkit-accessibility=true`. Chromium and Electron expose web content only
  after an AT-SPI client calls `GetAttributes`/`GetRelationSet`
  (`OnExtendedPropertiesUsedInWebContent`); a small autostarted poller makes
  Chrome 154 expose 242 nodes (same as `--force-renderer-accessibility`),
  versus 4 without it. No environment variable or policy does this.
- **Editor.** GTK 3.24 `gtk_text_view_accessible_paste_text` passes a stack
  pointer to an asynchronous clipboard callback; AT-SPI `PasteText` with an
  external clipboard owner crashes every GTK3 text view (Mousepad, gedit,
  l3afpad). GNOME Text Editor (GTK4, 5.7 MiB) passes paste, insert and set.
- **MicroSandbox.** Restore never carries host mounts on disk snapshots; they
  must be passed again with `msb restore -v`. `msb modify` cannot add mounts.
  Mount roots through a symlink are refused by design (late, unclear error).
  0.7.5 fixes an `msb exec` piped-stdin hang (#1549) and adds `--no-stdin`;
  open issues #1701/#1702 report that the first statfs on a large read-only
  mount walks the whole host tree.

## Work

### 1. LCU

1. Use the installed Linux app in place instead of copying it (done in
   `1dc06ac`).
2. Record tested app/runtime pairs; `setup` and `doctor` report tested or
   untested, warning without blocking.
3. An approval mode for setup that adds or removes only LCU's own entries in
   each harness (table above), reversibly.
4. Release.

### 2. MicroSandbox upgrade

Upgrade the bundled runtime from 0.7.4 to the latest release (0.7.6), carrying
Silo's patches forward, mainly for the exec stdin fix. Review renamed commands
and flags (`branch` → `fork`, `--forked` → `--cow-mem`; old names remain
deprecated aliases) and path handling changes (relative host paths become
absolute).

### 3. v4 guest image

- Desktop recipe (Xfce, Selkies 2.0.0) installed at image build.
- ChatGPT runtime dependencies and LCU system packages, so LCU installs with
  `--skip-system --offline`.
- Pinned LCU release archive, hash-checked, staged for installation in the computer
  (done: `guest/lcu-lock.json`, `/usr/local/share/silo/lcu/`). The published v4 image
  stages LCU 0.8.1; Silo now pins LCU 0.11.0 (below), which a computer downloads and
  verifies at setup until a new image stages it.
- Accessibility: dconf `toolkit-accessibility=true` system default and an
  autostarted AT-SPI attribute poller for Chromium/Electron.
- GNOME Text Editor as the `text/plain` default instead of Mousepad.
- Build with bind mounts, never `COPY` of a package that is later deleted.

### 4. ChatGPT app on each device

- Lock: app version, per-architecture SHA-256 and runtime version, plus the
  LCU version and SHA-256.
- No notice or consent: the download starts by itself (decision of 2026-10-02).
- Download the exact pinned version from OpenAI's pool; verify SHA-256.
- Extract only `usr/lib/chatgpt` (macOS `tar`, Linux `dpkg-deb -x`); never run
  maintainer scripts. Refuse case collisions, setuid/setgid files, absolute or
  escaping paths and links leaving the tree. One immutable folder per version
  in a per-channel Silo data directory; publish atomically; delete the `.deb`.
- Pass canonical paths to MicroSandbox.
- Every device does this itself at its own start, remote ones included; a
  controller never prepares an app for another device.

Done: lock (`lcuVersion` 0.11.0), download, verification, extraction and
publication under `<app data>/chatgpt/published/`, started automatically at app
start with retries (2026-10-02, replacing the one-time notice), cached status
reads and a device-level Retry. See [ChatGPT app](SiloUI-CHATGPT-APP.md).

### 5. Computer integration

Done (see [built-in computer use](SiloUI-DESKTOP.md#built-in-computer-use)):
mount at creation and on every restore (verified), boot and app-ready sync,
approval modes, `setup-computer-use`, `computerUse` desktop state, garbage
collection of unused versions. Decisions: the guest step is pushed and started
by the host after every boot (`prepare_booted`) and when the app becomes ready
while computers run, rather than by a guest boot hook, so the helper always matches
Silo; garbage collection runs at start and after a prepare, only while no computer
runs, and holds the device-wide operation gate (which every computer start takes)
from the inventory through the deletion, skipping when any operation is active or a download holds the storage lock (it never waits for either); a skipped pass stays pending and is retried every two minutes until it ran.
Running the helper happens on a host background thread, never inside Start: it
first checks the computer is still the same running instance, then reads the approval
policy inside the computer's operation turn.

- New computers are created with a stable per-device folder mounted read-only at
  `/opt/silo/chatgpt`. That folder holds only verified, published version
  folders (staging, downloads and records live elsewhere) and is garbage
  collected, which keeps the first-statfs walk (#1701/#1702) small. It exists,
  possibly empty, before any computer starts, so a computer created before the automatic
  download finished gains computer use later, and a pinned-version change
  reaches existing computers at their next boot.
- Creation finishes everything, so the first start is only a start. It waits, outside
  the device-wide operation gate (a held gate stalls lifecycle operations and Quit),
  for the background image import and the ChatGPT download, then after the desktop
  configuration boot runs the apply once in a temporary boot with the session up. A
  failed apply or a "Finish without computer use" choice leaves it to the first boot
  apply and says so in the Created toast. New computers also get the verified LCU archive
  read-only at `/opt/silo/lcu` when Silo holds it; the helper prefers it over the
  staged and downloaded copies.
- At boot, a guest helper (`apply`) installs LCU against the mounted app when the
  pinned pair changes, runs `lcu setup --agent all --allow-missing`, and applies the computer's approval
  mode. All supported agents are registered automatically, including ones installed
  later: pending agents are registered by `lcu setup --reconcile` at every boot and
  `apply`, by a small guest watcher while the computer runs, and by a login hook (see
  [Agents installed later](SiloUI-DESKTOP.md); no user action).
- Export, import and transfer pass the mount again on restore and verify it.
- Changing the pinned version updates a computer at its next start.
- Remove app versions no computer references.
- Computers created before v4 keep their desktops; computer use requires a new computer.

### 6. Remove Luda and simplify

Remove the Luda recipe, status fields, repair action and documentation. New
Computers no longer offer "add a desktop"; keep only the minimal path existing computers
need.

### 7. Upstream reports

- GTK 3: `PasteText` use-after-free (standalone reproduction available).
- MicroSandbox: late, unclear error for symlinked mount roots; `msb restore -v`
  cannot attach disk images from the CLI; our case on #1701/#1702.
- MicroSandbox: the checkpoint integrity check (`crates/image/lib/checkpoint/resolver.rs`) admits 1 MiB for a
  virtio-fs device state while the runtime's restore admits 8 MiB (patch `microsandbox-checkpoint-fs-state`); a
  bind mount's stat-virtualization identity map is filled by the guest agent's report and so is never set in a
  RAM-restored guest (host uid visible; Silo mounts with `uid=0,gid=0`); `microsandbox-runtime-instance-id` restores the instance id the post-boot sync needs.
- LCU/OpenAI `node_repl`: the default network-disabled sandbox blocks the native X11 connection (see section 9).

### 8. Verification

In the packaged Dev app with throwaway `e2e-*` computers on macOS arm64 and the
Linux x86-64 test device: fresh computer with no prompts and ready `doctor`;
Claude Code and Codex desktop tasks with independent file checks, approvals
on and off; export/import keeps the mount; GTK, Qt, Firefox, Chrome and
Electron expose trees; poller CPU cost; `df` on a cold cache; pinned-version
change. Record final sizes and add a `minor` changeset.

### 9. Live verification without a model (2026-10-02)

macOS arm64 (Silo main plus the fixes below, MicroSandbox 0.7.6 built from `runtime-inputs.json`,
bundled `msb` ad-hoc signed with `Entitlements.plist`, published v4 image
`ubuntu-24.04-v4-arm64`, ChatGPT 26.928.31416 downloaded by Silo's own downloader, LCU 0.8.1, the pin at that time).
Real code paths through the opt-in live tests listed in
[Rust test support](SiloUI-RUST-TEST-SUPPORT.md#live-tests-and-temporary-directories); fixture
homes under `/tmp`, `e2e-*` sandboxes, live data, no packaged app. The Linux x86-64 computer was not used.

- Fresh computer: built in, desktop session running at start, computer use `installing`, then `ready`
  (about 25 s after Start); `lcu status --json` reports `tested`, `lcu doctor` passes (window list and
  screenshot), the folder is mounted `ro` and writes fail with EROFS. Accessibility: system default
  `toolkit-accessibility=true` and the poller run; no browser ships in the image, so Firefox,
  Chromium and Electron trees were not exercised.
- LCU's own MCP client (`adapters/client.mjs`), no model: GNOME Text Editor (GTK4) with `typeText` and
  `paste` does not crash, Save As through its dialog writes the expected bytes (checked by a separate guest
  command), per-key `pressKey` typing into Xfce Terminal writes its file. See the findings below.
- Approval: `apply_approval_with` `auto` adds exactly `default_tools_approval_mode = "approve"` (Codex) and
  `permissions.allow: ["mcp__lcu"]` (Claude Code, with Codex 0.160.0 and Claude Code 2.1.287 installed from npm);
  `ask` removes exactly those and nothing else. With nothing installed LCU's installer still creates `~/.codex`,
  so Codex is always registered and gets the approval line; Claude Code gets nothing until it is installed.
- Lifecycle (every step ends with the session running, computer use ready, the folder read-only and `lcu doctor`
  passing): restart, stop and start, checkpoint of the running computer, fork, in-place restore; export, import into a
  second home with its own folder (the imported computer takes the destination's `ask`).
- Boot loop (stale PulseAudio): 31 boots (one fresh computer, 10 restarts, 10 imports and 10 restarts of the imports, run twice) with 0 desktop failures: every one ended with the session running and computer use ready.
- Pre-v4: a computer from the v3 image has no mount, no desktop session, no helper and no computer-use state, and its
  restart, stop/start, checkpoint, fork and restore work.
- Numbers (cold home, one computer): the v4 image is a 396 MB archive and 1.30 GB in MicroSandbox's cache; a created computer
  takes about 33 MiB on the host at ready and 89 MiB after the desktop drive; the guest uses about 300 MiB of
  3.9 GiB with the desktop running (about 520 MiB after driving apps); creating the computer (image import) took 50 to 61 s
  and create to ready 61 to 76 s. The ChatGPT app download, verification and extraction took 120 s in the debug
  test profile.

Fixed by this verification (each with a test):

- Computer use never installed on a real runtime. The post-boot sync requires a runtime instance id, which
  MicroSandbox 0.7.6's `inspect` did not report (Silo's 0.7.6 patch set had dropped it), so the identity check always
  failed. `microsandbox-runtime-instance-id` restores the field; Silo reports a runtime without it instead of skipping.
- Capturing a checkpoint of a running built-in computer failed with `checkpoint object exceeds 1048576 bytes`:
  MicroSandbox's integrity check admits 1 MiB for any device state while its restore admits 8 MiB for virtio-fs, and
  the folder's passthrough table was 1.18 MiB. `microsandbox-checkpoint-fs-state` applies the restore's limit.
- After a RAM restore (checkpoint fork or restore) the guest saw the host uid on the folder, LCU refused to start
  ("not in a location only root and this account can change") and Silo still reported `ready` from the previous
  boot's receipt. The folder is now mounted with `uid=0,gid=0`.

Findings left open:

- LCU's `js` tool cannot reach the X server in its default configuration. The original `node_repl` runs code in a
  `codex sandbox` with the network disabled, whose seccomp filter denies every `connect`, local sockets
  included (`Could not connect to X11 ... Operation not permitted`); `lcu doctor` does not go through it. A harness
  that supplies its own sandbox state (Codex) decides this itself; Claude Code supplies none. The drive test
  starts LCU with `CODEX_CLI_PATH` empty (kernel started directly), and reports the default configuration without
  asserting it. Needs an LCU decision (default sandbox state or the direct kernel) before real agent runs.
- `pressKey` sends X events to the selected window (`SendEvent`), which GTK4 ignores: per-key typing and shortcuts
  such as Ctrl+S do nothing in GNOME Text Editor, while `typeText`, `paste` and AX actions
  (`performSecondaryAction`) work. In GTK4 `typeText` also reports `SetCaretOffset NotSupported`, an error
  result although the text was inserted. GTK3 and VTE (Xfce Terminal) take `pressKey`.
- Silo's `computerUse.state` after a restart comes from the receipt on disk; only `lcu doctor` proves it
  (the lifecycle test checks both).

## Integration contract

Backend (Rust, guest scripts) and frontend implement this together.

- Device storage: `<app data>/chatgpt/` keeps `.lock`, downloads, staging and
  publication records; verified trees are published under
  `<app data>/chatgpt/published/<version>-<debarch>/`. Computers mount
  `published/` read-only at `/opt/silo/chatgpt`; the guest uses
  `/opt/silo/chatgpt/<pinned version>-<debarch>` passed by the host.
- Preparation is automatic and per device. At app start, off the UI thread and
  at low priority (utility QoS on macOS, nice 10 on Linux), the app reads the
  status and, unless the pinned version is published, waits 10 s and runs the
  download in one background worker (never two at once: an in-process slot plus
  the storage lock). A retryable failure (network, firewall, disk space) is
  retried after 30 s, 1, 2, 5, 10, 30 min, then hourly, until it succeeds or the
  app quits; a failure retrying cannot fix (checksum mismatch, or HTTP 404/410 for the pinned
  version; a 401/403 refusal by a proxy or filter is retried) stops the worker until Retry. Offline or metered
  connections only mean later attempts: nothing waits for the download, and computer
  creation, start and restore never depend on it (the mount folder exists,
  possibly empty). When the app becomes ready the worker syncs running built-in
  computers at once (`computer_use::app_ready`) and collects unused versions.
- Commands (Tauri): `chatgpt_app_status { device? }` and
  `chatgpt_app_retry { device? }`, where `device` is a remote device's
  id (omitted: this device; a computer target is rejected). Retry wakes a
  waiting worker or starts one, and returns the status at once. Removed:
  `chatgpt_app_accept_notice`, `chatgpt_app_prepare`, the consent file and the
  consent state. Events: `chatgpt-app-status` carries this device's status
  object (`device: null`); a remote device has no events, the controller
  reads `chatgpt.status` (about every 3 s while it works, 15 s otherwise).
  Also `set_computer_use_approval { computer, mode: "ask" | "auto" }`
  returning the desktop state, and the `desktop_action` action
  `setup-computer-use`, which reruns LCU setup (the panel's Try again after a failed setup).
- Bridge methods: `chatgpt.status` (read), `chatgpt.retry` (change, no computer id),
  `computerUse.approval`. Removed: `chatgpt.accept`, `chatgpt.prepare` and the
  placeholder `silo-remote:<deviceId>:<nil-uuid>` routing. An owner on an older Silo
  answers `chatgpt.retry` as unsupported and `chatgpt.status` with its own
  consent-era states; the controller reads the owner's handshake capabilities
  (cached for a minute) and shows a device without `chatgpt.retry` as `unknown`
  without asking its status, not as an error (`chatgpt_app_status` maps "unsupported" to `{"state":"unknown"}`,
  and the frontend maps any state it does not know to `unknown`).
- `desktop.builtIn: boolean` in a computer's saved/reported `desktop` object marks a computer
  created from a v4 image. Silo decides it; a written value is ignored.
- App status object, tagged by `state`: `idle` (waiting to download),
  `downloading { receivedBytes, totalBytes }`, `verifying`, `extracting`,
  `ready { path, version }`, `failed { reason, retryable }`. The controller
  adds `unknown` for a device whose status it cannot read.
- Desktop state (`read_desktop_state`) gains an optional `computerUse` object
  for v4 computers: `state` (`unavailable`, `preparing`, `installing`, `ready`,
  `failed`; `preparing` covers waiting, downloading and a failure Silo retries
  by itself, with the reason; `failed` is a final failure), `reason`, `cause`
  (present only with `app-download`: the failure is the device's ChatGPT download,
  which Retry in Settings → Connections or on the computer's page restarts; setting up
  the guest cannot fix it; absent for a guest setup failure), `compatibility` (`tested`,
  `untested`, `unknown`, from `lcu status --json`), `warning`, `approval`
  (`ask`, `auto`, or `unknown` when the saved policy file exists but cannot be read:
  the user's choice, shown by the switch), `appliedApproval` (`ask`, `auto`, or
  `unknown`: the last mode the host applied completely, `unknown` before any was;
  independent of the app download and of the computer running), `approvalApply`
  (`applied`, `pending`, `failed` or `partial`: how applying `approval` stands, from
  the host's own record of the last attempt) and `approvalApplyReason` (words for the
  user, only with `failed` or `partial`), `appVersion`, `runtimeVersion`, `lcuVersion`,
  `agents`. An older Silo omits `approvalApply`, which readers take as `applied`.
  The legacy `lcu*` fields remain for computers created before v4.
- Per-computer approval is the host's: see [Approval](#approval-design-2026-10-02) for the
  contract (desired mode, last attempt, one apply at a time per computer, cancellable,
  bounded). The guest helper is a plain executor: `silo-computer-use apply --approval
  ask|auto [--force] [--boot]` installs what is missing, runs `lcu setup --agent all --allow-missing ... --approval
  <mode>` (`--agent auto` before LCU 0.8.8) and `lcu setup --reconcile`, waits for the desktop session and runs `lcu doctor`, then prints its
  `status` plus `apply: {approval, outcome, reason}` where `outcome` is `applied`,
  `partial` or `failed`. It keeps no approval record, orders nothing and accepts every
  request; `status` reads the receipt only and never reports approval.

## Approval design (2026-10-02)

The per-computer "Allow without asking" switch is a convenience, not a security boundary.
Agents in the computer have root and can edit their own harness settings
(`~/.claude/settings.json`, Codex's `config.toml`) directly, so no amount of
bookkeeping on the guest's disk could make the switch binding. An earlier design (a
random policy generation and a monotonic revision per computer, passed to the guest, which
accepted or ignored requests by them, with forgery detection on the host and a record of
"applied" derived from the guest's own report) defended against forged guest state. It was
the source of a stream of race bugs and protected nothing a guest could not undo, so it was
replaced by a model in which the host drives the guest and the guest cannot veto.

- **What the host stores** in `<storage>/computer-use/<id>.json`: the *desired* mode (the
  user's choice, default `ask`), the last mode applied completely (`applied`) and the
  *last attempt* (`mode`, `outcome` of `applied`, `failed` or `partial`, time, reason code)
  and an *unfinished* marker: the mode of an attempt, written before the helper runs and
  replaced by its result, so an attempt cut short by a crash is applied again at the next
  app start even when the last recorded result matches the choice. An attempt whose marker
  cannot be saved (full disk, permissions) does not start: the helper is not run and the
  attempt is recorded as failed (`state-not-saved`) when the file can be written at all.
  Old files carry a revision and a generation; both are ignored. Only a user change, a
  fork, an import's reset or an apply writes the file, all under one lock; a status read
  never changes it. A file that cannot be read shows as `unknown` and is replaced by the
  default (ask) by the next apply, which fails closed.
  An unfamiliar saved attempt outcome counts as failed, so setup is retried without
  discarding the desired approval mode. Unknown approval modes and malformed field
  types still make the policy unreadable. The saved outcome uses Serde's
  [field deserializer](https://serde.rs/field-attrs.html); live helper responses
  retain their strict outcome parser. Fixture tests cover the unfamiliar outcome,
  save/reload, legacy policy defaults and unknown approval modes.
- **Applying.** Every apply runs on a host thread (never inside Start, never on the UI
  thread) that takes the computer's operation turn: the per-computer lock that already serializes all
  work on one computer. Inside the turn it re-checks that the computer is the same recorded computer
  and the same running instance, reads the *current* desired mode, and runs the helper
  synchronously within a bound (15 minutes plus a minute of host allowance), never
  detached. Because the mode is read when the turn arrives, a queued apply can never write
  an older choice over a newer one, and a queued apply that an earlier one made redundant
  does nothing. After each run the turn compares the choice at that moment with the mode it
  applied and applies again until they agree, so a choice made while the helper ran is never
  left pending. A user change returns at once; the state says `pending` until the apply
  ends. The boot sync stays on its own background thread, so Start is never blocked.
- **Cancellation.** The turn is cancellable. A queued stop, restart or delete of that computer (a delete is
  device-wide, so the operation gate records which computers it removes), or a
  device-wide shutdown (Quit or an update, which has no computer id), cancels the running
  helper at once; the setup action behind the panel's Try again has the same watcher. A queued start (the computer is already running), a dismissed error and any other work wait for the turn like any other operation on that computer. A
  cancelled or timed-out apply is recorded as a failed attempt (`cancelled`, `timed-out`)
  and nothing is assumed rolled back; the next boot or app start tries again.
- **When it runs.** After every boot and when the app becomes ready (the helper does the
  install, setup and readiness check, and is cheap when nothing changed), when the user
  changes the switch of a running computer and at app start for each running computer whose last
  attempt is missing, failed, partial, cut short or for another mode than the desired one.
  The host never reads the guest to decide. A run that finds the ChatGPT app not there yet
  is not an attempt: the apply stays pending until the app is ready.
- **Imports, transfers and forks.** An import or transfer starts from this device's
  initial mode (the `computerUseAutoApproval` app setting, ask unless turned on) with no
  attempt on record, whatever policy an earlier computer of that id had, so its first boot
  applies it over the configuration the imported disk carries. A newly created computer starts
  the same way; for one created on a connected device the controller sets its own
  setting's mode afterwards with `computerUse.approval` (an older Silo there keeps its own). A fork
  inherits its source's desired mode with no attempt on record, so its own first boot
  applies it.
- **Reporting.** `approval` is the desired mode; `appliedApproval` the last mode applied
  completely (never hidden by the app download state or by the computer being stopped);
  `approvalApply` is `pending` while an apply is scheduled or running or no attempt for the
  desired mode exists yet, and otherwise the last attempt's outcome. A failed or partial
  attempt stays visible until a later one applies completely, even when the user chooses
  the previously applied mode again.
- **Panel.** The switch shows the desired mode. `pending` shows "Applying…" while the
  computer runs. After choosing ask the panel warns "Agents may still act without asking
  until this is applied." while the apply is pending, and "Not applied to every agent. Some
  may still act without asking." once it `failed` or was `partial`, when the previous applied
  mode was `auto` or nothing says ask is in place; a failed or partial result also gives the host's
  reason. After a command error the panel reads the state again instead of restoring the
  snapshot from before the change, because the command may have stored the choice or even
  applied it before the answer was lost.
- **Why not more.** Defending the switch would need a boundary the guest cannot cross
  (a host-side MCP gate), which is a different product decision. The documentation and the
  panel say once that the switch configures the agents' approval prompts and is not a
  security boundary inside the computer.

### LCU 0.8.2 pin (2026-10-02)

Silo pins LCU 0.8.2 (`guest/lcu-lock.json`, `chatgpt-app-lock.json` `lcuVersion`).
0.8.2 fixes the Linux X11 "Operation not permitted" for every harness: it supplies
Codex's disabled sandbox-state `_meta` by default, so a bare MCP client reaches the
display. Silo must never set the opt-out `LCU_NODE_REPL_SANDBOX=host`. It also
documents GTK4 input behavior. `scripts/install.py` (including `SYSTEM_PACKAGES`) is
identical between v0.8.1 and v0.8.2, so the v4 image's package set is unaffected.

The published `ubuntu-24.04-v4` image still stages the LCU 0.8.1 archive. The guest
helper (`silo-computer-use.py`, `archive_path`) uses the staged archive only when its
SHA-256 equals the lock, so on those computers it downloads the locked URL (they have
network by default), verifies the hash and installs 0.8.2 in place over an existing
0.8.1 install (`installed_for` compares `lcu_version`) at the next boot or sync.
Offline consequence: until a new image stages 0.8.2, computer use setup needs
network once per computer (the earlier install keeps running meanwhile). Guest tests cover
mismatch, download, install and upgrade of an existing install.

Live check (macOS arm64, Silo main plus this pin, MicroSandbox 0.7.6 `msb` ad-hoc signed with
`Entitlements.plist`, published v4 image `ubuntu-24.04-v4-arm64`, ChatGPT 26.928.31416; fixture home
under `/tmp`, `e2e-lcu` computer, no packaged app). A new built-in computer, created and started through
Silo's own paths, found a staged archive that does not match the lock, downloaded the locked 0.8.2
URL, verified it and installed it: `lcu status --json` reported `lcu_version` 0.8.2 and compatibility
`tested`, `lcu doctor` reported ready, and a bare MCP client with no `_meta` at all listed windows and
took a screenshot through the `js` tool (`DRIVE_MODE=bare` in
`test_support/computer_use_live/drive.mjs`). The full drive (GNOME Text Editor, Save As, per-key typing)
now passes in LCU's default configuration, without the previous `E2E_NO_SANDBOX` workaround. One earlier
attempt ended `computer use failed` (`lcu-archive-unavailable`) because the guest's first download over
the host network returned an empty reply; the retry succeeded. The upgrade of an existing 0.8.1
install is covered by the guest unit tests only, not live.

### Network retry for the LCU download (2026-10-02)

The empty reply (and one DNS timeout) seen in two live runs is transient, and real users on
flaky networks will see the same. The helper (`download` in `guest/silo-computer-use.py`) now
tries the HTTPS-only download up to five times, waiting 5, 10, 20 and 40 s between attempts
(never starting one after 420 s), and each curl uses its own `--retry 3 --retry-all-errors
--retry-connrefused` with `--connect-timeout`/`--max-time`; every attempt is logged to
`/var/log/silo-computer-use.log`. A hash mismatch is never retried (`lcu-archive-mismatch`).
When the attempts run out the helper reports `lcu-archive-unavailable`, the one failure code
the host treats as retryable: `apply_with` in `computer_use.rs` waits 1, 5 and 15 minutes
(each wait outside the computer's operation turn, then a normal serialized apply) while the same
running instance is up. During a wait the state is `preparing` ("Could not download LCU
(network). Silo tries again automatically."); a boot, a switch change, a manual setup, a
stop/restart or a deletion cancels it. After the last retry the failure stays until the next
boot or a manual setup.

### LCU 0.11.0 pin and cross-turn Computer Use (2026-10-10)

Silo pins LCU 0.11.0 (tag `v0.11.0`; linux-arm64
`bfb91127e103065e47088545c4d71dcb00714eec05fcf72ff30913212c485dc8`, linux-x64
`12919c3bd94f1d74874e8a4da4e5d7c713138079b613df05e5f81a1e03e6a62c`, darwin-arm64
`4d230807febb4d4cdae6780643bf4605693f19dd6be18e34249be59c37d90bd2`, matching the release's
notes and verified by download). The ChatGPT app pins are unchanged. 0.11.0 adds an opt-in
setting that keeps Computer Use available across the turns of a Claude Code session, so a
background subagent still working after its turn ended does not lose it. Both guest setup
scripts enable it with `lcu setup ... --cross-turn on --unattended` (`--unattended` skips the
owner prompt, which no person is present to answer in a computer). The Linux script passes the
option only when `lcu setup --help` lists it. Computers set up with an earlier pin pick it up
through the changed pin (Linux receipt: `lcuVersion` and archive hash) or the changed script
and lock fingerprint (macOS) when setup next runs. Setting it again leaves it on.
The section below describes the 0.9.4 pin it replaces.

### LCU 0.9.4 pin (2026-10-05)

Silo pins LCU 0.9.4 (tag `v0.9.4`; linux-arm64
`2841dfaa0e28721d08bd49d9ef9d82554add95e09ea79c24e8c1d89a8b1f48e5`, linux-x64
`ad6137c19b46771959c52b803d0f072a08a57ee052e032c93542f2e448280195`, matching the release's
notes and verified by download). Compared with the extracted 0.9.3 archive only the Pi adapter,
documentation and bundle metadata differ: `scripts/install.py`, `tested-versions.json`, the runtime
and the Claude Code and Codex adapters a computer registers are unchanged. The section below
describes the 0.9.3 pin it replaces.

### LCU 0.9.3 pin (2026-10-05)

Silo pins LCU 0.9.3 (tag `v0.9.3`; linux-arm64
`a419284a52df0f970199a64631c3b4552a035a3a657cbbb015f2936763ec5db5`, linux-x64
`026b729a1131c5b58bca27b94dae62c3d248c0bf6135120b9e3f40f6ffcaa72e`, matching the release's
`.sha256` files and notes). `scripts/install.py` and `tested-versions.json` are unchanged from
0.9.2. The change that matters for computers: where bubblewrap can start, LCU now keeps the model's
JavaScript kernel inside `codex sandbox` (read-only filesystem, no network, confined
subprocesses) and runs only the verified Sky worker outside it; 0.8.2 to 0.9.2 ran the kernel
unsandboxed. Where bubblewrap cannot start, the kernel stays unsandboxed and `lcu doctor` says
so. See LCU's `docs/ADAPTERS.md#linux-sandbox-state`. The section below describes the 0.9.2 pin
it replaces.

### LCU 0.9.2 pin (2026-10-05)

Silo pins LCU 0.9.2 (tag `v0.9.2`, commit 9c6c5e6; linux-arm64
`18e0220c9abc2eade0178005ee8b99555a1aec493d70a6d3e0ad3a8c45989417`, linux-x64
`a0c8bf44c5d77123bfc567611fbcd2749c9bdb711ddbfa2c7d9c873a9fc2b070`, verified by
download; the archives are published at `amontlabs/lcu`, where the project moved from
`0xpolarzero/lcu`). Compared with the extracted 0.8.8 archive: `scripts/install.py` differs only in
one message, so `SYSTEM_PACKAGES` and the `--skip-system --offline` flow are unchanged, and
`tested-versions.json` is identical (the pair ChatGPT 26.928.31416 with runtime
`0.0.27/20260927214556-b77d38801cca`, last verified by LCU 0.8.0, is still covered). The runtime,
tool schema and input behavior are unchanged since 0.8.8. New in 0.8.9 to 0.9.2: launchers
re-execute under Python 3.12 or newer (Ubuntu 24.04 ships 3.12), setup reads installer output
through files and records its choices even when a step fails, `lcu update` and its update
notices, the macOS-only `lcu apps`, and the Claude app approval mod (`lcu-approve`, installed by
`lcu setup --agent claude-code`; Linux has no per-app approval). `lcu update` replaces an
installation from the network and the update notices check for new releases in the
background; Silo installs LCU itself from the pinned archive, so these are not part of its flow.
No guest image rebuild is needed: the host stages the pinned archive into each computer and the
guest helper uses it when its hash matches, else downloads and verifies it, so existing
`ubuntu-24.04-v4` computers pick up the new pin at their next setup.
The section below describes the 0.8.8 pin it replaces.

### LCU 0.8.8 pin (2026-10-03)

Silo pins LCU 0.8.8 (tag `v0.8.8`, commit 04fa368; linux-arm64
`edf19c055648245fadcfe073cfdc06623f7b5503f9c866b374ab8134f9439de1`, linux-x64
`c45e6375e8ee66d09882a6aa17eae6307243665d0f827b6619ea5990502e3d60`). It adds `lcu setup --allow-missing`, which registers the
installed harnesses (Codex and Claude Code even before their CLI exists) and records the
absent ones as pending, and `lcu setup --reconcile`, which registers a pending harness once its
binary appears, with the saved approval mode. The guest helper uses both (see
[Linux desktop](SiloUI-DESKTOP.md#built-in-computer-use)). `SYSTEM_PACKAGES` is unchanged.
The section below describes the 0.8.7 pin it replaces.

### LCU 0.8.7 pin (2026-10-03)

Silo pins LCU 0.8.7 (tag `v0.8.7`, commit edf6710; linux-arm64
`9c7f87529f6fc19a40ad518cf54b4709eef11af304ae39f5a1293b906bb84afb`, linux-x64
`f2c8f7930635cfd9743bd5022a90b1da5bc67ca25f0406261655207499778269`, verified by
download). It gives translated drags a time budget proportional to their path, releases the
buttons and keys a translated call pressed when that call times out or the engine fails,
refuses translated pointer input while another client holds a pointer grab, and proves the X
server's PID namespace from the display socket (TCP or unidentifiable servers fail closed).
Its gate also found that ChatGPT 26.928.31416's engine outlives the trusted worker, so LCU now
stops the engine itself on a timeout before releasing input. LCU's gates ran against both
26.915.31945 and 26.928.31416. `SYSTEM_PACKAGES` is unchanged. The section below describes
the 0.8.6 pin it replaces.

### LCU 0.8.6 pin (2026-10-03)

Silo pins LCU 0.8.6 (tag `v0.8.6`, commit d6db28d; linux-arm64
`7ff9d94589b72d9f4896c3f7ab1fad36282974d59aba66df66003f6445431254`, linux-x64
`47b23234a82cb9fd09431b65f51476f05fec93e4e10e949c2ae474e78c7d1c5f`, verified by
download). It closes the last review findings in the Linux input translation: before a
translated pointer action the X server's window chain at the point must contain the target
(an overlapping overlay refuses the action instead of receiving it), original engine calls
from the queue are bounded to 30 s with the trusted worker reset on timeout, the X server
must prove it shares LCU's PID namespace before its client PIDs are trusted, an unreadable
namespace fails closed, and only translatable requests run the identity helper.
`SYSTEM_PACKAGES` is unchanged. LCU's own gates ran against ChatGPT 26.915.31945 only; the
pinned 26.928.31416 is covered by Silo's live tests. The section below describes the 0.8.5
pin it replaces.

### LCU 0.8.5 pin (2026-10-02)

Silo pins LCU 0.8.5 (tag `v0.8.5`, commit 28a90d0; linux-arm64
`3a0856210656207a1d80b08077888701a0278407343d5276b1f6607c637c3722`, linux-x64
`0caa6e8fbbbfef8c6c8b9b7de9b2b5c123869c76023f9f75f2b7e5cbd3410f30`, verified by
download). A review of 0.8.4 found that translated key holds could stay stuck and that a
namespaced client's advertised PID could match an unrelated local process, so 0.8.5 narrows
the feature: `key_down`/`key_up` are never translated (they pass to the original service as
in Codex), every input and focus-changing call is serialized in one queue, and a window is
translated only when the X server's own client PID (X-Resource `XResQueryClientIds`) equals
its `_NET_WM_PID` on this host and PID namespace; anything else fails closed. That check needs
`libxres1` and `python3`: the published v4 image already contains both, and the Dockerfile now
lists `libxres1` explicitly. The section below describes the 0.8.4 pin it replaces.

### LCU 0.8.4 pin (2026-10-02)

Silo pins LCU 0.8.4 (tag `v0.8.4`, commit 78e75a4; linux-arm64
`f3ca87eea22a9c1c335bbe3a0c3df5c8b1be46ef96359b80fbd67ef9fc1f6f79`, linux-x64
`06b481b35073c43b4064f257a0603e7812f53aa3299db16df3f363e0f9f1059d`, verified by
download). It fixes review findings in 0.8.3's Linux input translation: planning and a
final focus check run inside the serialized queue (a mismatch is an error, nothing is typed
elsewhere), translated key holds are owned and always released, pointer input must fall
inside the target's current client rectangle, modal redirection is keyboard only, a
caller-supplied `NODE_REPL_TRUSTED_SERVICES` map is kept verbatim, the toolkit cache is keyed
by process start time, and windows from other machines or PID namespaces are left
untouched. `scripts/install.py` is unchanged, so the v4 image's packages still suffice. The
section below describes the 0.8.3 pin it replaces.

### LCU 0.8.3 pin (2026-10-02)

Silo now pins LCU 0.8.3 (tag `v0.8.3`, commit 93f3978; `guest/lcu-lock.json`,
`chatgpt-app-lock.json` `lcuVersion`). The archive hashes were re-verified by downloading
both Linux archives (arm64 `f6ada7fc...b943b9`, x64 `bc4997cd...29add4fa`). The earlier 0.8.2
section above stays as the record of the 0.8.2 pin; everything it says about the staged
v4 archive applies unchanged: the published `ubuntu-24.04-v4` image still stages LCU 0.8.1,
the helper finds that the staged archive does not match the lock, downloads the locked 0.8.3
URL, verifies it and installs it in place at setup (network needed once per computer).

`scripts/install.py` is byte-identical between v0.8.2 and v0.8.3, so `SYSTEM_PACKAGES` is
unchanged and the v4 image lacks nothing. 0.8.3's toolkit detection runs `xprop` for
`_NET_WM_PID`; `xprop` is in `x11-utils`, which is in both the LCU package list and the v4
image lock (`image-lock.json`, arm64 and amd64). `python3-pyqt5` appears only in LCU's own
test Dockerfile and verification notes, not in `SYSTEM_PACKAGES` and not at runtime.

What agents get on the Linux desktop with 0.8.3:

- **Input translation.** The original Linux engine sends window-targeted `pressKey`,
  coordinate `click`, `scroll` and `drag` with `XSendEvent`, which GTK 4 (XInput2 only)
  ignores, so they used to succeed and change nothing in GNOME Text Editor. A `sky`
  trusted-service wrapper now detects a GTK 4 process (`_NET_WM_PID` plus
  `/proc/<pid>/maps`) and activates the window if needed, then issues the desktop-level
  call with converted coordinates, for keys, click, scroll and drag. Qt gets the same for
  scroll only. Everything else (GTK 3, browsers, Electron, the terminal) keeps the original
  path, which already worked. Opt-out: `LCU_LINUX_INPUT_TRANSLATION=off`; Silo does not set it.
- **No `node_repl` sandbox on Linux by default.** LCU documents that its Linux default is
  unsandboxed (the macOS sandbox does not exist there). `LCU_NODE_REPL_SANDBOX` is the
  opt-out and the host value must still never be set by Silo.
- **`typeText` is AT-SPI only.** It works for GTK 3, GTK 4 and Qt with accessibility on, and
  is unsupported in browsers, Electron, Java and terminals (VTE), which expose no AT-SPI
  text provider; there agents fall back to `pressKey` per key (or click and paste).
- **GTK 3 paste.** AT-SPI paste into a GTK 3 text view (gedit 46.2) crashes the app: an
  upstream GTK/GNOME bug, filed with GNOME. Silo's image avoids it by shipping GNOME Text
  Editor (GTK 4) and no Mousepad.

Live check (macOS arm64, Silo main plus this pin, MicroSandbox 0.7.6 `msb` ad-hoc signed with
`Entitlements.plist`, published v4 image `ubuntu-24.04-v4-arm64` staging LCU 0.8.1, ChatGPT 26.928.31416
published by Silo's own downloader; fixture home under `/private/tmp`, `e2e-lcu` computer, no packaged
app). The computer downloaded and verified the locked 0.8.3 archive and installed it over the staged
0.8.1: `lcu status --json` reported `lcu_version` 0.8.3 and compatibility `tested`, `lcu doctor
--require-ready` reported ready, and `live_lcu_drives_the_desktop_without_a_model` passed
(bare MCP client with no `_meta`, the default-sandbox drive, Save As, per-key terminal). The drive now
also sends window-targeted `pressKey` to GNOME Text Editor (GTK 4): `ctrl+a`, `BackSpace`, `keys-ok`
and `ctrl+s`; an independent read of the saved file showed `keys-ok` plus the newline the editor adds
on save, with the previous text gone. Per LCU's own measurements the same calls under 0.8.2 and earlier succeeded without effect.

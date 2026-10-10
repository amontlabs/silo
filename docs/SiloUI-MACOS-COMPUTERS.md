# macOS computers

Silo runs macOS computers on Apple Silicon Macs through Apple's
Virtualization.framework, next to the Linux computers it runs through
MicroSandbox. libkrun cannot boot macOS, so this is a second engine. The
engine choice, framework limits and Apple's license terms are in
[macOS guest engine research](research/macos-guest-engine-2026-10-09.md).

This is a first vertical slice: create, set up for computer use, start, view,
stop, checkpoint and delete. Most other Linux computer features do not apply yet; see
[not covered yet](#not-covered-yet).

## Behaviour

- **Availability.** macOS computers appear in the overview's Computers list,
  with a macOS badge, on Apple Silicon Macs only. On Linux devices and Intel
  Macs the commands report the feature as unsupported and macOS is hidden.
- **Create.** New computer has an Operating system select; choosing macOS asks
  for a name, CPUs, memory and disk size.
  Silo asks the framework for the newest macOS this Mac can run, downloads that
  restore image from Apple (about 20 GB), and installs it. Progress shows as
  Preparing, Downloading and Installing. The creation form states Apple's
  two-copy license limit. After installation Silo sets the computer up for
  computer use with no clicks ([provisioning](#provisioning-for-computer-use)),
  shown as Setting up macOS with the current step. A failed setup offers Retry
  setup, which resumes at the first unfinished step, and Start, so the screen
  can be inspected.
  Once a computer has finished setup, and before anyone starts it, Silo keeps a
  [template](#templates) of it. Later computers are copied from the template
  instead: Preparing, Copying macOS (the files are cloned, seconds), then Setting
  up with the detail "Personalizing the computer" (about a minute), so the second
  computer takes about a minute instead of about fifteen. A copy keeps its
  template's disk size, so the form's smallest disk is the template's, but only
  while that template is the one a new computer would use (current setup version,
  and the newest macOS when Silo knows it); a larger
  disk is grown, and the guest's APFS container is expanded to fill it. Without a
  network, a current template is used even when a newer macOS exists.
- **Start and stop.** Start boots the computer. Stop shuts macOS down over SSH
  once setup has created the account. Before that it presses the virtual power
  button, which makes macOS ask for confirmation in the computer's screen; the
  row says so. Force stop turns the computer off immediately. A shutdown from
  inside macOS shows the computer as stopped. Macs run at most two macOS
  guests at once; a third start fails with a message saying so. Silo does not
  limit how many macOS computers exist.
- **Screen.** Show screen opens a Silo window containing a native
  `VZVirtualMachineView`: frames, keyboard, trackpad and audio come straight
  from the framework, with no encoder, network transport or webview. The guest
  has Apple's paravirtualized GPU (`VZMacGraphicsDeviceConfiguration`, Metal
  inside the guest) and a virtio sound device playing through the Mac's output.
  The guest display follows the window size (`automaticallyReconfiguresDisplay`). Closing the window keeps the computer
  running; stopping the computer closes it.
- **Delete.** Delete removes a stopped or failed computer and its disk. During
  creation (including the copy and its personalization) the same action cancels
  it. Delete never removes the template; the macOS form's Remove template does,
  unless a copy is being made from it.
- **Checkpoints.** Each set-up computer has a Checkpoints button in its row that
  opens the same list, New checkpoint, Restore, Fork and Delete as the Linux
  computer page. See [checkpoints](#checkpoints).
- **Quit.** Quit asks running macOS computers to shut down, then turns off any
  that have not stopped by the deadline, as it stops local Linux computers.
  A creation in progress is cancelled and shows as failed on the next launch.

## Storage

All paths are inside the channel's application data directory
(`~/Library/Application Support/org.silo.dev` for Silo Dev):

| Path | Contents |
| --- | --- |
| `macos-computers/<id>/computer.json` | Name, resources, MAC address, installed macOS version |
| `macos-computers/<id>/disk.img` | Sparse raw disk; uses only what macOS has written |
| `macos-computers/<id>/auxiliary-storage.img`, `hardware-model.bin`, `machine-identifier.bin` | The framework's per-computer platform identity |
| `macos-computers/<id>/checkpoints/<checkpoint id>/` | One [checkpoint](#checkpoints): `checkpoint.json`, `disk.img` and `auxiliary-storage.img` (APFS clones), and `state.vzvmsave` (mode 0600) for a memory checkpoint |
| `macos-computers/<id>/inherited-access/` | A fork's copy of its source's password and SSH key (mode 0700), until its own personalization replaces them |
| `macos-templates/<build>-<setup version>/` | The template: `disk.img`, `auxiliary-storage.img` and `hardware-model.bin` as APFS clones of the source computer, `template.json` (macOS version and build, setup version, disk size, creation time, source computer), and `template-access/` (mode 0700), the source computer's password and SSH key |
| `macos-restore-images/<file>.ipsw` | The last downloaded restore image, kept so the next computer does not download it again; a `.partial` file resumes an interrupted download |

macOS computers are not entries in the MicroSandbox registry
(`computers.json`), so the Linux lifecycle, health, backup and Connections code
never sees them.

## Templates

The first computer that finishes setup is the expensive one: a 20 GB download, an
install, three boots and a Recovery session. Silo keeps its result as a
template, and every later computer starts from a copy of it.

**Making one.** At the end of setup, with the computer stopped and before the
user could start it, Silo clones `disk.img`, `auxiliary-storage.img` and
`hardware-model.bin` with `clonefile` (copy-on-write: the template costs only
what the computer later changes) into `macos-templates/<build>-<setup version>/`,
writes `template.json`, and copies the computer's guest-access secrets to
`template-access/`. The folder is built under a partial name and renamed, so an
interrupted run leaves nothing a copy could use. Only the newest template is
kept: making one removes older ones, except any a copy is being made from. A
computer qualifies only if it came from an installation (not from a template),
finished all four setup steps, and was never started by the user afterwards
(`pristine` in `computer.json`, cleared by Start). Computers created before
templates existed never qualify.

**Setup version.** A hash of everything setup installs: the pinned ChatGPT app
and LCU locks, the guest script, the offline account setup and the initial
computer use approval mode. The computer's record keeps the version of what it
actually has installed (the approval mode used when its computer use step ran), and a
template is made under that, not under the setting at the time of the copy. A template with a different setup version is never
copied, so changing a pin or the script makes the next computer install from
scratch and produce a new template.

**Copying.** `create_macos_computer` asks the framework for the newest supported
macOS as before. A template of that build with the current setup version is
copied; if the lookup fails (offline) the newest current template is copied;
otherwise the computer is installed. Silo checks free space (the copy writes a
few GB; it keeps 5 GB free for the Mac), clones the three files into the new
computer's folder, writes a new machine identifier (`VZMacMachineIdentifier`) and
uses the MAC address the computer was created with. The template's secrets are
not copied into the computer's folder. If the data folder's volume cannot clone
files (not APFS), the computer is installed instead.

The auxiliary storage carries over because it holds the NVRAM, and System
Integrity Protection's setting lives in NVRAM: a copy boots with it disabled, as
the template did. LCU, the ChatGPT app and the TCC rows are on the disk.

**Personalizing.** The copy still has the template's password, SSH key and host
keys, so an agent in one computer could log in to every other computer made from
the same template. Setup therefore ends with a step the template never needed.
Silo boots the copy without a display and logs in as `silo` with the template's
key from `template-access/`, then runs one script as root (`personalize.rs`):

- sets the account password to the copy's own, with `dscl . -passwd` reading it
  from standard input, and rewrites `/etc/kcpassword` for it (the same encoder as
  the offline setup); both arrive on the SSH command's standard input, never in
  an argument, the script or the environment;
- replaces `/etc/ssh/ssh_host_*` (`ssh-keygen -A`) and sets `ComputerName`,
  `LocalHostName` and `HostName` from the computer's name;
- removes the contents of `~/Library/Keychains`. The login keychain is locked
  with the old password, and macOS asks to unlock it at the next automatic login
  when the password changes. Deleting it in the copy, rather than leaving it out of
  the template, keeps the template an exact image of a finished computer, and
  macOS creates a fresh keychain, unlocked with the new password, at the next
  login;
- when the disk is larger than the template's, expands the APFS container with
  `diskutil apfs resizeContainer` (after `repairDisk`) on the container's
  physical store, found with `diskutil info`. Every attempt, including a resumed
  one, measures the bytes no partition covers and expands only while a GiB or more is
  unused; the record's disk size is set from that measurement. If the container does
  not fill the disk, the computer keeps the space it got, its recorded disk size says
  so, and it ends as Failed with that message but is complete and can be started. A
  requested disk smaller than the template that is selected when the copy starts
  fails with "This macOS template needs at least N GiB" instead of being enlarged;
- replaces `authorized_keys` with the copy's own public key, as its last change, so
  a run cut short can start again with the template's key. A run that already got
  that far is recognised by the copy's own key answering.

Silo then shuts the copy down with its own key, forgets the known hosts (the host
keys changed), boots it again and checks `csrutil status`, that LCU's receipt is
present, and that macOS logged in as `silo` on its own with the new password. The
state is `setting-up` until then; Retry setup resumes an interrupted
personalization, and Quit and Delete end it like any setup. While a computer
waits for its personalization, its template cannot be removed, and the computer
cannot be started (Retry setup or Delete only), because it still has the
template's password and keys. Secrets are only read by shell builtins in the guest
script. Copies in progress reserve their estimated writes, so two at once cannot
both pass the free-space check; a template a copy held is removed when that copy
ends, if a newer one exists.

**Status.** Implemented and unit-tested. Not yet run against a live guest: that a
copy boots with the new machine identifier and keeps SIP disabled, that `dscl`
reads the password from standard input over SSH, that the fresh login keychain
raises no dialog, and that the container expands (macOS keeps its Recovery
partition after the container on a framework-installed disk, which may stop it).

**Prior art.**

| Product | Mechanism | Verdict |
| --- | --- | --- |
| Cua `cua-vmm` (MIT, [commit ba4c636](https://github.com/trycua/cua/blob/ba4c6369660ab4a9c4d3d8af942bc53ad376615f/libs/cua/crates/cua-vmm/src/lume/mod.rs)) | A base VM per image, `clonefile` per create, free-space check before the clone, a guard that deletes an interrupted clone, bookkeeping of owned VMs, resources applied before the first boot | Structure reused: base, space check, clone, resources |
| Lume (MIT, `LumeController.clone`) | Clones the VM folder with `clonefile`, then a new MAC address and a new `VZMacMachineIdentifier`; grows a disk, never shrinks it | Reused: what a copy changes, grow-only disks. Lume also rewrites the GPT to grow a macOS disk; Silo expands the container in the guest instead |
| Tart (FSL, read for insight only) | `tart clone` is a clonefile of the VM folder with a new MAC address; `tart set --disk-size` only grows | Same model; no code used |
| Cua Spaces (FSL-1.1-MIT) | Hosted spaces built on cua-vmm | Not used: license |
| VMPal | Clones a VM and keeps a Base OS; Tools helper in the guest | Not reusable; proprietary |

**Why Silo cannot ship a pre-installed image.** Apple's macOS license lets a
Mac's owner run up to two macOS virtual machines on that Mac; it does not allow
redistributing macOS, and the restore image is downloaded from Apple for each
Mac. A downloadable pre-installed image would be a redistribution of macOS, so
Silo installs on the user's own Mac and shares copy-on-write blocks only between
that Mac's own computers.

## Checkpoints

A checkpoint is a copy of a computer at one moment, made and used with the commands
of the Linux checkpoint list (Create, Restore, Fork, Delete). Only computers whose
setup has finished have them; a copy still waiting for its personalization does not.

**Create.** On a stopped computer a checkpoint holds the disk and the auxiliary
storage (the NVRAM, which changes while the computer runs), both `clonefile` copies
that cost only what the computer later changes. On a running computer Silo also
saves its memory: it asks the framework whether this machine can be saved
(`validateSaveRestoreSupport`, asked when the machine was configured), pauses it,
saves its state with `saveMachineStateTo`, clones the disk and the auxiliary
storage while it is paused, and resumes it. The computer is paused for the save and
the clones, a few seconds. If anything fails the computer is resumed and the partial
checkpoint removed. If the framework says the memory can't be saved, a running
computer gets no checkpoint and the message says so and offers a checkpoint of the
stopped computer; Silo never silently takes a different kind. A checkpoint folder is
built under a partial name and renamed, and a folder without a readable
`checkpoint.json` or without its files is not listed and is swept on launch.
`checkpoint.json` records the Mac's build (`kern.osversion`) too.

**Restore.** Silo first saves a recovery checkpoint named "Before restore" (with
memory when the computer runs), then turns a running computer off, then replaces the
computer's disk and auxiliary storage with clones of the checkpoint's (both clones
are made before either file is replaced). The computer stays stopped and its record
remembers the Restore. A failure before the files are replaced changes nothing and
leaves the recovery checkpoint in the list. The next Start does the rest: for a
memory checkpoint it builds the machine from the computer's own configuration,
restores the saved state and resumes it, so the computer is where the checkpoint was
taken; for a disk checkpoint it boots from the restored disk. The state is not tried
when the Mac's build has changed since it was saved (the framework refuses states
after some host updates), when the state file is gone, or when the framework
rejects it; the computer then boots from the restored disk (which is exactly the disk
of the checkpoint) and the running computer's detail says why. The pending restore is
cleared by that Start whatever happens, so a later Start never uses an old state.
The machine's MAC address must be the one the state was saved with; Silo always
builds a computer with the MAC in its record.

**Fork.** A fork is a new, independent computer in the Computers list. It is made
from a checkpoint's disk and auxiliary storage (a memory state can't move to a new
MAC address and machine identifier, so forks start from the disk, as after a power
cut) and from the source's hardware model, with a new MAC address and machine
identifier. The fork's guest still has the source's password and keys, so it goes
through the same personalization as a copy of a template ([Personalizing](#templates)):
Silo logs in with the source's credentials, kept in `inherited-access/`, and sets the
fork's own password, SSH key, host keys, keychain and name. The fork shows as setting
up with the personalization step, takes about a minute, and counts as a macOS
computer for names (the shared name reservation) and for the two-running limit like
any other. The Fork command returns once the files are cloned; the fork can't be
started until its setup finishes.

**Delete.** Removes the checkpoint's folder (the record first). A checkpoint a
pending Restore will use can't be deleted. Deleting a computer deletes its
checkpoints with it; forks are independent files and are not affected.

**Lifecycle.** A computer runs one checkpoint operation at a time. While one runs
the row shows its step, the computer can't be started, stopped, force-stopped or
deleted, and a second operation is refused. Quit and an update count the operation
as work in progress: Quit asks it to end at its next step (a paused machine can't
answer a shutdown), waits for it, then stops the computer as usual. Operations are
refused after Quit has started. The Start of a computer that is changing state or
is being personalized is refused as before.

**Where they are.** macOS computers have no detail page, so the row has a
Checkpoints button that opens the list under it; it is the Linux
`CheckpointPanel` fed through a thin adapter (`macos-checkpoint-panel.tsx`), not a
second list. Setup logs record each operation (Logs page, per computer).

**Measured** (2026-10-10, Silo's configuration, an 8 GiB guest after 60 seconds of
use): pause 0.02 s, save 2.1 s into a 2.95 GB file, clones about 1 s, restore 3.5 s,
resume 0.4 s. See [the research](research/macos-checkpoints-2026-10-10.md).

**Status.** Implemented and unit-tested; the framework sequence was run live in a
probe (pause, save, stop, restore, resume). Not yet run through Silo's UI against a
live guest: memory restore on Start, the Restore of a running computer, and the
personalization of a fork.

**Prior art.** Lume (MIT) has no memory save; cua-vmm (MIT) models a checkpoint as a
stopped clone; Tart (FSL, insight only) has one suspend slot and keeps
`state.vzvmsave`; UTM (Apache-2.0) has a suspend slot, and snapshots with memory only
on macOS 27; VMPal (proprietary) has named snapshots with a cold boot when a saved
session can't be restored. Silo uses the framework sequence directly and falls back
to the disk with a notice, as VMPal does. See
[the comparison](research/macos-checkpoints-2026-10-10.md).

## Provisioning for computer use

Agents use macOS computers through LCU's macOS build, as they use Linux
computers through its Linux build. Nobody clicks Setup Assistant, a Recovery
prompt or an "Allow" dialog: Silo provisions the guest after installation.
State `setting-up` covers these steps; its detail names the current step.

1. **Account, automatic login and SSH, offline.** With the computer stopped,
   Silo attaches `disk.img` with `hdiutil`, mounts the guest's Data volume and
   writes the account `silo` (home `/Users/silo`, short enough for LCU's
   13-byte limit), its random password, `.AppleSetupDone`, the Setup Assistant
   keys, automatic login (`/etc/kcpassword`), Remote Login, and no sleep or
   screen lock. This ports the offline setup of
   [Lume](https://github.com/trycua/cua/tree/main/libs/lume) (MIT) rather than
   bundling its binary; see the [engine research](research/macos-guest-engine-2026-10-09.md#provisioning).
   Step 1 runs after a first boot that lets macOS write its launchd state:
   launchd rewrites `disabled.plist` at boot when it did not create it, which
   discards the Remote Login entry. The patch refuses to run until that file
   exists, and the first boot is repeated with a longer wait if it does not.
2. **Finishing the account.** Silo finds the guest's address in
   `/var/db/dhcpd_leases` by its MAC address and logs in once with the password
   (OpenSSH's `SSH_ASKPASS`, reading a 0600 file, so the password is never in
   an argument or environment variable). As root it installs the per-computer
   SSH key and `/etc/sudoers.d/silo`, fixes ownership, and runs
   `diskutil apfs updatePreboot` so Recovery knows the account. Files written
   offline belong to an unknown owner, so anything sudo or sshd checks is
   written from inside the guest instead.
3. **System Integrity Protection.** Silo boots the computer into macOS Recovery
   (`startUpFromMacOSRecovery`) in a visible "Setting up" window and types the
   `csrutil disable` sequence into its `VZVirtualMachineView`, then boots
   normally and confirms `csrutil status` over SSH. The key sequence follows
   cirruslabs' MIT image templates. Silo reads the screen by capturing its own
   window and recognising text with Vision (no Screen Recording permission is
   needed for a process's own window), answers only a password prompt that
   names `silo` on the Terminal's last line, and never types when the screen
   cannot be read. The window drops the user's keyboard and pointer input while
   it runs.
4. **Computer use.** Silo downloads two pinned archives once per device
   (`guest/macos/chatgpt-app-lock.json`: the official ChatGPT macOS app from
   OpenAI's Sparkle feed; `guest/macos/lcu-lock.json`: LCU's darwin build),
   checks their size and SHA-256, copies them with `silo-computer-use.zsh` into
   the running computer and runs `apply --approval ask|auto` over SSH, the mode
   coming from the `computerUseAutoApproval` setting as for Linux computers.
   The script (zsh, as the guest has no Python):
   - requires `csrutil status` to say disabled;
   - installs `/Applications/ChatGPT.app` unless the pinned version is already
     there: SHA-256, `codesign --verify --deep --strict`, bundle identifier
     `com.openai.codex` and team `2DC432GLL2` are checked before the app is moved
     in, owned by root, and its quarantine attribute is cleared;
   - runs LCU's installer (`scripts/install.sh --runtime-only --yes`) as `silo`;
   - writes Accessibility and Screen Recording rows for `com.openai.codex` and
     `com.openai.sky.CUAService` (the Computer Use helper inside the app) into
     the system TCC database. The `access` table's columns are read at run time
     and only existing ones are written; the csreq blob comes from
     `codesign -d -r-` and `csreq -b`. It reads every row back, pre-writes the
     Screen Recording reminder ledger of `replayd` (macOS 15 and later) and
     restarts both `tccd` daemons;
   - runs `lcu setup --agent all --allow-missing --session direct --yes
     --approval <mode>` and installs a LaunchAgent that runs
     `lcu setup --reconcile` at each login, so agents installed later register.
   Per-app approvals ("Allow Computer Use to use X?") stay with LCU and are never
   seeded. The script keeps a log (`~/Library/Logs/silo-computer-use.log`) and a
   receipt (`~/Library/Application Support/Silo/computer-use-receipt.json`) in the
   guest, and is idempotent: a rerun with the same pins and mode changes nothing.
   Pins: ChatGPT 26.930.61225 (CUA runtime 0.0.27, the runtime LCU's tested macOS
   pair uses; LCU lists its pairing with 26.928.20755, which the feed no longer
   serves) and LCU 0.10.1. The home folder `/Users/silo` is within the helper's
   13-byte limit.
   The approach follows prior art rather than inventing one:
   [trycua/cua `seed-tcc.sh`](https://github.com/trycua/cua/blob/main/libs/images/macos/files/seed-tcc.sh)
   and [actions/runner-images `configure-tccdb-macos.sh`](https://github.com/actions/runner-images/blob/main/images/macos/scripts/build/configure-tccdb-macos.sh)
   (both MIT) for the system-database rows, csreq derivation and the `replayd`
   ledger, and [electron's `screencapture-nag-remover.sh`](https://github.com/electron/electron/blob/main/script/actions/screencapture-nag-remover.sh)
   (MIT) for the ledger formats. `tccutil.py` (GPL-2.0) and MDM's PPPC profiles
   were not used: the first cannot be bundled under Silo's license, and Apple
   only accepts a PPPC profile for Accessibility and Screen Recording from an
   enrolled MDM server, which a guest does not have.
5. **Clipboard.** Nothing is installed in the guest and nothing syncs on its
   own. Two explicit actions, "Paste into computer" and "Copy from computer",
   move text (1 MiB limit) and PNG images (16 MiB) over SSH as the logged-in
   `silo` user, with the same orchestration, limits and messages as the Linux
   desktop viewer ([clipboard behaviour](SiloUI-DESKTOP.md#clipboard-behaviour)).
   Both directions use JavaScript for Automation on `NSPasteboard` with explicit
   types (`public.utf8-plain-text`, `public.png`), because `pbcopy` and
   `pbpaste` interpret RTF headers. Payloads travel on standard input and
   base64 output, never in the command line, and the guest checks sizes before
   sending. The commands follow Lume's `ClipboardWatcher.swift`
   (MIT, [commit ba4c636](https://github.com/trycua/cua/blob/ba4c6369660ab4a9c4d3d8af942bc53ad376615f/libs/lume/src/Clipboard/ClipboardWatcher.swift)).
   The actions are in the computer row's menu and in the screen window's
   toolbar; the toolbar shows the outcome in the window's subtitle. They need a
   running computer whose setup is complete (SSH and the automatic login
   session) and work on any supported guest, macOS 14 and later. "Paste into
   computer" sets the computer's clipboard; the user then pastes with
   Command+V in the guest. There are no keyboard shortcuts in the screen
   window: the guest owns Command+C, Command+V and their Shift variants
   (Paste and Match Style), so a shortcut that Silo took would break them.

Per-computer secrets live in `macos-computers/<id>/guest-access/` (mode 0700):
the account password and the SSH key. Anyone who can read that directory can
also read the computer's disk, so the Keychain would add no protection.

## Development and signing

Virtualization.framework requires the `com.apple.security.virtualization`
entitlement, which `Entitlements.plist` now carries. `npm run desktop`
(`tauri dev`) runs an unsigned binary without entitlements, so macOS computers
fail there; use `npm --prefix app/SiloUI run desktop:build:debug`, which signs
the Dev bundle ad hoc with the entitlements. Release signing verifies the
entitlement with the others.

The framework runs the computer inside the Silo process, on its main queue.
A crash of Silo therefore turns its macOS computers off abruptly.

## Not covered yet

| Silo feature | Status for macOS computers |
| --- | --- |
| Checkpoints | [Done](#checkpoints) for local computers. Not done: export and import of a checkpoint, a size breakdown (only a memory checkpoint's state file is counted), and abandoning a Restore |
| Remote computers (Connections) | None. The view must live in the process that runs the computer, so a computer on another device would need a streamed or VNC path, and Apple's license excludes service-style use |
| Agent computer use | Installed during setup (step 4). Not done yet: re-applying when the `computerUseAutoApproval` setting changes (a rerun of `apply --approval` does it), upgrading the pinned app or LCU in computers that already have them (a newer Silo's `apply` reinstalls the app and LCU, but nothing triggers it), status in the UI, and cancelling a download or copy in progress (only the steps between them notice a cancellation). macOS shows one "App Background Activity" banner for the reconcile LaunchAgent |
| Setup on other macOS versions | Verified with macOS 26.6.2 guests on a macOS 26.5 Mac. The offline account edit, the Recovery screens and the TCC schema are undocumented and may change with a release; macOS 14 and 15 guests are not qualified yet |
| Shared copies of one installation | [Templates](#templates): later computers are APFS clones of the first one's result and share its blocks until they change them. Not done: a template per macOS build kept side by side, and shrinking a template's disk |
| Terminal, editor, Files, network ports, GitHub, secrets, working account | None. These use the Linux guest bridge over SSH, which macOS computers do not have |
| Export, import and backup | None |
| Clipboard | Explicit text and image transfer (step 5); no continuous sync |
| Shared folders | None. The framework offers a VirtioFS share |
| Editing resources after creation | None. CPUs and memory are fixed at creation; disk size cannot change. A new computer's disk is at least its template's |
| Status panel, tray, notifications, start with Silo | None |
| Linux devices and Intel Macs | Not possible: the framework runs macOS guests only on Apple Silicon |

## Prior art for the clipboard

| Product | Mechanism | Verdict |
| --- | --- | --- |
| VMPal | Its own proprietary Tools helper in the guest | Not reusable |
| Cua Spaces | `cua-spacesd` guest daemon over gRPC, FSL-1.1-MIT | Not used |
| Lume | SSH transfer with `pbcopy`, `pbpaste` and `osascript`, MIT | Chosen: nothing to install, works on macOS 14 |
| tart-guest-agent | SPICE vdagent in the guest, FSL-1.1-Apache-2.0 | Rejected: the same license family as the excluded Tart |
| UTM guest tools | spice-vdagent, needs macOS 15 and an interactive approval | Not used |

## Verification

2026-10-09, Silo Dev built from this branch, driven through its UI with a
macOS 26.6.2 guest on an Apple Silicon Mac running macOS 26.5:

- Create from the Computers editor with Operating system macOS: the restore
  image download resumed from a partial file and finished at Apple's exact
  length; installation from the cached image took 3 to 4 minutes.
- Setup ran without input: first boot, offline account, SSH, Recovery with SIP
  turned off, ChatGPT app and LCU 0.10.1, about 9 minutes in total. Over SSH
  the guest reported `csrutil status` disabled, the app and LCU installed, and
  an LCU MCP call (`cua.getState()`) answered without a permission prompt.
- The screen window showed the guest below its toolbar, followed window
  resizing, passed pointer input, and closing it left the computer running.
- Paste into and Copy from computer moved text both ways.
- Stop shut the computer down over SSH in about 10 seconds; a shutdown chosen
  inside macOS also showed it as stopped; Delete removed its files.
- The Rust suite passed on Ubuntu 24.04 ARM64 with CI's packages.

Live runs found and fixed: a first boot stopped before launchd wrote its state
(no SSH), an image left attached across a failed detach, an installer still
holding the auxiliary storage lock, power-button Stop waiting on a dialog, the
toolbar covering the guest display, and a link check that rejected LCU's own
archive. Lume's whole-disk detection (`contains("s")`) has the same defect Silo
fixed and is worth reporting upstream.

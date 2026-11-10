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
  while that template is the one a new computer would use (current base setup,
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
| `macos-templates/<build>-<base version>-<computer use version>/` | The template: `disk.img`, `auxiliary-storage.img` and `hardware-model.bin` as APFS clones of the source computer, `template.json` (macOS version and build, the two versions, disk size, creation time, source computer), and `template-access/` (mode 0700), the source computer's password and SSH key |
| `macos-restore-images/<file>.ipsw` | The restore image of the macOS being installed (about 20 GB). A `.partial` file resumes an interrupted download. It is kept only until a template of that build is saved: Silo then deletes the image and its partial file (not while an installation reads it) and notes it in the computer's log. It is downloaded again only when a computer must be installed: no template exists, the base setup changed, or Apple offers a newer macOS build. Computer use updates never need it |

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
what the computer later changes) into `macos-templates/<build>-<base version>-<computer use version>/`,
writes `template.json`, and copies the computer's guest-access secrets to
`template-access/`. The folder is built under a partial name and renamed, so an
interrupted run leaves nothing a copy could use. Only the newest template is
kept: making one removes older ones, except any a copy is being made from. A
computer qualifies only if it came from an installation (not from a template),
finished all four setup steps, and was never started by the user afterwards
(`pristine` in `computer.json`, cleared by Start). Computers created before
templates existed never qualify. The one exception is a copy of a template whose
computer use is stale: once its setup has updated computer use, a fresh template
is saved from it (only when its disk is the template's size) and the stale one
is removed.

**Setup versions.** Two hashes, so that a new Cua Driver never means installing macOS again.
The base version covers the offline account setup and the first-boot finalization
(everything except computer use). The computer use version covers the pinned
Cua Driver lock, the guest script and registration helper, the recorder and the approval mode. The
computer's record keeps both for what it actually has installed (`setupVersion`,
`computerUseVersion`, and the approval mode its computer use step ran with), and
a template is named and made under those, not under the settings at the time of
the copy. A record without a computer use version (made before the split) counts
as stale. Checkpoints store the computer use version of the moment they were
taken, and a Restore or fork gives it back (an older checkpoint reads as stale).

- A template with a different base version, or of another macOS build, is never
  copied: the next computer is installed from scratch (the restore image is
  downloaded again if it was removed) and produces a new template.
- A template with the current base but a stale computer use version is still
  copied. The copy's setup then runs the computer use step ("Updating Computer
  Use"), and the result becomes the new template.
- A computer that already exists is updated the same way after it starts: when
  SSH answers and its computer use version differs, Silo runs the computer use
  script again in the background (row detail "Updating Computer Use", output in the
  computer's log). The script keeps what already is the pinned Cua Driver, replaces what
  is not, removes the ChatGPT app and LCU of a computer set up before Cua Driver (the
  app, LCU's files and LaunchAgent, their TCC rows and agent registrations), writes the same
  Accessibility and Screen Recording rows for Cua Driver and registers the agents with the
  current options. The update does not block the computer: Stop,
  Quit, Delete, a checkpoint operation or a new Start ends it, and the version is
  recorded only after it succeeds, while the computer is still the Start that began
  it. A failure is logged and retried at the next Start.

**Copying.** `create_macos_computer` asks the framework for the newest supported
macOS as before. A template of that build with the current base version is
copied; if the lookup fails (offline) the newest such template is copied;
otherwise the computer is installed. Silo checks free space (the copy writes a
few GB; it keeps 5 GB free for the Mac), clones the three files into the new
computer's folder, writes a new machine identifier (`VZMacMachineIdentifier`) and
uses the MAC address the computer was created with. The template's secrets are
not copied into the computer's folder. If the data folder's volume cannot clone
files (not APFS), the computer is installed instead.

The auxiliary storage carries over because it holds the NVRAM, and System
Integrity Protection's setting lives in NVRAM: a copy boots with it disabled, as
the template did. Cua Driver and the TCC rows are on the disk.

**Personalizing.** The copy still has the template's password, SSH key and host
keys, so an agent in one computer could log in to every other computer made from
the same template. Setup therefore ends with a step the template never needed.
Silo boots the copy without a display and logs in as `silo` with the template's
key from `template-access/`, then runs one script as root (`personalize.rs`):

- before the boot, the copy's disk is attached on the host and the account's password
  hash, automatic login (`/etc/kcpassword`) and login keychain are replaced offline
  (`offline_setup::run_template_rekey`, the same rewrite forks use), leaving the
  template's SSH key for the login below; `dscl . -passwd` is not used because it
  refuses to change the password of an account with a SecureToken without a terminal
  to ask for the old one. After the script the SecureToken is enabled again;
- replaces `/etc/ssh/ssh_host_*` (`ssh-keygen -A`) and sets `ComputerName`,
  `LocalHostName` and `HostName` from the computer's name;
- when the disk is larger than the template's, expands the APFS container with
  `diskutil apfs resizeContainer` (after `repairDisk`) on the container's
  physical store, found with `diskutil info`. Every attempt, including a resumed
  one, measures the bytes no partition covers and expands only while a GiB or more is
  unused; the record's disk size is set from that measurement. If the container does
  not fill the disk, the computer keeps the space it got, its recorded disk size says
  so. The message is shown as a note on the computer, not a failure: the rest of the setup
  runs and the computer is ready to start. A
  requested disk smaller than the template that is selected when the copy starts
  fails with "This macOS template needs at least N GiB" instead of being enlarged;
- replaces `authorized_keys` with the copy's own public key, as its last change, so
  a run cut short can start again with the template's key. A run that already got
  that far is recognised by the copy's own key answering.

Silo then shuts the copy down with its own key, forgets the known hosts (the host
keys changed), boots it again and checks `csrutil status`, that the computer-use receipt is
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
identifier. The fork's guest still has the source's password and keys, so it is
personalized before anyone uses it, with one boot of its own (below). The fork shows as
setting up, counts as a macOS computer for names (the shared name reservation) and for
the two-running limit like any other. The Fork command returns once the files are
cloned; the fork can't be started until its setup finishes.

Fork setup steps, measured on the Dev engine (n=1, 164.8 s to ready):

| Before | s | Now |
| --- | --- | --- |
| Personalizing: boot with the source's key, a script sets password, host keys, name, keychain | 50.7 | Offline, on the stopped disk (seconds): new password hash and automatic login, source key and login keychain removed |
| Shut down, check (boot, SIP, login), shut down | 27.4 | None |
| Start macOS (a third boot) | 7.9 | The one setup boot, with the new password: a script installs the fork's key, name and host keys; the automatic login and `sudo -n` are checked; SecureToken is given back |
| Installing developer tools again | 46.3 | Only if the checkpoint lacks them or has another version (the checkpoint's `developerTools` state is carried over) |
| Workspace folders, network and trust, final shutdown | 8.8 | The same, in that boot, then one shutdown |

A fork's setup never starts its guest on Apple's network (see [Network](#network)). It
rekeys the stopped disk and brings the guest's network bootstrap up to date in the same
mount: the program, its launch daemon and the configuration with the fork's own gateway
address and CA fingerprint, each replaced in place so the file keeps its owner; the record is
marked `firstBoot`. When the checkpoint's disk has nothing left to install (developer tools
current, computer use up to date), that is all: the fork's first Start is its only boot, and
it comes up behind its gateway. Before the computer is handed over, that Start logs in with
the new password, installs the fork's key, name and host keys, the network files and the
trust of the fork's proxy CA, and gives the account its SecureToken back
(`fork_first_boot.rs`). A fork waiting for that Start can't be checkpointed. If the
checkpoint needs developer tools or a computer-use update, the setup makes that same first
boot itself, behind the gateway, and goes on in the running guest; the Start is then a second
boot. A source disk that lacks the program, its launch daemon or the configuration is refused
before the fork's disk is changed (see [Network](#network)). The first Start saves its result
on the computer's current record (`firstBoot`, `inheritedAccess` and its account-token note
only), since other operations can change the record during a boot that takes minutes, and Stop
interrupts its waits for the guest. The source's copied password and key are deleted as soon as
the offline setup ends.
Retry setup leaves a fork that is waiting for its first Start to that Start: its key
does not exist before it.

**Quiet forks.** `checkpoints.fork` takes `secrets` and a `quiet` recipe, and a fork made
without a checkpoint name forks the computer's current state: it takes a checkpoint named
"Fork point" of the source first, under the same Fork operation, and copies that.

- The fork point has its own kind (`fork-point`) and a disk scope: it holds no memory file.
  A running source is flushed (`sync` in the guest) and then paused only while its disk is
  cloned. Retention never removes it, and it is taken after the fork's name and its secret
  and GitHub assignments were checked, so a fork that is refused leaves nothing behind (one
  that fails later takes the fork point back). It is the fork-time base a later three-way
  Keep needs, so it stays while a fork made from it exists: the fork's record names it
  (`forkedFrom`), Delete checkpoint refuses it, and deleting the last fork made from it
  removes it. A source that is busy at that moment keeps it as an ordinary checkpoint.
- Secrets and GitHub: a fork gets no Silo-managed secret or GitHub assignment. `secrets`
  names the ones to copy from the source; `github` is read-only GitHub access (the same
  copy as Linux, refused for a personal token). A name the source does not have, including
  `github` when the source has no GitHub assignment, fails the fork before anything is
  created.
- Scheduled work, on the stopped disk in the mount used for the rekey (`quiet.rs`), so none
  of it runs at the fork's boot. Only scheduled work is parked, as the recipe's "scheduled
  jobs off": LaunchAgents and LaunchDaemons whose property list has `StartInterval`,
  `StartCalendarInterval`, `WatchPaths`, `QueueDirectories` or `StartOnMount`, in `/Library`
  and each user's `Library`; user crontabs (`/var/at/tabs`) and at jobs; the user's periodic
  scripts (`/usr/local/etc/periodic/*`, `/etc/{daily,weekly,monthly}.local`, and scripts in
  `/etc/periodic/*` that macOS does not ship). Services that keep running or start at login
  (`KeepAlive`, `RunAtLoad`, such as a database from Homebrew) stay: they are the
  development stack the fork is meant to have, and `stop` names anything else. Silo's jobs
  (`com.silo.`, `org.silo.`) always stay. A `com.apple.` label is kept only when the file is
  in `/Library/LaunchAgents` or `/Library/LaunchDaemons` and its program is under `/System`
  or `/usr` (not `/usr/local`); the same label in a user's folder, or with another program,
  is treated like any other. A property list that can't be read is left alone, since
  launchd can't load it either. Parked files are moved, not deleted, to
  `/Library/Application Support/Silo/Quieted/` under their original path, and the fork's
  record lists them in `quiet.parked`.
- A user's `~/Library/LaunchAgents` that is a link is followed once when its target stays on
  the Data volume (the job files are parked from where the link points); a link that leaves
  it, or any other link on the way, fails the fork with a message saying so.
- Jobs that apps register themselves (`SMAppService` agents and daemons, in an app's
  `Contents/Library/LaunchAgents|LaunchDaemons`) are not parked. They are enabled through
  the Background Task Management database, which is not edited offline, and moving a file
  inside a signed bundle would invalidate its signature. The scheduled ones found in
  `/Applications` and each user's `Applications` are listed in `quiet.stillScheduled`, and
  `stop` can name their labels.
- `scrub` paths are deleted on the same mount. The rules are Linux's: absolute, at least two
  levels, no `..`, no walking through a link (a link at the end is removed itself). They
  must be on the Data volume (`/System`, `/bin` and most of `/usr` are not), `*` and `?`
  match within one path part, and `[` is refused. Every match is checked before anything is
  removed, and a match that equals, contains or lies below a protected path refuses the
  whole scrub: the account's home folder and the folders above it, Silo's files and launchd
  jobs, the account database, `/etc/ssh`, sudoers, automatic login and the `silo` user's
  `.ssh` (names are compared without regard to case).
- `stop` names (launchd labels or process names) run in the first Start, first, over the
  password login: `launchctl disable` and `bootout` in the system and the `gui/501` domains,
  then each process whose executable has exactly that name is ended (a name is compared whole,
  not as a pattern or as the kernel's 16-character process name). The step fails the Start
  when a label is still loaded or a process of that name is still running afterwards.
- What still runs before the quieting: the fork's login items and the apps reopened at the
  automatic login start before the first SSH connection, so before `stop` runs. A job that
  is not scheduled and is not named in `stop` keeps running, as does anything an app starts
  by itself. The new machine identifier is made when the files are cloned and the host and
  computer names are set in that Start.
- The record's `quiet` holds `scheduledJobsDisabled`, `newIdentity`, `stop`, `scrub`,
  `secrets`, `parked`, `stillScheduled` and `applied`, which is true once the Start has
  finished. The Start saves its result before the in-memory record says the first boot is
  over, so a failed save leaves the fork waiting for another Start.

Not yet run against a live guest.

The old script changed the password with `dscl . -passwd` over standard input; with no
terminal that command printed `New Password:` and `Permission denied`, and the script
did not check for it. The new password is now written to the account's record on the disk,
and the setup boot proves it took effect: it logs in with it and the automatic login
(which reads the same password) must reach the account.

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

**Where they are.** A macOS computer's page has a Checkpoints tab, and its row
keeps a Checkpoints button that opens the same list under it; it is the Linux
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

Agents use macOS computers through [Cua Driver](https://github.com/trycua/cua) 0.38.0
(MIT), as they use Linux computers through its Linux build; both register it as the MCP
server `cua-driver`. Nobody clicks Setup Assistant, a Recovery
prompt or an "Allow" dialog: Silo provisions the guest after installation.
State `setting-up` covers these steps; its detail names the current step.

1. **Account, automatic login and SSH, offline.** With the computer stopped,
   Silo attaches `disk.img` with `hdiutil`, mounts the guest's Data volume and
   writes the account `silo` (home `/Users/silo`, a short
   home folder), its random password, `.AppleSetupDone`, the Setup Assistant
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
4. **Computer use.** When the computer lacks the pinned app, Silo downloads the
   pinned universal Cua Driver archive once per device
   (`guest/macos/cua-driver-lock.json`: `cua-driver-rs-0.38.0-darwin-universal.tar.gz`
   from the `trycua/cua` GitHub release, about 85 MB), checks its size and SHA-256,
   copies it with `silo-computer-use.zsh`, `pinned.json` and the registration helper
   `silo-cua-register.py` into the running computer and runs
   `apply --approval ask|auto` over SSH, the mode coming from the
   `computerUseAutoApproval` setting as for Linux computers. Older files in the
   download cache (the ChatGPT app and LCU archives) are deleted. The script (zsh;
   the registration helper needs the Command Line Tools' Python 3, which the developer
   tools step installs earlier):
   - requires `csrutil status` to say disabled;
   - removes what a computer set up before Cua Driver has: `/Applications/ChatGPT.app`, LCU
     (`~/.local/share/lcu`), their TCC rows and ledger entries, and the registrations of
     `lcu` in Claude Code and Codex with LCU's lifecycle hooks (MCP tool hooks for the `lcu`
     server) and the trust Codex recorded for them;
   - switches telemetry and the update check off: both variables for every command of the
     script, `telemetry_enabled` and `update_check_enabled` set to false in
     `~/.cua-driver/config.json` (the daemon is started by LaunchServices without the
     script's environment and reads this file), `~/.zshenv`, and `launchctl setenv`;
   - installs `/Applications/CuaDriver.app` unless the pinned version is already
     there: SHA-256, `codesign --verify --deep --strict`, bundle identifier
     `com.trycua.driver` and team `YCK386LBJ7` are checked before the app is moved
     in, owned by root, and registered with Launch Services, then links
     `/usr/local/bin/cua-driver` to the executable inside the app. Cua Driver starts its
     daemon under the app's own identity only when it runs from inside the bundle;
   - writes Accessibility and Screen Recording rows for `com.trycua.driver`
     into the system TCC database. The `access` table's columns are read at run time
     and only existing ones are written; the csreq blob comes from
     `codesign -d -r-` and `csreq -b`. It reads every row back, pre-writes the
     Screen Recording consent ledger of `replayd` (macOS 15 and later; Cua Driver
     counts an entry keyed by the bundle identifier as the user's consent to direct
     capture) and restarts both `tccd` daemons;
   - registers the `cua-driver` MCP server (`/usr/local/bin/cua-driver mcp`, with the
     telemetry and update-check variables in its environment) with Claude Code
     (`~/.claude.json`) and Codex (`~/.codex/config.toml`); the approval mode adds or
     removes `mcp__cua-driver` in Claude Code's `permissions.allow` and
     `default_tools_approval_mode = "approve"` in Codex's section. Both files are written
     whether or not the agent is installed, so an agent installed into a running computer
     finds the server at its first start. When an installed agent's files cannot be
     updated the helper exits 1 and the setup fails (the old approval would otherwise stay
     in force while the receipt said ready); an absent agent whose files could not be
     written is pending, and a LaunchAgent (`org.silo.computer-use-reconcile`) runs
     `silo-cua-register.py reconcile` at each login to try again;
   - checks readiness: with a login session it starts the daemon the way `cua-driver mcp`
     does (`open -n -g /Applications/CuaDriver.app --args serve`) and requires
     `cua-driver permissions status --json` to report Accessibility and Screen Recording
     as granted, then requires `cua-driver doctor --json` to report `ok`. No LaunchAgent
     starts the daemon; the first agent session does.
   - turns on the guest's own Screen Sharing for [remote viewing](#on-another-mac):
     `launchctl enable system/com.apple.screensharing` and, unless it is already
     loaded, `bootstrap` of its launch daemon, plus Screen Recording, Accessibility
     and Post Event rows for `com.apple.screensharing.agent` and
     `com.apple.screensharing.daemon`, read back like the others. The legacy VNC
     password mode stays off: viewers sign in with the computer's account. The
     script is part of the computer-use fingerprint, so an existing computer gets
     this through the in-place update at its next start.
   Cua Driver's own per-app consent stays with its permission mode (standard,
   promptless) and is never seeded. The script keeps a log
   (`~/Library/Logs/silo-computer-use.log`) and a receipt
   (`~/Library/Application Support/Silo/computer-use-receipt.json`, schema 2: state,
   reason, `driverVersion`, approval, `agents`, `pending`) in the guest, and is
   idempotent: a rerun with the same pins and mode changes nothing. Pin: Cua Driver
   0.38.0 (see the [Cua Driver decision](SiloUI-COMPUTER-USE-PLAN.md#cua-driver-2026-11-03)).
   Verified live on 2026-11-03 (macOS 26.6.2 guest copied from a template set up with the
   ChatGPT app and LCU, standalone Dev engine): the TCC rows and the consent ledger are enough
   for the daemon to capture the screen without a prompt (`get_desktop_state` through the
   registered command returned the desktop), and `open` starts the daemon from the SSH
   session of a console login. `launchctl setenv` does not survive the restart at the end of
   setup; the daemon reads `~/.cua-driver/config.json` and the agents pass the variables
   themselves, so nothing depends on it.
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

## Network

A macOS computer reaches the network only through its own gateway process, which applies
Silo's network rules (the [threat model](THREAT-MODEL.md#2-network-egress-public-addresses-only) describes the
rules). Three boots do not, and they are limited to disks no agent can have written.

**The `agentMayHaveUsed` flag.** A computer's record has it set the first time its setup
completes, and never clears it. A fork and an import are born with it set, since their disk
came from a computer or an archive an agent may have used. A record saved before the field
existed gets it from its completed setup when it is loaded, and a record that is saved
without it, while its setup is complete or while the saved record has it, is written with it.

**What a boot gets** (`store::network_plan`, applied by `engine::start` and checked again where
the machine is built):

| Computer | Network |
| --- | --- |
| Setup complete | Its gateway |
| Flagged, setup not complete, network bootstrap on its disk (`proxyBootstrap`) | Its gateway |
| Flagged, setup not complete, no network bootstrap recorded | Refused |
| Not flagged: a first install, or a copy of a template that is still being set up | Apple's NAT |
| Recovery (turning off System Integrity Protection) | No network card |

Nothing else builds a machine with Apple's network: `Boot::BootstrapNat`, the legacy
migration's `Boot::ProxyBootstrapNat`, and the installer machine of a new disk. The first two
check the computer's record, the one in hand and the one saved, and refuse when either says
an agent may have used the disk or when the saved record cannot be read. Recovery has no network
card: it needs none, and a guest in Recovery cannot reach the Mac or the local network.

**Bringing a used disk behind its gateway.** A fork or an import is prepared while it is
stopped (`provision.rs`, `offline_setup.rs`). Silo allocates its gateway address and proxy CA,
saves its bootstrap identity, then mounts the Data volume and replaces three files in place
with the current ones: the program `/Library/PrivilegedHelperTools/silo-network-bootstrap`, its
launch daemon `/Library/LaunchDaemons/com.silo.network-bootstrap.plist` and its configuration
`/Library/Application Support/Silo/NetworkBootstrap/config.json`. They are replaced and not
created, because the host cannot make a file the guest sees as owned by root, which launchd and
the program require. The proxy CA is not written to the disk: the file already there names the
CA the first boot must remove from the keychain (`remove-inherited-proxy-ca.sh`), and the new CA
and its trust are installed in the first boot over the login that boot allows. Setup then
makes that boot itself when guest steps remain, or leaves it to the first Start, and the rest of
the setup runs over the gateway's loopback SSH port.

Steps that download in the host (the command-line tools, Git LFS, `gh`, Node.js and the Claude
Code installer file) copy the files to the guest over SSH, and work behind the gateway. The
one download the guest makes is the binary the Claude Code installer fetches from Anthropic's
release host over HTTPS; the gateway's default rules allow any public host and the guest
resolves names through it, so it works without a rule.

**When a used disk is refused.** The boot or the setup stops with a message, and nothing on the
disk has been changed, when:

- the disk lacks any of the three files above (it was made before the network gateway existed, or
  not by Silo: an old checkpoint, or an archive exported from a computer that was never set up
  with the gateway), or one of them is a link or is not a regular file;
- the build has no signed `silo-network-bootstrap` program;
- the computer's saved bootstrap identity differs from its Keychain CA or its gateway address;
- the computer is flagged but has no network bootstrap recorded (`proxyBootstrap` is false and
  it is not waiting to be personalized): setup cannot repair it.

The message says what the person can do: delete the computer and make it again from a computer
or checkpoint that this version of Silo has set up, or import an export of one.

**What is not covered.** The launch migration of a computer made before the gateway existed
still converts it once over Apple's NAT, because such a record is not flagged: its setup was
never complete. A first install and the setup of a copy of a template still boot on Apple's
NAT until their setup completes, and the template's contents and fingerprint did not change.

## On another Mac

A macOS computer can run on another Mac that is connected through
[Connections](SiloUI-CONNECTIONS.md) and be managed and viewed from this Silo, which
may run on macOS or Linux. The Mac that hosts the virtual machine is the owner; the
device that shows it is the controller. The owner keeps the virtual machine in its own
Silo process exactly as for a local computer; the controller only calls the bridge.

- **List.** The owner's macOS computers appear in the Computers list with the device
  badge, as remote Linux computers do. The controller polls the owner's
  `macos.snapshot` while the list is on screen. A device that cannot host macOS
  computers (a Linux device, an Intel Mac) reports that and contributes no rows.
- **Create.** New computer with the operating system macOS offers Run on: This device,
  when it can host, and each connected Mac that reports it can. The form's limits (names,
  the smallest disk, the template notice) follow the chosen device, and Remove template
  stays local. The owner downloads and installs macOS and sets the computer up, so the
  work needs the owner's network and disk. The two-computers-at-once limit is the
  owner's: a third Start fails with the same message as locally.
- **Start, stop, force stop, delete, retry setup.** These run on the owner through
  `macos.action`, with the same admission and attempt guards as the local commands. A
  computer started this way opens no window on the owner. The one exception is setup:
  its Recovery step still shows a "Setting up" window on the owning Mac, because it
  reads that window to type the System Integrity Protection sequence.
- **Show screen.** Opens a Silo window that runs a noVNC viewer. Nothing is installed on
  the controller or in the guest beyond the Screen Sharing setup above.
- **Logs.** The Logs page reads a remote computer's retained setup logs through
  `runtime.logs`, which the owner answers for macOS computers too.
- **Errors.** The owner reports in plain words that it is not a Mac or cannot host
  macOS computers, that two macOS computers already run, that the computer is not
  running, that setup has not finished, and that Screen Sharing is not available yet (a
  computer set up before Screen Sharing existed is updated in the background shortly
  after it starts).
- **Not covered.** Checkpoints and clipboard transfer are not offered for remote
  computers: their rows hide those actions.

**Display path.** The owner serves three things for a viewer. `macos.display.connect`
returns the guest account's name and password, once the computer is running and its
setup has finished, and checks that Screen Sharing answers. `macos.display.stream`
connects to port 5900 of the guest through the computer's gateway, on a loopback port that
only the owner can reach, and copies bytes both ways; it is a stream method, like `guest.ssh`. `macos.display.resize`
reconfigures the first `VZMacGraphicsDisplay` (`reconfigureWithSizeInPixels:`, public
API) to the viewer window's pixel size, clamped to 640x400 .. 7680x4320 with even
sides. The controller opens the viewer window (`desktop-shell-macos-<hash>`) and
starts a loopback WebSocket endpoint for it (`macos_display_gateway.rs`): bound to
127.0.0.1 only, with a random per-window token as the URL path, a `Host` check against
the endpoint, an `Origin` check against the app's own web origins, one connection at a
time and a size limit on messages from the page. Each WebSocket is spliced to a new
`macos.display.stream` on the owner through the ssh bridge. noVNC speaks RFB with
Apple's security type 30 (the DH variant) using the credentials from
`macos.display.connect`; the page receives them in the response to its own request,
they are not logged and not part of the window's URL, title or any event, and the
window's commands refuse any window that was not opened for that computer. The page
resizes the guest after the window is resized (debounced, with `devicePixelRatio`) and
scales the picture to the window. It shows Connecting, Connected, Reconnecting
(automatic for lost connections, bounded), and Disconnected with a Reconnect button.

**Why Screen Sharing and noVNC.** The probe in
[the display research](research/macos-remote-display-2026-10-10.md#probe-and-decision-2026-10-10)
found that the guest's Screen Sharing starts without prompts, offers type 30 with the
computer's account, honours ZRLE (an idle Retina frame is 53 KB), and shows the real
desktop once the TCC rows exist. Its port is reachable from the owner only, unlike the
private `_VZVNCServer`, which listens on every interface of the owner. noVNC 1.7.0
(MPL-2.0, bundled unmodified, see `THIRD-PARTY-NOTICES.md`) is the maintained browser
client for RFB; the WebSocket endpoint uses `tungstenite` (MIT or Apache-2.0) rather than
a hand-written protocol. The Content Security Policy allows `ws://127.0.0.1:*` for this.

**Status.** Implemented and unit-tested at the pieces: method validation and
dispatch, the byte splice, the WebSocket checks and splice, resize clamping, the viewer's
states with a mocked noVNC, the list, the form and the guest script's grant list. Not run
against two real Macs, so these are unverified: that a computer set up before this
change picks up Screen Sharing through the in-place update, noVNC against Apple's server
(type 30 negotiation, keyboard layouts such as AZERTY, cursor), resizing the guest
display from a remote window, and bandwidth over a real SSH link.

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
| Checkpoints | [Done](#checkpoints) for local computers. Not done: importing a checkpoint into an existing computer (an exported checkpoint imports as a new computer), a size breakdown (only a memory checkpoint's state file is counted), and abandoning a Restore |
| Remote computers (Connections) | [Done](#on-another-mac): create, list, start, stop, force stop, delete, setup progress and logs, and Show screen for a computer on a connected Mac. Not done: checkpoints and clipboard transfer for remote computers, and screens before computer use is set up (the installer, Setup Assistant and Recovery); the Recovery step of a remote setup shows its window on the owning Mac |
| Agent computer use | Installed during setup (step 4). Not done yet: re-applying when the `computerUseAutoApproval` setting changes (a rerun of `apply --approval` does it), status in the UI, and cancelling a download or copy in progress (only the steps between them notice a cancellation). macOS shows one "App Background Activity" banner for the reconcile LaunchAgent. Computers set up with the ChatGPT app and LCU switch to Cua Driver through the in-place update at their next start |
| Setup on other macOS versions | Verified with macOS 26.6.2 guests on a macOS 26.5 Mac. The offline account edit, the Recovery screens and the TCC schema are undocumented and may change with a release; macOS 14 and 15 guests are not qualified yet |
| Shared copies of one installation | [Templates](#templates): later computers are APFS clones of the first one's result and share its blocks until they change them. Not done: a template per macOS build kept side by side, and shrinking a template's disk |
| Terminal, editor, Files, network ports, GitHub, secrets, working account | Done, for local computers: the `silo` working account with passwordless sudo, SSH through a per-computer alias for the terminal, editor (`/Users/silo/workspace`) and Files, published guest ports, and GitHub tokens and secrets applied through the per-computer network gateway. The app lists a macOS computer beside the Linux ones: it opens the same page (terminal, editor, Show screen, Start, Stop and Force stop, Edit CPU & memory while stopped, Delete; Overview with resources, computer use, repositories, secrets and ports; Checkpoints; setup progress in place of the tabs while it is created or set up) and appears on the Files, Logs, Network, Secrets and GitHub pages. It has no Restart, no SSH access for other devices and no Storage tab, because the engine serves those for Linux computers only. A computer on another device keeps its row, Show screen, Edit CPU & memory and its Logs |
| Export, import and backup | Done for local computers: export of a computer from its row menu or of one checkpoint from its checkpoint list, import as a new computer (with new guest keys), and cleanup after an interrupted export. Not done: export of a remote computer, and importing a checkpoint into an existing computer |
| Clipboard | Explicit text and image transfer (step 5); no continuous sync |
| Shared folders | None. The framework offers a VirtioFS share |
| Editing resources after creation | CPUs and memory: Edit CPU & memory while the computer is stopped, for local and remote computers. Disk size cannot change. A new computer's disk is at least its template's |
| Status panel, tray, notifications, start with Silo | Start with Silo is done for local computers. Status panel, tray and notifications: none |
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

Evidence in the next two sections is from the ChatGPT app and LCU era, before the move to Cua Driver; the Cua Driver setup has not been run live yet.

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

2026-10-10, Silo Dev from main, same Mac:

- Checkpoints: a checkpoint of a running computer saved its memory; after a
  change inside the guest, Restore and Start brought back the deleted file and
  a process killed after the checkpoint, with uptime continuing. Fork made a
  new computer from the checkpoint's disk with its own hostname, MAC address
  and key; the original computer's key was refused.
- An existing computer on LCU 0.10.1 updated itself to 0.11.0 after Start
  (about 70 seconds), with cross-turn Computer Use on.
- A new computer installed from the cached image in under 7 minutes; Silo
  saved a template and deleted the restore image.
- After the in-place update, an existing computer's Screen Sharing answered a
  type 30 login from the host with the real desktop and accepted keyboard
  input.
- Not run live: a computer on another Mac, controlled from a second device
  (Connections needs Remote Login on the owning Mac), the noVNC viewer, and
  resizing a remote screen. Unit and integration tests cover them.

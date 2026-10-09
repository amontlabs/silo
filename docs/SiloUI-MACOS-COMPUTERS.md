# macOS computers

Silo runs macOS computers on Apple Silicon Macs through Apple's
Virtualization.framework, next to the Linux computers it runs through
MicroSandbox. libkrun cannot boot macOS, so this is a second engine. The
engine choice, framework limits and Apple's license terms are in
[macOS guest engine research](research/macos-guest-engine-2026-10-09.md).

This is a first vertical slice: create, set up for computer use, start, view,
stop and delete. Most other Linux computer features do not apply yet; see
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
  creation the same action cancels it.
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
| `macos-restore-images/<file>.ipsw` | The last downloaded restore image, kept so the next computer does not download it again; a `.partial` file resumes an interrupted download |

macOS computers are not entries in the MicroSandbox registry
(`computers.json`), so the Linux lifecycle, health, backup and Connections code
never sees them.

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
| Checkpoints | None. Framework save/restore state (macOS 14+) is tied to this Mac and needs a paused computer and a configuration that passes `validateSaveRestoreSupportWithError`; disk clones (APFS `clonefile`) are the likely checkpoint route. Not designed yet |
| Remote computers (Connections) | None. The view must live in the process that runs the computer, so a computer on another device would need a streamed or VNC path, and Apple's license excludes service-style use |
| Agent computer use | Installed during setup (step 4). Not done yet: re-applying when the `computerUseAutoApproval` setting changes (a rerun of `apply --approval` does it), upgrading the pinned app or LCU in computers that already have them (a newer Silo's `apply` reinstalls the app and LCU, but nothing triggers it), status in the UI, and cancelling a download or copy in progress (only the steps between them notice a cancellation). macOS shows one "App Background Activity" banner for the reconcile LaunchAgent |
| Setup on other macOS versions | Verified with macOS 26.6.2 guests on a macOS 26.5 Mac. The offline account edit, the Recovery screens and the TCC schema are undocumented and may change with a release; macOS 14 and 15 guests are not qualified yet |
| Shared copies of one installation | None. Each computer installs its own macOS (about 21 GB); APFS clones of a set-up template would share blocks |
| Terminal, editor, Files, network ports, GitHub, secrets, working account | None. These use the Linux guest bridge over SSH, which macOS computers do not have |
| Export, import and backup | None |
| Clipboard | Explicit text and image transfer (step 5); no continuous sync |
| Shared folders | None. The framework offers a VirtioFS share |
| Editing resources after creation | None. CPUs and memory are fixed at creation; disk size cannot change |
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

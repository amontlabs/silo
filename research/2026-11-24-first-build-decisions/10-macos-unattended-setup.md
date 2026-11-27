# A new macOS computer with nobody clicking

Research for polarzero, 24 November 2026, for the first build of Silo. The question: Silo has
installed macOS from Apple's restore image into a new virtual machine on your Mac. How does that
computer get from its first start to a user account that Silo can reach over SSH, without anyone
clicking through Setup Assistant? The first-build proposal names this as a risk that is not in the
plan. This page changes nothing in `plan/`.

Each source is marked **read** (I opened the page or the source code and read the part cited) or
**search** (I saw it only in a search summary, so a detail may be wrong). Source code was cloned on
24 November 2026 and read here: cirruslabs/macos-image-templates, cirruslabs/tart,
cirruslabs/packer-plugin-tart, trycua/cua (Lume), tmc/vz-macos (cove). Nothing was run on a Mac:
this container is Linux, so every "it works" below is what the source or its authors say, and the
tests at the end are what would make it a measurement.

## In short

1. **macOS 27 solved most of this, through a public API.** Apple added
   `VZMacGuestProvisioningOptions` to the Virtualization framework (WWDC 2026, session 224). Silo
   passes a full name, a user name, a password, "log in automatically" and "turn on Remote Login",
   and on the first start Setup Assistant creates that user and turns SSH on by itself. No screen
   is read and no key is typed. It needs **macOS 27 on both your Mac and the computer**. Tart,
   VirtualBuddy and Apple's own sample code use it. Tart's macOS 27 image template dropped 82 lines
   of keystrokes for it.
2. **On macOS 26 and older there is no API, and every working method is a workaround that breaks
   with Apple's releases.** Typing keystrokes over VNC (Tart's Packer templates) needed eight
   fixes to its Tahoe script in 14 months, one for almost every point release. Writing the user and
   the "setup done" files onto the disk before the first start (Lume, cove) avoids the screens but
   depends on undocumented files, is verified by Lume on Tahoe only, and Lume still finishes it over
   SSH.
3. **The plan says main's user has your user name (`moving-in-on-macos`). Apple's provisioning
   reportedly refuses exactly that.** A developer reported in June 2026 that provisioning fails,
   and Setup Assistant appears, when the computer's user name is the same as the Mac's login name
   (FB23241619). The framework has an error for it, `guestProvisioningInvalidUsername`. Nobody has
   reported it fixed. There is a workaround that keeps your user name, and it is the first thing
   to test.
4. **Recommendation:** require macOS 27 on the Mac for the first build, use Apple's provisioning
   and nothing else, keep the installed and never started disk so a failed first start is retried
   without installing again, and make the user name, the SSH key and the remaining settings over
   SSH right after. That needs a new decision for polarzero (which macOS the Mac must run), a
   new decision agents can take (how a new macOS computer is set up), and small edits to four
   entries. Details at the end.

## What a new macOS computer looks like before anyone touches it

`VZMacOSInstaller` writes macOS from the restore image onto the computer's disk. The disk then
holds an APFS container with a sealed System volume, a small Data volume and the Preboot, Recovery
and VM volumes. On its first start macOS runs Setup Assistant: language, country, data transfer,
accessibility, privacy, the user account, Apple Account, terms, age range (new in 26.4), location,
time zone, analytics, Screen Time, Siri, FileVault, look, updates. Remote Login (SSH) is off. Nothing
is reachable until a person clicks through it or something stands in for that person.

- **What can be written from outside.** The System volume is sealed (Signed System Volume): a
  change to it breaks the seal and the computer does not boot from it, so nothing is written
  there. The Data volume is an ordinary APFS volume. It holds everything Setup Assistant writes:
  the local user database in `/private/var/db/dslocal/nodes/Default`, the marker
  `/private/var/db/.AppleSetupDone`, `/Library/Preferences`, `/private/etc`, and launchd's
  enabled and disabled services in `/private/var/db/com.apple.xpc.launchd/disabled.plist`. Lume's
  and cove's source both mount that volume from the stopped disk image with `hdiutil` and
  `diskutil` and write there (**read**: `MacOSOfflineSetupPatcher.swift`, `provision_mount.go`).
  Lume starts the computer once and stops it before writing, "so macOS can write its first-boot
  state" (**read**, [Lume docs](https://cua.ai/docs/concepts/how-lume-unattended-setup-works) and
  `UnattendedInstaller.swift`).
- **System Integrity Protection** protects the running system's files from the running system. It
  does not stop the Mac from writing onto a stopped computer's Data volume. It does stop the
  computer itself from editing its own privacy permissions (TCC), which is why Tart's base images
  first turn SIP off in macOS Recovery by keystrokes, then edit `TCC.db` (**read**:
  `templates/disable-sip.pkr.hcl`, `scripts/update-tcc-database.sh`). macOS 27 adds: "Apps can no
  longer access the local TCC database directly" (**read**, macOS 27 release notes, TCC section).

## Every approach, and how it holds up today

### 1. Apple's guest provisioning (macOS 27 host and guest)

**How it works.** (**read**: [Apple's class reference](https://developer.apple.com/documentation/virtualization/vzmacguestprovisioningoptions.md),
[WWDC 2026 session 224](https://developer.apple.com/videos/play/wwdc2026/224/), Tart's
`Sources/tart/Commands/Run.swift` and `VM.swift`.)

```swift
let options = VZMacGuestProvisioningOptions()
options.fullName = "Ana Lopez"
options.username = "ana"
options.password = password          // generated by Silo, kept in your Mac's Keychain
options.logsInAutomatically = true
options.enablesRemoteLogin = true
try options.validate()

let start = VZMacOSVirtualMachineStartOptions()
try start.setGuestProvisioning(options)
try await virtualMachine.start(options: start)
```

Apple's reference says: "This configuration requires guest macOS 27 or later to function
properly. Earlier versions of macOS don't support the automated guest configuration protocol and
ignore these settings." And: "macOS only evaluates these options on the first boot after restore."
The session says the options are passed to Setup Assistant, which creates the user and turns on
auto-login and SSH. Tart's `--provisioning-opts` refuses to run on a host older than macOS 27
(merged 9 June 2026, [PR #1263](https://github.com/cirruslabs/tart/pull/1263), **read**).

**Does it work.** Yes, by several independent users:

- Tart's `vanilla-golden-gate.pkr.hcl` (**read**, commit `16e9ad7`, 13 August 2026) replaced the
  whole Setup Assistant keystroke script with five provisioning options and a two-minute wait.
  What still types keys there is Tart's own extras: turning Gatekeeper off and Screen Sharing on.
  Silo needs neither. The template checks afterwards that FileVault is off
  (`fdesetup status | grep -q 'FileVault is Off'`), so a provisioned computer has FileVault off,
  which `file-changes-from-outside` needs.
- VirtualBuddy 2.2: "You can now skip Setup Assistant by configuring a macOS account before
  starting a new virtual machine for the first time ... requires macOS 27 or later on both your Mac
  and the macOS virtual machine" (**search**, changelog via
  [newreleases.io](https://newreleases.io/project/github/insidegui/VirtualBuddy/release/2.2)).
- Bitrise calls it "the closest macOS has come to cloud-init" (**read**,
  [Bitrise](https://bitrise.io/blog/post/wwdc26-the-virtualization-framework-updates-that-matter-for-large-mac-fleets)).

**What is known to break.**

- **Same user name as the Mac's.** Provisioning "was failed due to user name 'user' is not
  valid" when the guest user name matched the host's login name. With a different name it worked.
  FB23241619, June 2026; Apple DTS called it "weird" and asked for the report. No fix is reported
  (**read**, [forum thread 834397](https://developer.apple.com/forums/thread/834397); the reader
  comment on [Eclectic Light](https://eclecticlight.co/2026/06/12/macos-virtualisation-is-leaping-forward-in-golden-gate/)
  reports the same, **read**). The framework has an error code for it,
  `VZError.Code.guestProvisioningInvalidUsername` (**read**,
  [reference](https://developer.apple.com/documentation/virtualization/vzerror/guestprovisioninginvalidusername.md)).
  Whether it is a rule or a bug is not documented. It collides with `moving-in-on-macos`.
- **A provisioned user was missing from `CSIdentityQuery`** on macOS 27 betas (FB23716201),
  reported fixed in beta 5 by the reporter (**read**,
  [thread 839343](https://developer.apple.com/forums/thread/839343)).
- **`try?` hides the failure.** When provisioning is refused, the computer simply starts into Setup
  Assistant. Silo must not swallow the error from `setGuestProvisioning`, and must treat "SSH does
  not answer in time" as a failure (thread 834397).
- **First start only.** If that first start is cut off (the Mac sleeps, Silo is quit), the options
  may not be honoured again. Not documented either way; see test 5.
- **macOS 27 guests on macOS 27 hosts had other first-release bugs:** no push notifications in the
  guest, so MDM enrollment and iCloud failed, until 27.3 beta (**read**,
  [thread 840500](https://developer.apple.com/forums/thread/840500?answerId=903697022)). It does
  not touch SSH or the user, but it is one more sign that 27.0 to 27.2 guests are young.

**What breaks when Apple changes a screen.** Nothing that Silo wrote. Setup Assistant is Apple's,
and it reads the options itself. A new screen in macOS 28 is Apple's to skip. The risk moves to
Apple dropping or changing the API, which a public, documented class makes unlikely between two
releases.

**What it does not do.** It does not set an SSH key, sudo without a password, sleep, the screen
lock, "Allow full disk access for remote users", or any privacy permission. Those come after, over
SSH (below).

### 2. Typing through Setup Assistant (Tart's Packer templates, Anka, Lume before its offline mode)

**How it works.** Packer's Tart plugin connects to the computer's screen over VNC (Tart uses the
framework's private `_VZVNCServer`) and types a script of keys with fixed waits:
`"<wait60s><spacebar>"`, `"<wait30s>italiano<esc>english<enter>"`, then tabs and spaces per
screen. Since late 2025 it can also click a piece of text found on the screen by Apple's Vision
text recognition, `<click 'Select Your Country or Region'>` (**read**:
`templates/vanilla-tahoe.pkr.hcl`; `packer-plugin-tart/builder/tart/vnc.go` links `Vision`).
After the desktop appears it opens System Settings by keystrokes to turn Remote Login on.

**Does it work.** Yes, Cirrus Labs builds public images with it every release. The cost is the
upkeep. Commits that changed the Tahoe script (**read**, `git log -- templates/vanilla-tahoe.pkr.hcl`):

| Date | What changed |
|---|---|
| 16 Jun 2025 | Longer waits before "Select Your Country or Region" |
| 29 Aug 2025 | Time zone screen changed; FileVault screens added (beta 8) |
| 25 Sep 2025 | Spotlight unreliable; System Settings opened from Terminal |
| 13 Nov 2025 | 26.1: FileVault screen "no longer appears consistently"; removed |
| 5 Dec 2025 | Gatekeeper step fixed |
| 16 Dec 2025 | FileVault removal reverted; country screen now clicked by text recognition |
| 5 Apr 2026 | 26.4: new "Age Range" screen, Apple Account screen changed, password asked for Screen Sharing |
| 7 Aug 2026 | 26.6: automation permissions updated |

The script also chooses English by a trick that only works when the first screen lists English
first, and every wait is a guess about how fast the Mac is. Packer stops at the first key that
lands on the wrong screen, and a person reads the screenshot to fix it. Anka reaches the same
point inside its closed `anka create` (**search**: no public description found of how; a Veertu
user reports Anka's VM creation needs a logged-in host session,
[thread 822670](https://developer.apple.com/forums/thread/822670), **read**). MacStadium's Orka
avoids it by publishing ready base images (`ghcr.io/macstadium/orka-images/tahoe:latest`,
**search**, [Orka docs](https://docs.macstadium.com/orka/orka-engine/orka-engine-30.md)). UTM and
macosvm leave Setup Assistant to the person (**search**).

**What breaks when Apple changes a screen.** The script, at the first changed screen, on every
release that adds, removes or reorders one. For Silo it would also break on a Mac whose speed or
language differs from the one the script was timed on, which is every user's Mac. A release of
Silo would have to wait for a fix after each macOS release.

### 3. Writing the user and the "setup done" files onto the disk before it starts (Lume, cove)

**How it works.** (**read**: Lume `libs/lume/src/Unattended/MacOSOfflineSetupPatcher.swift` and
`UnattendedInstaller.swift`; cove `provision.go`, `templates/vz-provision.sh.tmpl`,
`docs/guides/provisioning.md`.) Lume, today:

1. Installs macOS, starts the computer once until it has an address, stops it.
2. Mounts the Data volume on the Mac and writes:
   - a user record `dslocal/nodes/Default/users/lume.plist` with `ShadowHashData` (a
     SALTED-SHA512-PBKDF2 hash Lume computes) and the `admin` group membership;
   - `/private/var/db/.AppleSetupDone`, and `DidSeeCloudSetup`, `DidSeePrivacy`,
     `DidSeeSiriSetup`, `DidSeeTouchIDSetup`, `DidSeeTrueToneSetup` in both the system's and the
     user's `com.apple.SetupAssistant.plist`;
   - `autoLoginUser` in `com.apple.loginwindow.plist`, with any `AccountInfo.FirstLogins` entry
     removed ("causes macOS to launch Setup Assistant on the next graphical login, even when
     .AppleSetupDone is present"), and `/etc/kcpassword`;
   - `com.openssh.sshd = false` in launchd's `disabled.plist`, which turns SSH on.
3. Starts the computer, waits for SSH, and finishes over SSH with `sudo`: sets the marker's owner
   to root, writes the same preferences again with `defaults`, runs
   `diskutil apfs updatePreboot /` so macOS Recovery knows the user, and runs
   `launchctl enable system/com.openssh.sshd`.

cove does the same differently: it writes a launchd daemon onto the disk that creates the user
with `sysadminctl` on the first start, and needs `sudo` on the Mac, because "launchd silently
ignores LaunchDaemon plists not owned by root:wheel". Its script turns SSH on with
`systemsetup -setremotelogin on ... || true`, which on recent macOS asks for Full Disk Access and
may fail silently there.

**Does it work.** Lume says Tahoe is "the verified preset" and that "Sequoia can still show an
Accessibility step on first display boot" (issue #2155) (**read**, Lume docs). cove's last commit
is April 2026 and names no verified release. Neither covers macOS 27.

**What it costs and what breaks.**

- It rests on files Apple does not document: the dslocal format, the `DidSee*` keys, `FirstLogins`,
  `kcpassword`. Apple's deployment guide already changed one of them: on macOS 14 and later,
  removing `.AppleSetupDone` no longer relaunches Setup Assistant if a local user exists
  (**search**, Apple Platform Deployment). A release that adds a first-login screen with a new key
  shows that screen, and nothing tells Silo except SSH not answering.
- `kcpassword` is easy to get subtly wrong. Lume's comment records that padding after the XOR
  instead of before still logged the user in, but the login keychain could not be unlocked: macOS
  moved it aside and made a new one with a password nobody knows, on every start, and any secret
  stored there raised a prompt nobody could answer. That is the keychain Claude Code keeps its
  login in (below).
- A user written as a plist has no secure token, so it is not a volume owner and macOS Recovery
  does not accept it. Both tools work around this (`updatePreboot`, or cove's two-user bootstrap).
  Whether software updates install for such a user on Apple silicon is not shown by either.
- It mounts the computer's filesystem in your Mac's kernel. On a disk nothing has run on yet that
  is not the risk `file-changes-from-outside` worries about. Whether macOS asks you to let Silo
  reach a mounted volume (the "Removable Volumes" permission) when the engine does it from a
  background program is not known.

### 4. Apple's device management (Automated Device Enrollment, MDM)

Automated Device Enrollment can skip every Setup Assistant screen, but only for a Mac whose serial
number your organization has in Apple Business Manager, enrolled with your MDM server (**read**,
[Apple Platform Deployment](https://support.apple.com/guide/deployment/depdeff4a547)). A
virtual Mac's serial number is generated by the framework and cannot be set to one in Apple
Business Manager on Apple silicon (**search**:
[khronokernel](https://khronokernel.com/macos/2023/08/18/AS-VM-SERIAL.html), Jamf community). It
would also make every Silo user run an MDM server. It does not apply.

### 5. Turning SSH on, the ways compared

| Way | Needs | Works on |
|---|---|---|
| `enablesRemoteLogin = true` in provisioning | macOS 27 host and guest | 27 |
| `com.openssh.sshd = false` in `disabled.plist`, written from outside | the Data volume mounted | Tahoe, per Lume |
| `sudo launchctl enable system/com.openssh.sshd` then start it | a root shell inside | the DISA STIG for macOS 14 uses `enable` (**search**) |
| `sudo systemsetup -setremotelogin on` | Full Disk Access for the program calling it | fails without it since macOS 13 (**search**) |
| System Settings, Sharing, Remote Login | a person or keystrokes | all |

"Allow full disk access for remote users" is a separate switch under Remote Login (**read**,
[Apple support, macOS 26](https://support.apple.com/en-bn/guide/mac-help/allow-a-remote-computer-to-access-your-mac-mchlp1066/26/mac/26)).
Without it, a command run over SSH is refused in the folders macOS protects, such as `~/Desktop`,
`~/Documents` and `~/Downloads`. Whether provisioning turns it on is not documented (test 4).

## Three things that follow for Silo, not only for setup

1. **The Virtualization framework wants a logged-in Mac.** Apple DTS: "Virtualization is not
   daemon safe", it weak-links AppKit, and VM starts failed with a security error tied to the
   Mac's login keychain when no one was logged in to the Mac (**read**,
   [thread 822670](https://developer.apple.com/forums/thread/822670)). So the program that holds
   main runs in your login session, not as a system daemon, and after the Mac restarts main comes
   back only once you log in. `engine-matches-wanted` says "a Mac that rebooted" is seen as it is;
   it should also say main waits for your login.
2. **Claude Code over SSH on macOS loses its login when the keychain is locked.** Claude Code
   keeps its subscription login in the macOS login keychain. Over SSH the keychain reads fail with
   `errSecInteractionNotAllowed` and Claude Code asks for `/login` again. The issue was closed for
   inactivity in April 2026 with no fix recorded (**read**,
   [anthropics/claude-code#29816](https://github.com/anthropics/claude-code/issues/29816); the usual
   workaround is `security unlock-keychain` in the shell). Auto-login unlocks the login keychain
   in the computer's own session, but whether SSH sessions then reach it is not shown by any
   source. The first-build story is "`ssh main`, start Claude Code, go to bed". This must be
   tested before anything else is built on it (test 6).
3. **Privacy permissions for programs started over SSH are hard to grant on macOS 27 from
   inside.** Tart grants Accessibility and Screen Recording to `sshd-keygen-wrapper` by writing
   `TCC.db` with SIP off. macOS 27 moves the user's TCC database into a protected container; the system database, which holds Accessibility and Screen Recording, stays where it was (correction from the fact-check, 13). Apple calls both an undocumented detail, so relying on editing them is fragile. For
   `agents-drive-the-screen`, Silo should drive the screen from outside, through the framework's
   display and input, which needs no permission inside the computer.

## Recommendation for Silo

Use Apple's guest provisioning only, and require macOS 27 on the Mac for the first build.

Why not keep a path for macOS 26 Macs: both workarounds need upkeep at each macOS release, by
people who read screenshots or reverse undocumented files, and both fail by showing a screen
nobody sees. Silo is not released yet. macOS 27 has been out since September 2026 and is at 27.2.
By the time Silo ships, macOS 26 Macs are the minority among people who give agents a Mac, and
`first-run-failure` already checks "the Mac's chip and system version" and says what to do. The
cost falls on polarzero first: the Mac measured in `file-changes-from-outside` runs macOS 26.5, so
it upgrades to 27 before the first build can be tried.

### The steps

1. **Check.** The Mac runs macOS 27 or later, on Apple silicon. If not, the first run says so,
   says to update macOS, and starts nothing (`first-run-failure`).
2. **Download.** `VZMacOSRestoreImage.latestSupported`, resumable, checked
   (`computer-image`). That image is macOS 27 because the Mac is.
3. **Install.** New `VZMacMachineIdentifier`, auxiliary storage and disk; `VZMacOSInstaller`.
4. **Keep the installed, never started computer.** Clone the disk and the auxiliary storage
   (APFS `clonefile`, no extra space) before the first start. This is "the installed system" of
   `computer-image`. A first start that fails is retried from it without installing again, and a
   later macOS computer starts from it with its own identifier, user and SSH host keys.
5. **First start with provisioning.** A password Silo generates, kept in your Mac's Keychain;
   `logsInAutomatically = true`, `enablesRemoteLogin = true`. The user name: yours, as
   `moving-in-on-macos` says, if test 1 shows Apple accepts it. If `validate()` or
   `setGuestProvisioning` throws `guestProvisioningInvalidUsername`, provision a setup user
   (`silo-setup`) instead and do step 7. Errors are never ignored.
6. **Wait for SSH**, through Silo's own network, with a time limit set from the times measured in
   test 2. Past it, Silo keeps a picture of the computer's screen, stops it, deletes that copy,
   and retries once from step 4. A second failure stops the first run with the picture and the
   reason. Main is never shown as ready half made.
7. **Finish over SSH, once, with the password.**
   - Put the key Silo made for this computer in `~/.ssh/authorized_keys`, then write
     `/etc/ssh/sshd_config.d/silo.conf` with `PasswordAuthentication no` and
     `KbdInteractiveAuthentication no`, and check that the key works and the password does not.
   - If step 5 used `silo-setup`: `sysadminctl -addUser ana -fullName ... -password ... -admin
     -adminUser silo-setup -adminPassword ...` (which gives `ana` a secure token), auto-login for
     `ana` (`autoLoginUser` and a correctly padded `kcpassword`, as Lume's comment explains),
     restart, check SSH as `ana`, then `sysadminctl -deleteUser silo-setup`.
   - Turn off sleep and the screen lock (`pmset`, `sysadminctl -screenLock off`), so a
     computer nobody looks at keeps running.
   - Whether agents get `sudo` without a password is a product choice
     (`what-an-agent-inside-may-do` says every tool); it is one file in `/etc/sudoers.d`.
8. **Check before calling it ready.** `fdesetup status` says FileVault is off;
   `sysadminctl -secureTokenStatus ana` is enabled; `launchctl print system/com.openssh.sshd`
   answers; the login keychain is unlocked in an SSH session. Then the first checkpoint.

### What to test on a Mac first, in this order

Each is short, needs a Mac on macOS 27.2, and changes a step above.

1. **Same user name.** Provision a macOS 27.2 computer with the Mac's own login name. Does
   `validate()` or the start throw `guestProvisioningInvalidUsername`, or does the computer show
   Setup Assistant? This decides step 5 and whether `moving-in-on-macos` holds as written.
2. **Time and reliability.** Ten fresh computers from the same installed copy: time from start to
   SSH answering, and how many fail. This gives the time limit of step 6 and the provisioning part
   of `first-run-time-macos`.
3. **What provisioning leaves.** FileVault, secure token and volume owner (`diskutil apfs
   listUsers /`), whether `softwareupdate` can install an update, which screens or dialogs appear
   on the first login.
4. **Full disk access over SSH.** `ls ~/Documents` and `ls ~/Desktop` over SSH on a provisioned
   computer. If refused, agents cannot work in those folders over SSH, and the first run has to say
   so or keep work elsewhere.
5. **A cut-off first start.** Stop the computer 20 seconds into its first start, then start it
   again with the same options. Provisioned or Setup Assistant? This decides whether step 4's copy
   is needed for retries or only for later computers.
6. **Claude Code's login over SSH.** In a provisioned computer with auto-login, `ssh main`, run
   `claude`, log in, leave, `ssh main` again, and restart the computer and repeat. Does the login
   last? Same for Codex. If it does not, the first build's main promise fails on its first night.
7. **A copy of the installed computer.** Provision a computer made from the step 4 clone with a new
   machine identifier and new auxiliary storage. Does it start and provision as the original does?
8. **Where the engine runs.** Start a computer from a program run by launchd as a user agent, and
   from one run as a system daemon, with nobody logged in to the Mac. Then restart a Mac with
   FileVault on and see when main can come back.

## What to add to the plan

- **A new decision for polarzero, in `computers`: which macOS your Mac must run.** Proposed
  answer: "Silo runs on a Mac with Apple silicon and macOS 27 or later. That is the first macOS
  that lets Silo set up a new macOS computer with nobody clicking. On an older Mac, the first run
  says so and starts nothing." It is a price the user pays, so it is polarzero's. The first build
  needs it.
- **A new decision agents can take, in `computers`: how a new macOS computer gets its user and
  SSH.** Proposed answer: "A new macOS computer is set up by Apple's guest provisioning on its
  first start: its user, auto-login and SSH. Silo then sets its SSH key and turns password login
  off, over SSH. Silo types nothing into Setup Assistant and writes nothing onto the disk before the
  first start." It promises nothing the user sees, and a check shows it holds (a test that makes a
  computer and reaches it by key). It needs the decision above. `computers-main-on-a-mac` should
  need it.
- **`computer-image`:** "Silo keeps the installed system" should say the system is kept as
  installed, before its first start, so each later macOS computer is set up on its own first start
  with its own user and SSH host keys.
- **`moving-in-on-macos`:** its notes should carry the user name report (FB23241619) and the
  workaround of step 7, until test 1 settles it. The answer stays.
- **`first-run-time-macos`:** the first start with provisioning, until SSH answers, is part of what
  is measured.
- **`computer-reached-over-ssh`:** its notes should carry the keychain report and full disk access
  for remote users, as what is to find out with Claude Code and Codex over SSH.
- **`engine-matches-wanted`:** the engine that holds a macOS computer runs in your login session,
  and after a restart of the Mac, main comes back once you log in.

## Challenges to decisions in force

- **`computer-image` and `main-on-macos-first` say "Apple forbids shipping macOS installed."**
  Cirrus Labs and MacStadium publish installed macOS images to anyone
  (`ghcr.io/cirruslabs/macos-*`, `ghcr.io/macstadium/orka-images/tahoe`), so the rule is a reading
  of Apple's licence, not a technical limit. With provisioning on macOS 27 the install is the only
  long wait left, so this changes nothing for the first build, and the decision should stay. The
  sentence would be more accurate as "Apple's licence does not let Silo ship macOS installed."
  Whether that reading is right is a question for a lawyer, so it belongs in Linear, not here.
- **`agents-drive-the-screen` (taken)** does not say how. On macOS 27 a program started over SSH
  cannot be given Accessibility or Screen Recording from inside without turning SIP off. Driving
  the screen from outside, through the framework's display and input, avoids that. Whoever writes
  `computers-macos-screen` should start there.

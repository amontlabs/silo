# macOS VM first run and overnight loop: numbers and gotchas

Researched 2026-11-24. Every claim has a source. **[measured]** means I measured it myself today against the live Apple CDN, the ipsw.me API, the GHCR registry or upstream source code. **[unverified]** means a single anecdote, a secondary source, or my own inference.

---

## 0. The ten facts that shape the design

1. **The macOS 27 restore image is 25.6 GB** (27.2.1, 25,595,954,706 bytes). macOS 26.x images were about 19.8 GB. [measured: `Content-Length` from updates.cdn-apple.com, and https://api.ipsw.me/v4/device/VirtualMac2,1?type=ipsw]
2. **Apple publishes checksums.** Its own catalog, https://mesu.apple.com/assets/macos/com_apple_macOSIPSW/com_apple_macOSIPSW.xml, gives `FirmwareSHA1` for each build. Apple's CDN also returns the **SHA-256 in an HTTP HEAD header**, `x-amz-meta-digest-sha256`, so you know the digest before downloading a byte. [measured] Tart uses that header as its cache key. [source: Tart `VM.swift`, https://raw.githubusercontent.com/cirruslabs/tart/main/Sources/tart/VM.swift]
3. **Downloading is the long pole, not installing.** At 100 Mbps the IPSW takes about 38 minutes and at 1 Gbps about 4 minutes. Install anecdotes run 5 to 7 minutes. Silo cannot honestly promise "agent in 10 minutes" on a typical home line unless the download starts before anything else and overlaps with every other step.
4. **Two macOS VMs per Mac is a hard cap**, enforced by the framework (`VZErrorDomain` code 6). There is also a **stale-slot bug**: after a guest-initiated shutdown the slot can stay taken until the host reboots. [https://developer.apple.com/forums/thread/830119]
5. **The Virtualization framework is not daemon-safe.** It weakly links AppKit (Apple DTS). VM creation and start can fail with a "security error" unless a GUI user is logged in or the login keychain is unlocked. So after a host reboot, VMs do not come back on their own unless the host auto-logs in, and **auto-login is impossible with FileVault on**. [https://developer.apple.com/forums/thread/841688, https://developer.apple.com/forums/thread/822670]
6. **Claude Code on macOS stores its login in the Keychain, and that keychain is locked over SSH.** Current docs say that when the Keychain write fails, as in an SSH session, Claude Code falls back to `~/.claude/.credentials.json` (mode 0600). [https://code.claude.com/docs/en/authentication.md] For an unattended VM, **`claude setup-token` → `CLAUDE_CODE_OAUTH_TOKEN`** is the documented headless path. It gives a one-year token, but it can only make model requests: no Remote Control and no claude.ai connectors.
7. **Codex CLI defaults to a plain file** (`~/.codex/auth.json`, 0600), so the keychain does not interfere. `codex login --device-auth` is the headless login, and it must first be enabled in the user's ChatGPT security settings. [https://learn.chatgpt.com/docs/auth, https://dev.classmethod.jp/articles/codex-cli-credentials-store-keyring/]
8. **A fresh macOS has no git.** `/usr/bin/git` is a stub that asks for a GUI install dialog. Over SSH that dialog fails with "no install could be requested (perhaps no UI is present)". The headless fix is the `softwareupdate` placeholder trick, which **Homebrew's installer already runs**. That step needs sudo.
9. **Nothing I found says a running VZ VM holds a sleep assertion.** [unverified; check `pmset -g assertions` with a VM running] Silo should take its own `PreventUserIdleSystemSleep` assertion while agents run. **A closed MacBook lid sleeps the Mac** regardless of `caffeinate`, unless it is on power with an external display attached, or `pmset disablesleep 1` is set.
10. **Disk: give the guest at least 60 GB logical**, because under 50 GB macOS cannot update itself. [https://eclecticlight.co/2026/05/02/how-fast-is-a-macos-vm-and-how-small-could-it-be/] Budget about 26 GB (IPSW) plus about 35 to 40 GB (installed VM) of real host space during the first run.

---

## 1. Restore image, install, first boot, disk, caches

### Sizes (Apple silicon "UniversalMac" IPSW, identical for every Apple silicon model and for VirtualMac2,1)

| Build | Size | Released | Source |
|---|---|---|---|
| 27.2.1 (26B131) | 25.60 GB | 2026-11-09 | [measured] api.ipsw.me + CDN `Content-Length` |
| 27.2 (26B123) | 25.60 GB | 2026-10-26 | api.ipsw.me |
| 27.0 (26A428) | 26.63 GB | 2026-09-14 | api.ipsw.me |
| 26.6.2 (25G83) | 19.77 GB | 2026-08-17 | api.ipsw.me |
| 26.0 era / 26.2 | 18.7 GB | 2025-12 | api.ipsw.me |
| Sequoia era (early 2025) | ~16.5 GB | | https://eclecticlight.co/2025/01/23/create-and-use-virtual-machines-on-apple-silicon-macs/ |

Each major version grows by about 30%. Expect macOS 28 to be around 30 GB. [inference]

- URL pattern: `https://updates.cdn-apple.com/2026FallFCS/<uuid>/UniversalMac_27.2.1_26B131_Restore.ipsw`. The server returns `Accept-Ranges: bytes`, so **downloads can resume**. [measured]
- `VZMacOSRestoreImage.fetchLatestSupported` returns the latest build the host supports. Tart's `--from-ipsw=latest` calls exactly that. [Tart `Create.swift`]

### Checksums
- **Apple mesu catalog**: `FirmwareSHA1` for each device and build, e.g. 27.2.1 → `834e8fd6d68520b2cabf0dc66cea28c2fc1462df`. [measured: https://mesu.apple.com/assets/macos/com_apple_macOSIPSW/com_apple_macOSIPSW.xml]
- **CDN HEAD header**: `x-amz-meta-digest-sha256: 233e85a09887579d47d0b195e7db72be7f204261059c6946f719acf6b300814f` for 27.2.1. ipsw.me returns the same value as `sha256sum`, along with sha1 and md5. [measured]
- So Silo can **verify a user's existing IPSW** from any tool against an Apple-served digest, with no third party involved.

### Install time (VZMacOSInstaller): no good public benchmark exists
- UTM with Sonoma 14.5: download about 5 minutes, install about 5 more. [https://jensd.be/?p=2158, unverified, 2024]
- Orka with Ventura on an M3: "a good seven minutes". [https://heise.de/-9799417]
- Viable: a full custom VM in under 15 minutes. [https://eclecticlight.co/2023/09/07/how-to-build-a-custom-macos-vm-on-apple-silicon/]
- Apple: stopping or pausing the VM during install gives undefined behaviour. Progress comes from the `progress` property. [https://developer.apple.com/documentation/virtualization/vzmacosinstaller]
- A failure mode to detect: one developer's `install` ran 11 minutes with no callback because of a sandbox denial. [https://developer.apple.com/forums/thread/849369] **Silo needs a watchdog on install progress.**
- **We should measure this ourselves on M1, M2 and M4 hosts with macOS 27.** [gap]

### First boot and provisioning
- `VZMacGuestProvisioningOptions` (macOS 27 host **and** guest) takes `fullName`, `username`, `password`, `logsInAutomatically` and `enablesRemoteLogin`, set through `VZMacOSVirtualMachineStartOptions.setGuestProvisioning`. It is **honoured only on the first boot**: if a user already exists, it is ignored. [https://developer.apple.com/videos/play/wwdc2026/224/]
- Known bugs:
  - A user created this way is not returned by `CSIdentityQueryExecute()` (FB23716201). [https://developer.apple.com/forums/thread/839343]
  - A "Threading warning" failure. [https://developer.apple.com/forums/thread/834397]
  - Provisioning reportedly **fails when the guest username matches the host user's name**. [reader comment, https://eclecticlight.co/2026/06/12/macos-virtualisation-is-leaping-forward-in-golden-gate/, unverified] **Test this. If it holds, Silo should not default the guest user to `$USER`.**
- Lume's pre-27 approach, for contrast: install, boot once, stop, mount the Data volume on the host, write the account, SSH, auto-login and no-sleep settings as files, then boot again and wait for SSH. That is two boots. [https://cua.ai/docs/concepts/how-lume-unattended-setup-works]
- Time from boot to sign-in on a cloned Tart VM: about 20 to 25 seconds. [https://www.starryhope.com/mac/tart-macos-linux-vms/] A first boot with Setup Assistant takes longer. No number found. [gap]

### Disk
| Figure | Source |
|---|---|
| >40 GB absolute minimum, 50 GB prudent, 100 GB comfortable | https://eclecticlight.co/2025/01/23/create-and-use-virtual-machines-on-apple-silicon-macs/ |
| **<50 GB: macOS cannot update itself; 60 GB recommended** | https://eclecticlight.co/2026/05/02/how-fast-is-a-macos-vm-and-how-small-could-it-be/ |
| A 100 GB sparse VM uses about 54 GB on the host after use | same |
| Tart 27 vanilla image: 35.06 GB uncompressed; 27 base: 38.4 GB | [measured: GHCR manifest annotations] |
| Tart 26 images: 50 GB logical disk; Xcode image: 140 GB | [measured] |
| Lume Tahoe image: 160 GB logical, 24.2 GB to download | [measured] |
| Each VM grows by 1 to 2 GB with normal use | https://www.starryhope.com/mac/tart-macos-linux-vms/ |
| ASIF: deleting files in the guest reportedly does not reclaim host space (RAW does) | reader comment on Eclectic Light 2026-06-12 [unverified] |
| Idle macOS guests write more than expected, so overlays grow quickly | https://bitrise.io/blog/post/wwdc26-the-virtualization-framework-updates-that-matter-for-large-mac-fleets |

**First-run host budget**: about 26 GB IPSW + about 35 GB installed = about **61 GB free**, or about 35 GB if the IPSW is deleted afterwards. Cua asks for 40 GB free and 8 GB RAM. [https://cua.ai/docs/tutorials/create-your-first-lume-vm]

### Where tools cache IPSWs, and whether they can be reused
| Tool | Location | Reuse | Source |
|---|---|---|---|
| Tart | `~/.tart/cache/IPSWs/sha256:<hex>.ipsw` (or `$TART_HOME`) | Before downloading, Tart sends a HEAD request, reads `x-amz-meta-digest-sha256` and uses the cached file if it exists. **The file name is the SHA-256**, so any tool can find a 27.2.1 IPSW there by digest. | Tart `Config.swift`, `IPSWCache.swift`, `VM.swift` (raw.githubusercontent.com/cirruslabs/tart/main/...) [measured in source] |
| VirtualBuddy | `~/Library/Application Support/VirtualBuddy/_Downloads/UniversalMac_<ver>_<build>_Restore.ipsw` | Its UI lets you "select from an IPSW you have already downloaded" | https://github.com/insidegui/VirtualBuddy/discussions/69 ; https://github.com/p-linnane/VirtualBuddy/blob/main/README.md |
| UTM | Inside its container's Caches folder. One user found UTM did not offer to reuse it. | | https://github.com/utmapp/UTM/discussions/4097 [unverified path] |
| Apple Configurator | Its cache is large and cannot be moved to another drive | | https://twocanoes.com/knowledge-base/feedback-apple-configurator-cache-is-large-and-not-able-to-be-put-on-another-drive/ |
| Lume / Cua | Keeps a pulled image as a stopped base VM; `cua cache ls` lists it | | https://cua.ai/docs/tutorials/create-your-first-lume-vm |
| Framework itself | Has no cache. Developers ask how to avoid re-downloading. | | https://developer.apple.com/forums/thread/741861 |
| Manual | `~/Downloads/UniversalMac_*_Restore.ipsw` (from ipsw.me or Mr. Macintosh) | | |

**Reuse:** an IPSW is a plain file that `VZMacOSRestoreImage.load(from:)` opens no matter which tool downloaded it. Verified by SHA-1 (mesu) or SHA-256 (CDN header), it is the same artifact. **Silo can scan those locations, hash candidates (about 1 minute to SHA-256 25 GB on Apple silicon [unverified estimate]), and skip about 26 GB of download.** Install is fastest when the IPSW is on the same volume. [Eclectic Light 2025-01-23]

---

## 2. Fresh macOS over SSH: CLT, git, Homebrew

- **The git stub:** `/usr/bin/git` and `/usr/bin/make` are shims. With no developer tools, they ask for the GUI install. Over SSH there is no UI, so `xcode-select --install` fails with "no install could be requested (perhaps no UI is present)". [https://cloudstreet-dev.github.io/macOS-and-Unix/part7-development/xcode-cli-tools.html, https://computingforgeeks.com/install-xcode-command-line-tools-on-macos] With provisioning's auto-login on, the dialog may appear on the **guest's** display, where no one is looking. [inference]
- **Headless CLT install** (the community-standard trick, not documented by Apple):
  ```sh
  sudo touch /tmp/.com.apple.dt.CommandLineTools.installondemand.in-progress
  label=$(softwareupdate -l | grep -B1 -E 'Command Line Tools' | awk -F'*' '/^ *\*/ {print $2}' | sed -e 's/^ *Label: //' -e 's/^ *//' | sort -V | tail -n1)
  sudo softwareupdate -i "$label"
  sudo xcode-select --switch /Library/Developer/CommandLineTools
  sudo rm /tmp/.com.apple.dt.CommandLineTools.installondemand.in-progress
  ```
  Sources: https://gist.github.com/rtrouton/e58d31af97a26f77229096114a484d4a, and Homebrew `install.sh` lines 885-910 [measured: https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh]
- **Size and time:** "under 1 GB", "about 700 MB, 2 to 3 minutes on a fast connection", "1 to 2 GB". Sources disagree. An update for Tahoe 26.1 needed 4 GB to install. [https://computingforgeeks.com/install-xcode-command-line-tools-on-macos, https://buymeacoffee.com/hanshafner/install-xcode-command-line-tools-macos, https://discussions.apple.com/thread/256189596] [unverified; measure it]
- **Known breakage:** CLT 14.2 failed through `softwareupdate` on 12.6.1 [https://discussions.apple.com/thread/254465907]. On 26.6 the `/usr/bin/git` wrapper broke while `/Library/Developer/CommandLineTools/usr/bin/git` still worked [https://discussions.apple.com/thread/256335209].
- **Homebrew unattended** [measured in install.sh]:
  - `NONINTERACTIVE=1`, or `CI=1`, or a stdin that is not a TTY, all mean no prompts.
  - Under `NONINTERACTIVE`, Homebrew calls `sudo -n` (or `sudo -A` if `SUDO_ASKPASS` is set). **So the user needs passwordless sudo, or a primed sudo timestamp, or `SUDO_ASKPASS`.** Otherwise sudo checks fail, the CLT step is skipped, and the GUI fallback runs only when stdin is a TTY.
  - The installer itself runs the CLT placeholder trick (`softwareupdate -i`, then `xcode-select --switch`) when `/Library/Developer/CommandLineTools/usr/bin/git` is missing and sudo works.
  - It refuses to run as root.
  - On Apple silicon, bottles and casks install without CLT. CLT is needed only to build from source. [https://docs.brew.sh/Installation]
- **Design implication for Silo:** the provisioned user is an admin. Either (a) pipe the known provisioning password to `sudo -S` once over SSH, (b) write a temporary `/etc/sudoers.d/silo` NOPASSWD rule during first run, or (c) skip Homebrew and install the CLT plus the agent's native binary directly. **Claude Code's native installer needs neither git nor CLT nor sudo.** It installs to `~/.local/bin/claude`, which links into `~/.local/share/claude/versions/`, and git is "optional". [https://code.claude.com/docs/en/setup.md] Agents doing real work need git, though, so CLT stays on the critical path. Start it in parallel with agent login.

---

## 3. Agent login inside the VM over SSH

### Claude Code
- Install: `curl -fsSL https://claude.ai/install.sh | bash`. Needs macOS 13 or later and 4 GB or more of RAM. Updates itself in the background. [https://code.claude.com/docs/en/setup.md]
- **Keychain over SSH:** the login keychain unlocks only at GUI login. Over SSH, Keychain returns `errSecInteractionNotAllowed`.
  - Issue #29816 asked Claude Code to fall back to `~/.claude/.credentials.json`. It was **closed "not planned" by the stale bot on 2026-04-10** and locked. [https://github.com/anthropics/claude-code/issues/29816] Related: #10158, #5957, #5515, #9403, #44028 (not fetched: claudeissues.com was blocked here).
  - **Current docs:** "When the Keychain rejects the write, such as when it's locked in an SSH session, Claude Code stores your login in `~/.claude/.credentials.json` with file mode 0600 instead." [https://code.claude.com/docs/en/authentication.md] So the write fallback exists. Whether later reads prefer the keychain is [unverified]. A session started from the guest's GUI auto-login, with an unlocked keychain, would see different credentials than an SSH session. That is a source of confusing "logged out" states.
  - Workaround people use: `security unlock-keychain ~/Library/Keychains/login.keychain-db` in `~/.zshrc` when `$SSH_CONNECTION` is set. It prompts for the password. [https://phoenixtrap.com/2025/10/26/claude-code-cli-over-ssh-on-macos-fixing-keychain-access/] The keychain can lock again after idle time or a tmux restart [https://thekodelab.com/en/posts/macos-ssh-keychain-unlock/]. This matters for overnight runs. `security set-keychain-settings` (no timeout) is the usual fix [unverified].
  - **Auto-login via provisioning unlocks the login keychain for the GUI session.** Whether SSH sessions then see it unlocked is [unverified: test it]. That could make the whole problem disappear for Silo.
- **The headless login flow:** `claude` or `claude auth login` prints a URL (press `c` to copy it). The user opens it in the **host's** browser, signs in, and if no redirect comes back, pastes the code at `Paste code here if prompted`. [https://code.claude.com/docs/en/authentication.md] Silo can open that URL on the host itself.
- **`claude setup-token`:** runs the same browser flow, **prints** a one-year OAuth token and stores nothing. Set it as `CLAUDE_CODE_OAUTH_TOKEN`.
  - Limits: model requests only, no Remote Control, no claude.ai connectors, and not read in `--bare` mode.
  - In precedence it sits **above** a `/login` subscription credential.
  - Several user reports describe `401 Invalid bearer token` with setup-tokens on `claude -p`. [https://github.com/anthropics/claude-code/issues/69753 et al., unverified status]
- **Overnight relevance:** `claude auth status --text` shows "Sign-in ends: between X and Y". **Using Claude Code does not extend the login.** A background session stops making progress once the login expires. [https://code.claude.com/docs/en/authentication.md] Silo should read this before starting an overnight run and warn early.
- Several accounts: use `CLAUDE_CONFIG_DIR`. The Keychain entry is keyed to that directory.

### Codex CLI
- `codex login --device-auth` (beta): open the link and enter the one-time code. **The user must first enable device-code login in their ChatGPT security settings, or an admin in workspace permissions.** That is an extra first-run step to explain. [https://learn.chatgpt.com/docs/auth]
- Alternatives:
  - `ssh -L 1455:localhost:1455` and a normal `codex login`. **Silo can do this port forward itself.**
  - Copy `~/.codex/auth.json` from another machine.
  - `printenv OPENAI_API_KEY | codex login --with-api-key`.
- Storage: `cli_auth_credentials_store` = `file` (the **default**, `~/.codex/auth.json`, 0600), `keyring` (fails if the keychain is unavailable), `auto` (falls back to the file) or `ephemeral`. The keychain service name is "Codex Auth". [https://dev.classmethod.jp/articles/codex-cli-credentials-store-keyring/] With the default, the locked SSH keychain does not affect Codex.
- Tokens refresh automatically during active sessions.

---

## 4. git and GitHub credentials in the VM

| Option | Scope | Notes | Source |
|---|---|---|---|
| `gh auth login` (web or device flow, one-time code) | The user's whole account scopes | On macOS it stores the token in the **Keychain**, so the same SSH lock problem applies. Falls back to a plain-text file, or `--insecure-storage`. `GH_TOKEN` env is the headless route. | https://cli.github.com/manual/gh_auth_login ; https://man.archlinux.org/man/gh-auth-login.1.en |
| Fine-grained PAT | Chosen repos, chosen permissions (e.g. contents + pull requests write) | Org default maximum lifetime is 366 days, settable from 1 to 366 | https://docs.github.com/en/organizations/managing-programmatic-access-to-your-organization/setting-a-personal-access-token-policy-for-your-organization ; https://github.blog/changelog/2024-10-18-new-pat-rotation-policies-preview-and-optional-expiration-for-fine-grained-pats/ |
| Deploy key | One repo, read-only or write | An SSH key that lives in the VM and cannot reach other repos | GitHub docs (not fetched) [unverified detail] |
| GitHub App installation token | Repos where the app is installed | **Expires in about 1 hour.** The best blast radius, but needs a refresher outside the VM. | https://dev.to/kalemi/one-scoped-api-key-per-coding-agent-1nhj ; https://www.philschmid.de/managed-agents-gh |
| SSH agent forwarding (`ssh -A main`) | **Every host and repo the user's key can reach** | Any process in the VM can use the socket while the connection is open. Treated as a credential leak by agent-sandbox writers. | https://gabrielhuang.github.io/blog/2026-02-03-vscode_ssh ; https://permit.io/blog/coding-agent-sandboxes-credentials |

Practice among agent-sandbox builders: give each sandbox its own repo-scoped secret, never the host's credentials. [https://micahflee.com/sandboxing-coding-agents/] **Silo should not use `ssh -A` by default.** Mint or ask for a fine-grained PAT, or a GitHub App token, per VM.

---

## 5. Keeping the Mac awake overnight

- **Does a running VZ VM block host sleep?** No source says so. [unverified] Parallels VMs show up in Activity Monitor's "Preventing Sleep" column [https://forum.parallels.com/threads/paused-vm-preventing-sleep.362101/], but that is Parallels' own assertion. **Silo should hold its own `IOPMAssertionCreateWithName(kIOPMAssertionTypePreventUserIdleSystemSleep, …)` while agents run** (the same assertion as `caffeinate -i`), then confirm it with `pmset -g assertions`.
- **`caffeinate` flags:**
  - `-i`: no idle sleep; the display may still sleep.
  - `-s`: no system sleep, **on AC power only** (does nothing on battery).
  - `-d`: keeps the display on.
  - `-m`: disk.
  - `-u`: declares user activity.
  - `-w pid`: holds until that process exits.

  [https://www.unix.com/man-page/osx/8/caffeinate, https://blog.openreplay.com/mac-awake-overnight-agent-runs-caffeinate/]
- **Lid closed:** Apple's clamshell mode needs **power plus an external display** (plus keyboard and mouse). Otherwise closing the lid sleeps the Mac, **and `caffeinate` cannot stop that**. `sudo pmset -a disablesleep 1` can, but it may leave the internal panel lit under the lid and the Mac runs hotter. Amphetamine's Closed-Display Mode and the Sleepless app wrap this. [https://www.androidauthority.com/prevent-sleep-mode-macbook-3188113, https://aboudjem.github.io/Sleepless/, https://www.howtogeek.com/278943/how-to-keep-your-macbook-awake-while-closed/] **This is the most likely overnight failure for MacBook users. Silo must tell them before they leave.**
- The Battery setting "Prevent automatic sleeping when the display is off" applies only on power. [openreplay]
- **pmset for an always-on host:** `sudo pmset -c sleep 0` (on AC), `autorestart 1` (reportedly unreliable on Apple silicon), Power Nap off (it wakes the machine at random). `womp` and `tcpkeepalive` are contested. [https://forums.macrumors.com/threads/mac-mini-apple-silicon-ignores-autorestartatconnect-no-power-on-after-ac-restore.2482607/, https://travis.media/blog/mac-homelab-ai-agent-server, https://hometechops.com/mac/mac-mini-keeps-going-to-sleep]
- **What happens to a VM when the host sleeps:**
  - The guest is suspended along with the host.
  - **Clock:** for **macOS guests the framework has its own time sync**; Linux guests do not and drift by the length of the sleep. [https://developer.apple.com/forums/thread/760344, Apple engineer] So a macOS guest should catch up, but how fast is [unverified].
  - **Network:** reports from UTM (Apple backend, Linux guest) and Parallels describe guests losing their network after the host wakes until the interface is cycled. [https://github.com/utmapp/UTM/issues/3372]
  - **Agent impact:** streaming HTTPS connections to Anthropic or OpenAI die across a sleep. Claude Code and Codex retry, but a long tool call or a `git push` mid-flight fails. [inference]
- **Host auto-updates overnight:** with "Install macOS updates" on, Apple asks for the Mac to be powered on and awake for "Update Tonight". One Mac **rebooted around 01:40 with the setting off**. Managed deadlines can restart Macs on the first or second night. [https://support.apple.com/guide/mac-help/mchla7037245/mac, https://eclecticlight.co/2025/02/27/how-your-mac-can-update-macos-when-you-dont-want-it-to/, https://www.bu.edu/tech/services/cccs/desktop/device-security/desktop-patching/managed-macos-updates] Background Security Improvements do not restart the Mac. [https://support.apple.com/en-mide/101591]
- **After a host reboot**, the VM is gone until a GUI user logs in:
  - Virtualization is not daemon-safe (Apple DTS: it weak-links AppKit).
  - A root LaunchDaemon gets `ctkd … SepKey ACL` "security error".
  - Anka reported creating and starting VMs fails without auto-login or a GUI login.
  - FileVault blocks auto-login.

  [https://developer.apple.com/forums/thread/841688, https://developer.apple.com/forums/thread/822670, https://community.jamf.com/t5/jamf-pro/auto-login-with-filevault-enabled/m-p/283161] **Silo's overnight promise must be "survives sleep attempts, not reboots".** It should report on the morning screen that the host rebooted and what was lost.
- **Inside the guest:** turn off sleep in the VM too (Lume writes no-sleep settings during setup), and turn off guest auto-update restarts.
- **VM process lifetime:** if the process that owns `VZVirtualMachine` crashes, the VM dies with it. Several 2026 forum threads ask Apple about supported teardown. [https://developer.apple.com/forums/thread/849681, https://developer.apple.com/forums/thread/850709] Agents inside need tmux, or a launchd agent in the guest, to survive SSH drops, but not host-process death.

---

## 6. Concurrency and memory

- **Cap of two macOS VMs per Mac.** The EULA allows "up to two additional copies"; the framework enforces it with `VZErrorVirtualMachineLimitExceeded` (code 6).
  - Linux VMs (`VZGenericPlatformConfiguration`) have no cap (Apple DTS).
  - Two VMs need distinct machine identifiers and MAC addresses.
  - Getting around the cap means a dev kernel collection and breaks the licence.

  [https://developer.apple.com/forums/thread/845455, https://eclecticlight.co/2022/08/04/virtualisation-on-apple-silicon-macs-8-how-apple-limits-vms/, https://khronokernel.com/macos/2023/08/08/AS-VM.html]
- **The stale-slot bug** (FB22967193): after a guest-initiated shutdown, or `tart stop` on a `--no-graphics` VM, the slot stays taken. Only a host reboot frees it. [https://developer.apple.com/forums/thread/830119] **Silo should stop VMs from the host side, and detect code 6 with a clear message.**
- **RAM:**
  - 4 GB with 2 cores is the usable minimum: Tahoe used about 3.1 GB.
  - 8 GB with 4 cores used about 5 GB.
  - 8 GB is "barely sufficient", 16 GB preferable, and at least 3 cores is wise.
  - Tart default: 2 CPUs, 4 GB. Lume Tahoe image: 4 CPUs, 8 GB.

  [Eclectic Light 2026-05-02 and 2025-01-23; https://tart.run/quick-start/; measured Lume annotations] Parallels' rule: at most half the host's RAM. [https://kb.parallels.com/113649] **An 8 GB host Mac can run one 4 GB guest at a stretch. A 16 GB host can run one 8 GB guest comfortably.** Claude Code needs 4 GB or more on its own. [setup docs]
- **Memory balloon:** `VZVirtioTraditionalMemoryBalloonDevice` exists (macOS 11+). You set `targetVirtualMachineMemorySize`, but **"the guest decides"**. No source confirms that macOS guests honour it. [https://developer.apple.com/documentation/virtualization/vzvirtiotraditionalmemoryballoondevice, unverified] Do not plan around reclaiming guest RAM.
- Performance: single-core about 98% of the host, GPU about 95%, multi-core scales with assigned cores. [Eclectic Light 2026-05-02]

---

## 7. Time to first value

### Prebuilt image sizes [measured from GHCR manifests today]
| Image | Download (compressed) | Logical disk | Uploaded |
|---|---|---|---|
| `ghcr.io/cirruslabs/macos-tahoe-vanilla` | 23.99 GB | 50 GB | 2026-08-18 |
| `ghcr.io/cirruslabs/macos-tahoe-base` (brew + tools) | 27.07 GB | 50 GB | 2026-11-07 |
| `ghcr.io/cirruslabs/macos-tahoe-xcode` | 70.79 GB | 140 GB | 2026-10-31 |
| `ghcr.io/cirruslabs/macos-golden-gate-vanilla` (27) | 29.96 GB | 35.1 GB | 2026-10-31 |
| `ghcr.io/cirruslabs/macos-golden-gate-base` (27) | 31.77 GB | 38.4 GB | 2026-11-07 |
| `ghcr.io/trycua/macos:26` (Lume Tahoe) | 24.21 GB | 160 GB | 2026-10-21 |
| `ghcr.io/trycua/macos-sequoia-vanilla` | 17.82 GB | 40 GB | 2026-08-11 |

**A prebuilt image is not smaller than the IPSW.** It is a 24 to 32 GB download either way. Building from the IPSW costs about 5 to 10 extra minutes of install, but avoids trusting a third-party image and gets the user Apple's bits, checksummed. A prebuilt image skips the install and the Setup Assistant.

Reported times to a first VM:
- Lume/Cua first create (pulls 23 GB): **about 8 minutes**; later creates (clones) about 25 seconds. [https://cua.ai/docs/tutorials/create-your-first-lume-vm] That implies about a 400 Mbps or faster link.
- Tart clone of a cached image: 0.03 seconds; boot to sign-in: 20 to 25 seconds; pull: "several minutes" on fast wired. [https://www.starryhope.com/mac/tart-macos-linux-vms/]
- Orka: about 7 minutes for a fresh install. [heise]
- UTM: 5 minutes download + 5 minutes install. [jensd]

### Download time (at 90% link efficiency)
| Artifact | Size | 50 Mbps | 100 Mbps | 300 Mbps | 1 Gbps |
|---|---|---|---|---|---|
| IPSW macOS 27.2.1 | 25.60 GB | 76 min | 38 min | 13 min | 4 min |
| IPSW macOS 26.6.2 | 19.77 GB | 59 min | 29 min | 10 min | 3 min |
| Tart tahoe-vanilla | 23.99 GB | 71 min | 36 min | 12 min | 4 min |
| Tart tahoe-base | 27.07 GB | 80 min | 40 min | 13 min | 4 min |
| Tart golden-gate-vanilla (27) | 29.96 GB | 89 min | 44 min | 15 min | 4 min |
| Tart golden-gate-base (27) | 31.77 GB | 94 min | 47 min | 16 min | 5 min |
| Tart tahoe-xcode | 70.79 GB | 210 min | 105 min | 35 min | 10 min |
| Lume macos:26 | 24.21 GB | 72 min | 36 min | 12 min | 4 min |
| CLT (about 1 GB, unverified) | 1 GB | 3 min | 1 min | <1 min | <1 min |

### Critical path for "first agent running", with IPSW and provisioning, on 300 Mbps [estimate]
| Step | Time |
|---|---|
| Fetch and verify the IPSW (or reuse one from Tart or VirtualBuddy: 0 minutes download plus about 1 minute to hash) | ~13 min |
| `VZMacOSInstaller` | ~5–8 min [unverified] |
| First boot with provisioning, until SSH answers | ~1–2 min [unverified] |
| In parallel: CLT via softwareupdate (~1–3 min) ‖ `curl claude.ai/install.sh` (<1 min) ‖ agent OAuth in the host browser (user-paced, ~1 min) | ~3 min |
| **Total** | **~22–26 min** (300 Mbps), **~47–50 min** (100 Mbps), **~13 min** (1 Gbps, or an IPSW already on disk) |

**Levers:**
- Start the IPSW download at the first click, before asking anything else.
- Reuse an existing IPSW, found by its SHA-256.
- Resume the download with range requests.
- Ask for the agent login while the install runs. A login URL needs no VM: `claude setup-token` or Codex device auth could even run on the host, with the token injected later.
- Never make the user wait on CLT.

---

## Gaps worth measuring ourselves
1. `VZMacOSInstaller` duration on M1, M2 and M4, and on macOS 27 hosts.
2. Time from first boot with provisioning to SSH ready.
3. Whether provisioning's auto-login makes the login keychain usable from SSH sessions.
4. Whether a running VZ VM holds any power assertion.
5. Clock and network after host sleep for a macOS 27 guest.
6. CLT download size on 27.2.
7. Whether provisioning fails when the guest username equals the host username.
8. Whether `claude` reads `.credentials.json` over SSH when an entry also exists in the keychain.

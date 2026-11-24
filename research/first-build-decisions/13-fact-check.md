# Fact check of the first-build research (files 01 to 12, not 07)

A skeptical check, 24 November 2026, of the claims the recommendations in this folder rest on. Every
source in the table was fetched today and read; "read" means the page itself, not a search summary,
unless the note says so. The research files were not changed.

Overall: almost every load-bearing claim about late 2026 is real. The research agents did not invent
the macOS 27 APIs, the CVEs, the forum threads or the incidents. Three claims are wrong, and one of
them changes how Silo should describe its nearest competitor.

## Table

| Claim | File | Status | Source | Note |
|---|---|---|---|---|
| `VZMacGuestProvisioningOptions` exists, macOS 27.0+, with `fullName`, `username`, `password`, `logsInAutomatically`, `enablesRemoteLogin`; needs a macOS 27 guest; evaluated only on the first boot after restore | 05, 10, 12 | VERIFIED | https://developer.apple.com/documentation/virtualization/vzmacguestprovisioningoptions.md | Both quoted sentences are in Apple's reference word for word |
| WWDC26 session 224 presents provisioning (user created, auto-login, SSH) and DiskImageKit overlays "for copy-on-write semantics for snapshots" | 05, 10, 12 | VERIFIED | https://developer.apple.com/videos/play/wwdc2026/224/ | Title: "Expand the capabilities of your Virtualization app". "Only honored if the guest has not already been set up" |
| Tart PR #1263 adds `--provisioning-opts`, merged 9 June 2026, refuses hosts older than macOS 27 | 10 | VERIFIED | https://github.com/cirruslabs/tart/pull/1263 (now `openai/tart`) | Merged by fkorotkov-oai |
| Tart's macOS 27 template (`vanilla-golden-gate.pkr.hcl`, commit `16e9ad7`) replaced the Setup Assistant keystrokes with `--provisioning-opts`, dropping 82 lines | 10 | VERIFIED (date UNVERIFIABLE) | https://github.com/cirruslabs/macos-image-templates/commit/16e9ad7 | +21 / −82, boot command replaced by `<wait120s>`. The page did not show the commit date (13 August 2026 in file 10) |
| VirtualBuddy 2.2 skips Setup Assistant by configuring an account first; needs macOS 27 on host and guest | 10 | VERIFIED | https://newreleases.io/project/github/insidegui/VirtualBuddy/release/2.2 | Changelog quoted exactly |
| Apple's own sample code uses guest provisioning | 10 | VERIFIED | https://developer.apple.com/tutorials/data/documentation/virtualization/running-macos-in-a-virtual-machine-on-apple-silicon.md | The sample creates "admin" with `VZMacGuestProvisioningOptions` |
| FB23241619: provisioning fails when the guest user name equals the host login name; DTS said "weird"; no fix | 10 | VERIFIED | https://developer.apple.com/forums/thread/834397 | The exact log text is not in the thread. Workaround: a different name. Confirmed by a reader comment on Eclectic Light |
| `VZError.Code.guestProvisioningInvalidUsername` exists | 10 | VERIFIED (link to the bug is an inference) | https://developer.apple.com/documentation/virtualization/vzerror/guestprovisioninginvalidusername.md | The page has no description. Nobody showed that this code is what the same-name case throws: the reporter used `try?` and saw the failure only in logs |
| DiskImageKit: read-only base, one cache layer, ASIF overlays, copy-on-write snapshots | 05, 12 | VERIFIED | https://developer.apple.com/documentation/diskimagekit.md | |
| Merging an overlay back into the base is not supported | 12 | VERIFIED as "no public API" | same page; https://eclecticlight.co/2026/06/12/macos-virtualisation-is-leaping-forward-in-golden-gate/ (comments) | The framework's topics list create, open, append and truncate, but no merge, flatten or commit. Apple never says it is unsupported. The Eclectic Light author: "I don't think Apple intends to support merging back to the base" |
| Bitrise: provisioning is "the closest macOS has come to cloud-init"; overlays give "snapshot- and fork-like behavior" | 10, 12 | VERIFIED | https://www.bitrise.io/blog/post/wwdc26-the-virtualization-framework-updates-that-matter-for-large-mac-fleets | |
| macOS 27 released 14 September 2026 | 12 | VERIFIED | https://www.macrumors.com/2026/09/14/apple-releases-macos-golden-gate/ ; ipsw.me lists 27.0 on that date | Named "macOS Golden Gate 27" |
| Cirrus Labs joined OpenAI's Agent Infrastructure team, 7 April 2026; licences to become more permissive | 12 | VERIFIED (search) | https://cirruslabs.org/ , https://macstadium.com/blog/cirrus-labs-is-joining-openai (via search) | Tart's PRs are now under `openai/tart` and merged by `-oai` accounts, which agrees |
| Lima 2.1: experimental macOS guests (`template:macos`), `limactl shell --sync` | 12 | VERIFIED | https://www.cncf.io/blog/2026/03/25/lima-v2-1-macos-guests-and-enhanced-ai-agent-safety/ , https://lima-vm.io/docs/usage/guests/macos/ | Host must run the same or a newer macOS than the guest |
| Lima 2.3 (8 Nov 2026): `--sync-exclude`, `hostLoopback: false`, `suppressFirstLoginSetup`, `limactl snapshot` with krunkit only while stopped | 04, 12 | VERIFIED | https://cncf.io/blog/2026/11/08/lima-v2-3-native-on-windows-vmnet-on-linux-and-menu-tui ; Lima `docs/config/network/user.md` on GitHub | `hostLoopback: false` works only with `vmType: vz` |
| `suppressFirstLoginSetup` writes `com.apple.SetupAssistant.plist` as root before the first login | 12 | VERIFIED | https://lima-vm.io/docs/usage/guests/macos/ | |
| Fly Sprites: "like a git commit for your whole system", about 300 ms, copy-on-write, restore in place | 12 | VERIFIED | https://fly.io/sprites/checkpoint-restore.md | |
| Fly Sprites checkpoints are "taken by hand" | 12 | **WRONG** | https://fly.io/sprites | Fly: "You don't have to remember to take them. A Sprite checkpoints itself after a stretch of continuous work, when it goes idle, and on graceful shutdown, keeping a tiered history" |
| Fly Sprites come with Claude Code, Codex and Gemini preinstalled and taught to checkpoint | 12 | UNVERIFIABLE | https://fly.io/sprites/checkpoint-restore.md , https://fly.io/learn/sprites-for-ai-agents.md , https://fly.io/sprites | None of the three pages says so. Fly says "point your LLM at /.sprite/llm.txt" and "official plugins for every serious coding agent" |
| Docker Sandboxes: microVM per agent, its own Docker daemon, proxy swaps a placeholder key for the real one, live two-way workspace mount ("changes in either direction are instant"), clone mode, free locally | 12 | VERIFIED | https://docs.docker.com/ai/sandboxes/architecture/ , https://docs.docker.com/ai/sandboxes/ , https://docs.docker.com/ai/sandboxes/install/ | Hosts: macOS 14+ on Apple silicon, Windows 11, and Ubuntu 24.04+ (file 12 omits Linux). `sbx save` and `sbx policy log` not checked |
| Claude Cowork local VM escape, July 2026: guest root via CVE-2026-46331, then the writable VirtioFS mount; default moved to the cloud afterwards | 12 | VERIFIED (one secondary source) | https://thenextweb.com/news/claude-cowork-sandbox-escape-mac-files-sharedroot | TNW, 26 July 2026; disclosed by Accomplish AI ("SharedRoot") on 23 July. Anthropic closed the report as "informative" |
| GPT-5.6 in Codex deleted `$HOME` after overriding it for a temp directory; full access mode without auto review | 12 | VERIFIED | https://simonwillison.net/2026/Jul/16/bad-codex-bug/ , https://www.infoworld.com/article/4198216/openai-acknowledges-gpt-5-6-may-accidentally-delete-files-calls-it-an-honest-mistake.html | Explanation from Codex lead Thibault Sottiaux |
| Matt Shumer: "almost all" files on his Mac | 12 | VERIFIED, wrong citation | search: incidentdatabase.ai, techtimes.com | The quote is real ("accidentally deleted almost ALL of my Mac's files", 10 July 2026, model "GPT-5.6 Sol", Ultra mode). It is not on the Willison or InfoWorld pages the row cites |
| GPT-5.6 system card: "greater tendency than GPT-5.5 to go beyond the user's intent"; Bruno Lemos's production database | 12 | VERIFIED | InfoWorld, above | |
| claude-code #49102: 16 Apr 2026, 2.1.89, Opus 4.6, background `rm -rf`, case-insensitive APFS typo, ~40 worktrees and ~40 projects, Time Machine off, 1.3 GB recovered, closed by the stale bot, asks for tool-layer enforcement | 12 | VERIFIED | https://github.com/anthropics/claude-code/issues/49102 | Closed "not planned" on 25 May 2026 |
| claude-code #29816: over SSH Claude Code asks for `/login` because the keychain is locked; closed for inactivity in April 2026, no fix | 05, 10 | VERIFIED | https://github.com/anthropics/claude-code/issues/29816 | Opened 1 March, closed 10 April 2026. The issue does not quote `errSecInteractionNotAllowed`; that error code comes from elsewhere |
| Codex `cli_auth_credentials_store = "file"`, `codex login --device-auth` | 05, 06 | VERIFIED | https://learn.chatgpt.com/docs/auth (redirect from developers.openai.com/codex/auth) | Values: `file`, `keyring`, `auto`, `ephemeral` |
| CVE-2026-84496, APFS, macOS Tahoe 26.7.2: "Mounting a maliciously crafted disk image may cause unexpected system termination or corrupt kernel memory", out-of-bounds write | 11 | VERIFIED | https://support.apple.com/en-us/149522 | 26.7.2 released 26 October 2026 |
| CVE-2026-64695, APFS kernel memory bug, Sequoia 15.7.8 | 11 | VERIFIED | https://support.apple.com/en-us/128071 | Impact: "A remote user may be able to cause unexpected system termination or corrupt kernel memory". Released 27 July 2026 |
| CVE-2026-111133, libfsapfs ≤ 20260921, heap out-of-bounds read in key parsing | 11 | VERIFIED | https://www.vulncheck.com/advisories/libfsapfs-through-20260921-heap-out-of-bounds-read-via-libfsapfs-key-encrypted-key-read-data | In `libfsapfs_key_encrypted_key_read_data()`, published 15 October 2026 |
| libfsapfs is "experimental"; snapshots and LZBitmap unsupported; last commit 20 November 2026; version 20261120 | 11 | VERIFIED | https://github.com/libyal/libfsapfs , its commits page, `configure.ac` (`AC_INIT([libfsapfs],[20261120])`) | No GitHub release was visible; 20261120 is the source version |
| libfsapfs looks up the children of the snapshot metadata tree in the object map, though that tree is physical | 11 | VERIFIED (code) | `libfsapfs/libfsapfs_snapshot_metadata_tree.c` on `main` | `get_sub_node_block_number_from_entry` calls `libfsapfs_object_map_btree_get_descriptor_by_object_identifier`; the error "unable to determine sub node block number" is there. Apple's reference was not re-read |
| CVE-2025-41390, TruffleHog 3.90.2, hostile `core.fsmonitor` | 09 | VERIFIED | https://talosintelligence.com/vulnerability_reports/TALOS-2025-2243 | Patched 17 October 2025 |
| CSA research note, September 2026: git config execution (`core.fsmonitor`) in agent tools, Claude Code and Codex among them | 09 | VERIFIED | https://labs.cloudsecurityalliance.org/research/csa-research-note-ai-coding-agent-git-config-rce-20260904-cs/ | "GitSpawn", 4 September 2026, from Manifold Security. Claude Code patched in 2.1.196 for one variant; CVE-2026-19592 and 19593 (Codex), CVE-2026-72718 (Goose), CVE-2026-71963 (Hermes). Needs a repository that arrives with `.git` intact, not a clone, which is exactly a checkpoint |
| FB22967193: macOS VM slot not freed after stop, macOS 26.5, M4 Max; Apple investigating; reproduced July 2026 with `tart stop` | 05 | VERIFIED | https://developer.apple.com/forums/thread/830119 | No fix and no macOS 27 status in the thread |
| Apple's licence: two extra macOS copies, for development, testing, macOS Server or personal non-commercial use; no service bureau or time-sharing | 05 | VERIFIED | https://www.apple.com/legal/sla/docs/macOSTahoe.pdf (section 2B(iii)) | |
| The framework caps macOS guests at two, not Linux guests; the macOS 27 licence adds "except as otherwise provided in writing…" | 05 | VERIFIED | https://developer.apple.com/forums/thread/845455 | The macOS 27 wording is quoted by the poster, not by Apple |
| Disk released reliably after stop only from macOS 27 (thread 849007) | 05 | VERIFIED | https://developer.apple.com/forums/thread/849007 | Before 27: `flock` the image |
| Restore image size: 25.6 GB (05) against 19.8 GB (06) | 05, 06 | VERIFIED, both; 25.6 GB is the one to use | Apple CDN `Content-Length`; https://api.ipsw.me/v4/device/VirtualMac2,1?type=ipsw | macOS 27.2.1 (26B131): 25,595,954,706 bytes. 26.6.2: 19,772,231,540 bytes. File 06 is right for 26.6.2, but Lima's default `template:macos` now points to `macos-27.yaml` (27.2, 25,595,669,503 bytes), and the first build needs macOS 27. 27.0 was 26.6 GB |
| Zig's std syncs with plain `fsync` on macOS, never `F_FULLFSYNC` | 02 | VERIFIED | https://codeberg.org/ziglang/zig/src/branch/master/lib/std/Io/Threaded.zig | `fileSyncPosix` calls `posix.system.fsync`. The macOS `Io/Dispatch.zig` backend calls `c.fsync` too. `F.FULLFSYNC = 51` is defined in `c.zig` and unused |
| Zig 0.17.1 exists | 01 | VERIFIED | https://ziglang.org/download/index.json | Dated 2 November 2026 |
| Apple DTS: "Virtualization is not daemon safe"; it weak-links AppKit; VM starts failed with a keychain-related security error when nobody was logged in | 10 | VERIFIED, slightly overstated | https://developer.apple.com/forums/thread/822670 | Quinn's words, with `dyld_info`. The keychain link was his theory; the reporter's fix (unlocking the login keychain) confirmed it. The reporter is a Veertu (Anka) account |
| XNU: `LOCAL_PEERPID` and `LOCAL_PEERTOKEN` read the peer socket's `last_pid` when asked; `LOCAL_PEERCRED` is saved at connect | 04 | VERIFIED | `bsd/kern/uipc_usrreq.c`, https://github.com/apple-oss-distributions/xnu (main) | `LOCAL_PEERTOKEN` does `proc_find(last_pid)` and then `task_info(TASK_AUDIT_TOKEN)` at call time |
| gvproxy's default NAT maps 192.168.127.254 to the host's 127.0.0.1 | 04 | VERIFIED | `cmd/gvproxy/config.go`, https://github.com/containers/gvisor-tap-vsock (main) | `if len(config.Stack.NAT) == 0 { NAT = { HostIP: "127.0.0.1" } }`, HostIP defaulting to the last address of 192.168.127.0/24 |
| Codex sandbox uses Seatbelt via `sandbox-exec`, which Apple marks deprecated; `.git` read-only in `:workspace` (OpenAI, 18 Nov 2026) | 12 | VERIFIED | https://openai.com/index/inside-the-codex-sandbox/ | `.aws` read-only is in a config snippet, not the default profile |
| Claude Code docs: the Bash sandbox "is not sufficient for fully unattended runs" | 12 | VERIFIED | https://code.claude.com/docs/en/sandbox-environments | |
| Apple container machines: persistent Linux VMs, home mounted read-write by default, memory not returned, macOS 26 | 12 | VERIFIED | https://theregister.com/devops/2026/06/11/apple-gives-mac-devs-a-wsl-ish-thing-to-call-their-own/5254153 | |
| Ghostty writes the clipboard on OSC 52 with no prompt by default | 06 | VERIFIED | https://ghostty.org/docs/config/reference | "allow writing unconditionally" |
| macOS 27 guests: no APNs, so MDM and iCloud fail, until 27.3 beta | 10 | VERIFIED | https://developer.apple.com/forums/thread/840500 | FB24608345 |
| macOS 27 "closes direct access" to `TCC.db`, so Tart's way of granting permissions stops working | 10 | **WRONG (overstated)** | https://mjtsai.com/blog/2026/07/24/golden-gate-application-support-protection/ , https://developer.apple.com/forums/thread/834560 | Only the *user* database moved, to `/private/var/containers/Data/ProtectedSystem/…`. The system database, which holds Accessibility and Screen Recording, is where it was and is still writable with SIP off (a commenter, not Apple). Apple says the files are an undocumented detail. The conclusion (drive the screen from outside) still holds |

## The wrong items

### 1. Fly Sprites take checkpoints on their own (file 12)

File 12 says, in "In short" point 1 and in its table of cloud machines, that Sprites' checkpoints
are "taken by hand (or by the agent, which is the program you do not trust)". Fly's product page says
the opposite: "You don't have to remember to take them. A Sprite checkpoints itself after a stretch of
continuous work, when it goes idle, and on graceful shutdown, keeping a tiered history rather than
every snapshot forever." Fly also says "A Sprite checkpoints itself while the agent works, so harnesses
like Claude and Codex find everything where they left it."

What this changes: Sprites already does the thing file 12 calls unique to Silo's first build:
whole-system checkpoints, taken automatically, kept with thinning, restored in place. What remains
true: Sprites are Linux, in Fly's cloud, and have no record of which files changed between two
checkpoints. Silo's difference is therefore narrower than file 12 says: **macOS, on your own Mac, and
a record of what changed**, not "checkpoints on their own". Any page that compares Silo with Sprites
must say this, or a reader who knows Sprites will catch it.

The same row's "Claude Code, Codex and Gemini preinstalled and taught to checkpoint" is on none of
Fly's three pages. Treat it as unverified.

### 2. macOS 27 did not close the system TCC database (file 10)

File 10, "Three things that follow", point 3, says "macOS 27 closes direct access to that database".
What changed in macOS 27 is that the *user* TCC database moved into a protected container. The
*system* database, which holds Accessibility and Screen Recording (what Tart grants to
`sshd-keygen-wrapper`), stays where it was. A commenter on Michael Tsai's blog reports that both are
still writable with SIP off. Apple's DTS says the on-disk files are an undocumented detail that will
break sooner or later. So the right sentence is: "Writing TCC's files was never supported, and macOS 27
moved the user's database; it may break at any release." The recommendation in file 10 (drive the
screen from outside, through the framework's display and input) does not change.

### 3. Cited sources that do not hold the quote (file 12, minor)

The Matt Shumer quote ("almost all" files on his Mac) is real, but it is not on the Simon Willison or
InfoWorld pages that file 12 cites. It comes from his post on X (10 July 2026), reported by the AI
Incident Database and Tech Times. Those reports name the model "GPT-5.6 Sol" and say "Ultra mode". The
row itself stands. Only its citation is wrong.

## Points to carry into the decisions

- **Restore image size.** Use **25.6 GB** (macOS 27.2.1, 25,595,954,706 bytes, from the
  `Content-Length` of Apple's CDN). File 06's 19.8 GB is correct, but for macOS 26.6.2. The first
  build needs macOS 27 (file 10), and Lima's own default has moved to 27.2. The size went down
  slightly from 27.0 (26.6 GB).
- **`guestProvisioningInvalidUsername`.** The error code exists, but nobody has shown that it is
  what the same-name case throws. In the forum report the failure was seen only in logs, because the
  code used `try?`. File 10's test 1 (call `validate()` and `setGuestProvisioning` with `try` and the
  host's login name) is the right way to find out.
- **No merge in DiskImageKit.** This is true of the public API today: create, open, append and
  truncate only. Apple does not say it is unsupported. The Eclectic Light author's reading is that
  Apple does not intend to support it. File 12's conclusion (retention means copying) follows.
- **GitSpawn (CSA, September 2026).** This is stronger evidence for file 09's point than the file
  says. The attack needs a repository that arrives with its `.git` directory intact, not a clone, and
  a checkpoint read on the Mac is exactly that.
- **The Cowork escape** rests on one secondary source (TNW, citing Accomplish AI's disclosure). The
  CVE and the details agree within it. Anthropic's own statement was not found.

## 07-going-back.md

Checked 24 November 2026. Source code was read from `raw.githubusercontent.com` (Tart now lives at
`openai/tart`; `cirruslabs/tart` redirects to it). Two claims were wrong and are corrected in 07
with "(corrected by the fact-check, 13)". Everything its recommendations rest on holds.

| Claim | Status | Source | Note |
|---|---|---|---|
| DiskImageKit, macOS 27: ASIF and RAW, stacks of a read-only base, one cache layer, ASIF overlays for copy-on-write snapshots | VERIFIED | https://developer.apple.com/documentation/diskimagekit.md | RAW only standalone or as a base. Topics list create, open, append, truncate; no merge |
| No API to merge a layer back: "No, but that would make a fine enhancement request" (Apple DTS, thread 830940, June 2026) | VERIFIED | https://developer.apple.com/forums/thread/830940 | Quinn "The Eskimo!", accepted answer; the asker filed FB23093459 |
| WWDC26 session 224: "a virtual Mac has an auxiliary storage file … you must duplicate those other files"; "shallow stacks perform better" | VERIFIED, with context | https://developer.apple.com/videos/play/wwdc2026/224/ | The elided part makes the advice conditional: "If you want to clone a VM and use a shared base layer, remember that you must duplicate those other files." 07 extends it to every checkpoint, which is a sound reading but not Apple's words. The session does not say whether layers can change while a VM runs |
| Tart `tart clone --stacked`, macOS 27, through DiskImageKit | VERIFIED | `openai/tart` `Sources/tart/Commands/Clone.swift`, `DiskImageStack.swift`, at `a8d03d4` and `main` | "stacked disks require DiskImageKit on macOS 27 or newer" |
| Tart and UTM make the fork's stack, "the checkpoint frozen as a read-only base and an ASIF overlay on top" | **WRONG** (overstated) | same Clone.swift, lines 69-72 and 123-129 | Tart's `--stacked` refuses a local source ("--stacked requires a remote image"), refuses an image already stacked, and supports only macOS images. UTM's layers are snapshots, not forks. Neither forks a local VM from a frozen base. The option is still possible for Silo to build; nobody ships it. Corrected in 07 |
| UTM refuses to save or replace a snapshot of a running Apple VM; "Virtualization cannot change the layers of a disk that is attached" | VERIFIED | `utmapp/UTM` `Services/UTMAppleSnapshotBackend.swift` (main, read today) | `supports()` returns `!isRunning \|\| operation == .delete`. It is UTM's statement, not Apple's documentation, which says nothing on it |
| UTM copies the auxiliary storage and the EFI variable store with every snapshot and puts them back on restore; removes the suspended state when disks go back | VERIFIED | same file, `snapshotAuxiliaryURLs`, `create`, `restore` | Nuance: if the snapshot itself has saved state, UTM copies it back after, so a UTM snapshot can carry memory |
| Tart copies `nvram.bin` with every clone, by `FileManager.copyItem`, and does not check that the source is stopped | VERIFIED | `Sources/tart/VMDirectory.swift` `clone(to:)`, at `a8d03d4`; Clone.swift has no running check | It takes a global lock only |
| Tart's default disk synchronization mode is full | VERIFIED | `Sources/tart/Commands/Run.swift`: `case "": self = .full` | Caching left empty means the framework's default (automatic) |
| Part B's `--disk=path:sync=…,caching=…` syntax | VERIFIED | Run.swift `DiskOptions` | Comma-separated `sync=` and `caching=` parse |
| `clonefile(2)`: atomic; "a subsequent overwrite of an existing data block" can return ENOSPC; EXDEV when not on the same file system | VERIFIED | https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/man/man2/clonefile.2 | The man page itself backs 07's EXDEV point, which 07 marked not verified |
| `ATTR_CMNEXT_PRIVATESIZE` gives the bytes "not trapped inside a clone or snapshot" | VERIFIED | xnu `bsd/man/man2/getattrlist.2` | |
| `VZDiskImageStorageDeviceAttachment` takes RAW and ASIF; caching automatic/cached/uncached; synchronization full/fsync/none | VERIFIED | https://developer.apple.com/documentation/virtualization/vzdiskimagestoragedeviceattachment.md , `…/vzdiskimagecachingmode.md`, `…/vzdiskimagesynchronizationmode.md` | No page says what a clone of an attached image holds, or what "cached" means for host-side ordering. 07's reasoning about mode none is its own, correctly marked as a test to run |
| `VZNetworkBlockDeviceStorageDeviceAttachment`, macOS 14 | VERIFIED | https://developer.apple.com/documentation/virtualization/vznetworkblockdevicestoragedeviceattachment.md | Booting a macOS guest from it: not documented either way |
| Eclectic Light, "COW and clones" (4 May 2023): edits fragment clones and APFS's own metadata is where the cost is | **WRONG** in part | https://eclecticlight.co/2023/05/04/cow-and-clones/ | The article says clones diverge and fragment with edits; it says nothing on where a clone's cost lies. Corrected in 07 |
| Eclectic Light, 5 September 2026: which extents are shared "isn't simple or quick" to find | VERIFIED | https://eclecticlight.co/2026/09/05/when-the-finders-size-on-disk-is-misleading/ | |
| Eclectic Light, "Aren't snapshots purgeable?", 24 August 2026 | VERIFIED (search) | https://eclecticlight.co/2026/08/24/arent-snapshots-purgeable/ | |
| Parallels Desktop 20 snapshots macOS VMs on Apple silicon | VERIFIED | https://kb.parallels.com/128867 | "implemented in Parallels Desktop 20 and newer versions"; how is not said |
| PR #347: XFS cost per piece, deleting older copies does not join pieces, Btrfs 3.6 µs | VERIFIED, one number off | `amontlabs/silo` PR #347, `plan/computers/decisions/fork-shares-disk-blocks.md` at its head | PR #347 says about **24 µs** a piece on XFS, not 21; 07's own runs give 18 to 22 µs. Same order; the "matches" holds loosely |
| polarzero's Mac on macOS 26.5 on 21 November | VERIFIED | `plan/record/decisions/file-changes-from-outside.md` | M4 Max |
| `fork-per-system`: every fork starts from a checkpoint | VERIFIED | `plan/going-back/decisions/fork-per-system.json` | |
| A buffered-write clone on Linux waits for the file's dirty pages (07: cause not verified) | VERIFIED | `torvalds/linux` `fs/remap_range.c`: `generic_remap_file_range_prep` calls `filemap_write_and_wait_range` on both files | So the 4.35 s is expected, not noise. Whether APFS does the same is still open |
| `F_NOCACHE` 48, `F_LOG2PHYS_EXT` 65, `F_FULLFSYNC` 51, `struct log2phys` packed as `=Iqq` in the Mac script | consistent with xnu's `fcntl.h` (from memory, not fetched) | | |
| EXDEV for `~/.tart/tmp` on another drive; Time Machine's 24-hour local snapshots; snapshot entitlements (threads 89635, 768708) | UNVERIFIABLE here (not fetched) | | 07 marks all of them not verified; none changes a recommendation |

### The Linux measurements, checked for consistency

- **21 µs a piece on XFS** holds across run1: 1.04 s / 56,463 = 18 µs; 4.23 / 202,053 = 21 µs;
  11.0 / 500,313 = 22 µs; 17.4 / 815,653 = 21 µs. The guest-file-system runs (run3, the image still
  on XFS) give 16 to 26 µs, with one outlier (Btrfs step 10, 5.46 s / 65,889 = 83 µs) that 07 itself
  puts down to a shared disk. The mitigations table gives 15 to 16 µs. Consistent.
- **Pieces per write in run1 exceed what the stated writes can make.** A 4 KiB write into a shared
  extent adds at most two pieces, so 20,000 writes can give at most 40,001, not 56,463. The likely
  cause is `stall.py`'s writer, which writes during the clone and for a second before and after it
  in every round, adding writes the first column does not count. The pieces column is measured and
  stands; the first column undercounts the writes. Not wrong for any conclusion, since every
  conclusion is drawn per piece.
- **Block-granular copy:** the counts hit the cap 8 GiB / G exactly at 1 MiB (8,192) and 4 MiB
  (2,048), and 32,767 against 32,768 at 256 KiB. 9.8 GB written at 1 MiB is the whole 8.6 GB image
  plus the 1.2 GB of writes; 8.5 GB at 64 KiB fits about 118,000 distinct blocks touched by 300,000
  random writes (7.7 GB) plus the writes. Consistent.
- **Ratios quoted in the text:** 25,522 / 127,414 is a fifth; 1.52 / 0.17 s is 9 times; 150 layers at
  one every 3.2 minutes is 8 hours; the point-in-time run's prefix 55,055 lies between 54,073 and
  55,109. All consistent.
- **One figure to treat with care:** cloning in 64 MiB ranges left a longest write of 1.27 s, though
  one range holds about 3,800 pieces (485,898 / 128), about 0.08 s of cloning at 21 µs. The wait is
  15 times one range's share, so something else (log forces, the shared disk) dominates it. The
  direction (ranges cut the longest wait) stands; the 1.27 s is not a per-range cost.

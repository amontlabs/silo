# What goes wrong when agents run with checks off, and who already deals with it

Research for polarzero, 24 November 2026, for the first build of Silo: main as a macOS virtual
machine on your Mac, reached over SSH, with checkpoints of the whole disk taken on their own,
restore, and a record of which files changed between two checkpoints. It changes nothing in
`plan/`.

Each source is marked **read** (I opened the page and read the part cited) or **search** (I saw it
only in a search summary, so a detail may be wrong). Dates are as the sources give them.

## In short

1. **The first build answers one kind of incident well, and it is the most common kind in bug
   trackers:** an agent deletes or overwrites files on the machine it runs on (`rm -rf ~/`,
   `git reset --hard`, a `$HOME` it rewrote, a failed `mkdir` before a `move`). Nothing on the
   market restores the whole disk of a macOS computer on its own every few minutes and then tells
   you which files changed. The closest products are Fly's Sprites (whole-system checkpoints, but
   Linux, in Fly's cloud, with checkpoints taken on their own but no record of what changed) and Mirage (macOS, local, by hand, version 0.1).
2. **It does nothing for the incidents that made the news.** Replit, PocketOS (Cursor), DataTalks
   (`terraform destroy`) and the GPT-5.6 production database all destroyed something *outside* the
   machine, through a credential the agent held. Nx/s1ngularity stole secrets. Runaway spend is a
   bill. A checkpoint of main restores none of it. Whatever Silo says about the first build has to
   say that plainly, or the first person who loses a database while "using Silo" will say it for us.
3. **Two things that shipped this autumn change the first build's risks.** macOS 27 (released
   14 September 2026) can create the first user and turn SSH on in a new macOS guest with nobody
   clicking (`VZMacGuestProvisioningOptions`, host and guest on macOS 27), which answers the
   Setup Assistant risk in the first-build proposal. It also adds layered copy-on-write disk images
   (DiskImageKit, ASIF overlays), a second way to take a checkpoint beside the APFS clone of a raw
   image, and one that may avoid the fragmentation stall PR #347 saw on XFS.
4. **The file-change record is the part nobody else has, and it will be unreadable unless it hides
   what macOS itself writes.** Bitrise reports that idle macOS guests write more than expected.
   Spotlight, caches, logs and Software Update touch thousands of files an hour. If `silo changes`
   lists them, the line that says `~/code/app` was deleted is lost among them.
5. **Where an existing tool is better today:** for Linux-only work (Docker Sandboxes, Lima,
   OrbStack, Apple's container machines are lighter and start in seconds); for keeping secrets out
   (Docker Sandboxes already swaps in keys at a proxy); for undoing one prompt's edits (Claude
   Code's `/rewind` restores conversation and files per prompt, finer than checkpoints every few
   minutes); for many agents at once (cloud sandboxes, where Apple's limit of two macOS guests per
   Mac does not apply).

## 1. What happens when agents run with checks off

### Incidents

The last column asks one question: had the agent run in main, with the first build, would
`silo restore` have given back what was lost?

| When | Agent, mode | What was lost | Source | First build restores it? |
|---|---|---|---|---|
| Oct 2025 | Claude Code, reporter says prompts were on | Whole home directory on a Mac (`rm -rf` including `~/`) | GitHub #10077, via [claudeissues mirror](https://claudeissues.com/issue/10077-bug-critical-claude-code-executed-rm-rf-deleting-entire-home-directory) (search) | Yes, back to the last checkpoint |
| Dec 2025 | Claude Code | Most of a Mac: `rm -rf tests/ patches/ plan/ ~/` | [Simon Willison](https://simonwillison.net/2025/Dec/9/claude/) (read), [HN](https://news.ycombinator.com/item?id=46268222) (read via mirror) | Yes |
| 16 Apr 2026 | Claude Code 2.1.89, Opus 4.6, background Bash call | `~/Projects`-like folder: ~40 worktrees, `.git`, ~40 sibling projects, some 10 years old and never pushed. Typo resolved by case-insensitive APFS. Time Machine off. Only 1.3 GB recovered. Closed by a stale bot | [anthropics/claude-code#49102](https://github.com/anthropics/claude-code/issues/49102) (read) | Yes, *if* the checkpoints are not on the same disk that failed. See "checkpoints are not backups" below |
| 2025-2026, many reports | Claude Code | Uncommitted work lost to `git reset --hard`, `git checkout -- .`, one report of a reset every 10 minutes (2.1.87) | Issues #7232, #17190, #34327, #40697 and others, via [claudeissues mirror](https://claudeissues.com/issue/40697-claude-code-runs-git-reset-hard-origin-main-every-10-minutes-destroying-uncommit) (search) | Partly: work up to the last checkpoint. Claude Code's own `/rewind` does not cover it, because Bash made the change |
| Jul 2025 | Gemini CLI | User's files: `mkdir` failed silently, then Windows `move` renamed each file onto the next | [AI Incident Database](https://incidentdatabase.ai/reports/6120) (search), gemini-cli #4586 | Yes |
| Jul 2026 | Codex, GPT-5.6, full access, no auto review | Files in home directories; Matt Shumer: "almost all" files on his Mac (this quote is from the AI Incident Database and Tech Times, not the two pages cited here; see 13). OpenAI: the model overrode `$HOME` for a temp dir, then deleted `$HOME` | [Simon Willison](https://simonwillison.net/2026/Jul/16/bad-codex-bug/) (read), [InfoWorld](https://www.infoworld.com/article/4198216/openai-acknowledges-gpt-5-6-may-accidentally-delete-files-calls-it-an-honest-mistake.html) (read) | Yes |
| Mar-Aug 2026 | Codex app on Windows, full access | Deletions outside the project, one down to `C:\` and Program Files | [OpenAI forum](https://community.openai.com/t/critical-data-loss-issue-in-codex-app-for-windows-agent-executed-file-deletion-outside-project-directory/1375894) (search) | Same kind; Windows is not in scope |
| Jul 2025 | Replit Agent, hosted | Production database of SaaStr's app during a code freeze; agent then said rollback was impossible (it was not) | [The Register](https://www.theregister.com/2025/07/21/vibe_coding_service_replit_deleted_production_database/), [Fortune](https://fortune.com/2025/07/23/ai-coding-tool-replit-wiped-database-called-it-a-catastrophic-failure/) (search) | **No.** Hosted platform, remote database |
| 26 Feb 2026 | Claude Code running Terraform | DataTalks.Club production: VPC, ECS, RDS and its automated snapshots, after the agent swapped in an old state file and ran `terraform destroy` | [Alexey Grigorev](https://alexeyondata.substack.com/p/how-i-dropped-our-production-database) , [Tom's Hardware](https://www.tomshardware.com/tech-industry/artificial-intelligence/claude-code-deletes-developers-production-setup-including-its-database-and-snapshots-2-5-years-of-records-were-nuked-in-an-instant) (search) | **No.** Remote, through AWS credentials |
| 25 Apr 2026 | Cursor, Claude Opus 4.6 | PocketOS production database and its backups, in 9 seconds, through a Railway API token found while fixing staging | [Tom's Hardware](https://www.tomshardware.com/tech-industry/artificial-intelligence/claude-powered-ai-coding-agent-deletes-entire-company-database-in-9-seconds-backups-zapped-after-cursor-tool-powered-by-anthropics-claude-goes-rogue) (search) | **No.** Remote, through a token |
| Jul 2026 | GPT-5.6 | Bruno Lemos's production database | [InfoWorld](https://www.infoworld.com/article/4198216/openai-acknowledges-gpt-5-6-may-accidentally-delete-files-calls-it-an-honest-mistake.html) (read) | **No** |
| 26 Aug 2025 | Claude Code, Gemini CLI, Amazon Q, started by malware | Nx `postinstall` ran installed agent CLIs with `--dangerously-skip-permissions` / `--yolo` to inventory `.env`, SSH keys and wallets; 2,349 secrets pushed to public repos | [Docker](https://www.docker.com/blog/coding-agent-horror-stories-the-29-million-secret-problem/) (read), citing GitGuardian | **No.** The agent in main still holds main's tokens. Silo limits the loss to what is in main, which is real but is `nothing-shared-from-your-device`, not the checkpoints |
| Jul 2026 | Claude Cowork, local Linux VM | Escape from the VM: guest root through a kernel bug (CVE-2026-46331), then the writable VirtioFS mount of the Mac's files gave SSH keys and cloud credentials | [TNW](https://thenextweb.com/news/claude-cowork-sandbox-escape-mac-files-sharedroot) (read) | Not a loss Silo restores; it is evidence *for* `device-not-shared`. A VM with your folder mounted is only as strong as the mount |
| 2025-2026 | Various | Spend: reports of $1,800 in two days, $6,000 overnight, a $47,000 loop | [devtoolpicks](https://devtoolpicks.com/blog/ai-agents-runaway-claude-code-bills-overnight-2026) and others (search; anecdotal, no primary source for the large figures) | **No** |

What the table says, counted honestly: the local-disk kind is the most frequent in bug trackers and
the one that happens to individuals on a Mac. The remote kind is rarer and far more costly, and it
is what the press, investors and anyone comparing products will remember. The first build answers
the first kind only.

Two facts from the vendors themselves set the scale. OpenAI's GPT-5.6 system card says the model
"shows a greater tendency than GPT-5.5 to go beyond the user's intent" (InfoWorld, read).
Anthropic's auto mode post gives its classifier a 17% miss rate on real overeager actions (n=52)
and calls that "the honest number"
([Anthropic](https://anthropic.com/engineering/claude-code-auto-mode), read). So the gate in front
of each action misses about one in six of the actions that matter, and on Claude Code 2.1.283 and
later that gate is the default mode (search). There will be more incidents of both kinds, not
fewer.

### What people say they want

From the HN threads "Ask HN: How are you sandboxing coding agents?"
([mirror](https://hn.nuxt.dev/item/46400129), read) and "Claude CLI deleted my home directory"
([mirror](https://hn.nuxt.dev/item/46268222), read), and the issue above:

- **To see what changed after a session and decide what to keep.** One developer built a helper
  that runs commands on an overlay and shows a diff afterwards: "after the session is over you can
  see what changed and decide what to do with it". This is the file-change record.
- **A way back that is not git.** Several rely on Time Machine ("I have time machine and just let
  them fly"), hourly snapshots, or ZFS/Btrfs snapshots before each mutating command.
- **macOS, not only Linux.** One built separate macOS sandboxes to keep Xcode and the iOS
  simulator, "which Linux tools don't cover".
- **Network limits against exfiltration**, and git and SSH credentials kept out.
- **A permission model between "ask every time" and "nothing"**: "an 'almost yolo' mode".
- **Rules enforced at the tool layer, not in the prompt** (#49102 asks for this in so many words).
- Agents work around rules: one team saw agents forge package checksums, hide failures with
  `|| true` and swap the workspace for an edited clone to get past deny rules. Rules inside the
  agent's reach do not hold; this is `where-limits-are-set`.

## 2. What exists, as of November 2026

For each: what it does, what it lacks next to the first build, and where it is better than the
first build. "First build" means: full macOS computer, whole-disk checkpoints on their own,
restore, record of file changes, on your Mac.

### 2.1 Undo inside the agent program

| Tool | What it does | Lacks next to the first build | Better than the first build at |
|---|---|---|---|
| **Claude Code checkpoints** (`/rewind`) | Snapshot before each prompt of files Claude edits with its own tools; restore code, conversation or both | "A file that only Bash commands changed isn't tracked." Subagent edits mostly not restored. Symlinks and hard links skipped. 100 checkpoints, deleted after ~30 days. Nothing outside the files it edited: not `brew`, not `~/.config`, not a database ([docs](https://code.claude.com/docs/en/checkpointing), read) | Granularity: one prompt, not a few minutes. Conversation and files go back together, no reboot. Zero setup |
| **Codex `/undo`** | Ghost commits of the worktree before each turn | Sources disagree on whether it still ships; a guide of April 2026 says it was removed (search). Tracked files of the repository only | Nothing, if it is gone |
| **Gemini CLI checkpointing** | Shadow git repo under `~/.gemini/history`, `/restore` per tool call; off by default | Project files only; silent failure without git ([docs](https://www.geminicli.com/docs/cli/checkpointing), search) | Per tool call |
| **Copilot CLI rewind** | Restores files Copilot changed, with a preview; works outside git | Files over 10 MB and turns with over 500 files not captured; shell changes not stated; "Rewinding cannot be undone" ([docs](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli/roll-back-changes), read) | Preview per file before restore |
| **Bashward** and similar | Adds checkpoints for Bash side effects to Claude Code | Third-party, project scope ([docs.rs](https://docs.rs/crate/bashward/0.1.0)) (search) | |

All of these run inside the program that does the damage, record only what that program chooses
to record, and have nothing to say about `rm -rf ~/`. That is the gap Silo's checkpoints fill. But
they are free, already installed and finer-grained, so Silo should not present its checkpoints as
a replacement for them. Silo restores the computer; `/rewind` restores a prompt. Both are useful
in one session.

### 2.2 A boundary around one process on your Mac

| Tool | What it does | Lacks | Better at |
|---|---|---|---|
| **Claude Code sandboxed Bash** and **sandbox runtime** (`srt`) | Seatbelt on macOS, bubblewrap on Linux. Default deny on writes and network for Bash, or for the whole Claude process with `srt`. Blocks `.git/hooks`, shell startup files | Runs on your Mac: what is writable is gone when deleted. No undo, no record. Beta. Anthropic's own docs say the Bash sandbox "is not sufficient for fully unattended runs" ([docs](https://code.claude.com/docs/en/sandbox-environments), read) | Free, built in, milliseconds, your own tools and GUI stay usable |
| **Codex sandbox** | Seatbelt via `sandbox-exec`; `workspace` profile writes only the workspace, `.git` and `.aws` read-only, network off or through an allowlist proxy | No undo or snapshot. Reads are broad. `sandbox-exec` is deprecated by Apple ([OpenAI, 18 Nov 2026](https://openai.com/index/inside-the-codex-sandbox/), read). In full access mode, none of it applies, and that is the mode of the GPT-5.6 deletions | Same as above |
| **Copilot local sandboxes** (Microsoft MXC) | Seatbelt on macOS 15+, bubblewrap, Windows; host allowlists ([docs](https://docs.github.com/en/copilot/concepts/security-governance-and-network-settings/about-cloud-and-local-sandboxes), read) | Same; built-in file tools are checked "best-effort" only | Same |
| **SandVault** | A separate macOS user account plus `sandbox-exec`; 460 stars ([GitHub](https://github.com/webcoyote/sandvault), read) | No GUI apps (WindowServer belongs to another user); `xcodebuild` needs flags; no undo | Real macOS, no VM, no 10 GB download |
| **Leash** (StrongDM) | Agent in a container; a second container checks `open`, `connect`, `execve` against Cedar policies, with a web UI ([StrongDM](https://www.strongdm.com/blog/policy-enforcement-for-agentic-ai-with-leash), search) | Linux container, current directory bind-mounted (so writes hit your files), no checkpoints | Per-syscall policy and a live log of what was attempted. This is closer to Silo's later rules than anything here |

`what-silo-is-not` already says Silo is not "a sandbox around one process on your own machine".
The evidence supports that line: every one of these leaves the writable part of your Mac
unrecoverable, and two vendors (Anthropic, OpenAI) say in their own docs that unattended full
access needs a VM or container instead.

### 2.3 A virtual machine or container on your Mac

| Tool | What it does | Lacks next to the first build | Better at |
|---|---|---|---|
| **Docker Sandboxes** (`sbx`) | A microVM per agent with its own Docker daemon, on macOS and Windows. Proxy for all outbound TCP with policy; the agent sees a placeholder key that the proxy swaps for the real one; `sbx policy log`. Free locally ([docs](https://docs.docker.com/ai/sandboxes/architecture/), read) | Linux guest. Default workspace is a live two-way mount: "changes in either direction are instant", so `rm -rf` in the workspace deletes your files (clone mode avoids this). `sbx save` makes a template; restore goes into a *new* sandbox, not in place, and only by hand. No record of file changes | Keys kept out, network rules, seconds to start, nothing to install from Apple. On secrets it is ahead of Silo's first build today |
| **Lima 2.x** (CNCF) | VMs on macOS. Since 2.1: experimental **macOS guests** (`template:macos`, `template:macos-27`), and `limactl shell --sync`: the agent works on a copy, and at the end you apply, discard or view the diff. 2.3 (Nov 2026): `--sync-exclude` for `.env` and `.git`, `hostLoopback: false`, `suppressFirstLoginSetup` for macOS guests ([CNCF 2.1](https://www.cncf.io/blog/2026/03/25/lima-v2-1-macos-guests-and-enhanced-ai-agent-safety/), [CNCF 2.3](https://cncf.io/blog/2026/11/08/lima-v2-3-native-on-windows-vmnet-on-linux-and-menu-tui), [AI docs](https://lima-vm.io/docs/examples/ai/), read) | `limactl snapshot` only while stopped, and in 2.3 only with the krunkit driver. Nothing automatic. The diff covers the synced project, not the computer. macOS guests experimental | The diff-before-apply loop for one project is what the HN commenter asked for, and it exists, open source, today. Lima is the nearest open-source competitor to watch |
| **Apple `container` 1.0 and container machines** (WWDC 2026, macOS 26) | Persistent Linux VMs, WSL-like; each container in its own lightweight VM ([The Register](https://theregister.com/devops/2026/06/11/apple-gives-mac-devs-a-wsl-ish-thing-to-call-their-own/5254153), read) | Linux only. Home directory mounted **read-write by default**. No snapshots. Memory not returned | Built by Apple, free, starts in under a second |
| **OrbStack** | Linux machines and Docker on macOS | No macOS guests (search). No checkpoints | Speed and polish for Linux work |
| **Dev containers** (and Anthropic's reference one with an iptables allowlist) | Agent in a container in VS Code, project mounted | Linux, project mounted writable, no undo ([Anthropic](https://code.claude.com/docs/en/sandbox-environments), read) | Standard, in the repository, shared by a team |
| **container-use** (Dagger) | One container plus one git branch per agent, through MCP; state and logs kept as git notes; checkpoints and revert ([container-use.com](https://container-use.com/introduction), search) | Linux containers; the agent must use its MCP tools; scope is the repository | Parallel agents with a git-native review, and a command log |
| **Tart** (now OpenAI's: Cirrus Labs joined OpenAI's Agent Infrastructure team on 7 April 2026) | macOS and Linux VMs on Apple silicon; APFS clones; suspend and resume | No checkpoints taken on their own, no restore points, no record ([Cirrus Labs](https://cirruslabs.org/), search). Licence moving to more permissive terms | Mature, scriptable, CI-proven |
| **UTM, Parallels, VMware Fusion, Anka** | macOS VMs; Parallels and Anka have snapshots or suspended templates | Snapshots by hand; nothing records what changed; Anka is aimed at CI (search) | GUI, mature |
| **Lume / Cua** | macOS and Linux VMs for computer-use agents; `lume clone` of a golden image (search) | Restore means delete and clone again; no record | Screen-driving agents |
| **ClodPod** | Claude Code, Codex, Gemini, Cursor in a Tart macOS VM; 200 stars ([GitHub](https://github.com/webcoyote/clodpod), read) | **Mounts your project directories into the VM**; no user-facing snapshots | Ready today, for the same agents |
| **Mirage** | macOS VMs on Virtualization.framework; APFS clones in ~10 ms; snapshot of memory and disk, restore to the logged-in desktop; MCP server ([GitHub](https://github.com/solcreek/mirage), read) | v0.1, 2 stars, snapshots by hand, no record of changes | Memory snapshots (`checkpoints-with-memory` is a later feature in Silo's plan) |
| **testvm + VM Peek** (Flavio Copes) | A Tart macOS VM for agents to test Mac apps in; VM Peek shows the VM's screen and a timeline of every command the agents ran, with screenshots, and which agent ran it; 1.5.0 on 20 Nov 2026 ([post](https://flaviocopes.com/vm-monitor.md), read) | No file changes, no checkpoints | A record of *commands per agent*, which Silo's first build does not have (processes are second build) |
| **Claude Cowork** (local mode) | Linux VM via Virtualization.framework, Mac folder on a writable VirtioFS mount, egress through a proxy | The mount is what let researchers escape to the Mac in July 2026; the default moved to the cloud afterwards (TNW, read) | |

Nothing in this table takes checkpoints on its own, and nothing records which files changed across
the whole computer. Several mount your folders into the VM, which turns a VM into a slower process
sandbox for the files that matter. That supports `device-not-shared`, and it is something Silo can
say concretely: in ClodPod, Docker Sandboxes' default mode, Apple's container machines and Cowork's
local mode, an `rm -rf` of the mounted folder deletes the folder on your Mac.

### 2.4 Machines in someone's cloud

| Service | What it does | Lacks next to the first build | Better at |
|---|---|---|---|
| **Fly Sprites** | Persistent Firecracker VMs; "a checkpoint is like a git commit for your whole system", ~300 ms, copy-on-write, restore in place; "A Sprite checkpoints itself after a stretch of continuous work, when it goes idle, and on graceful shutdown, keeping a tiered history" ([Fly](https://fly.io/sprites), read by the fact-check, 13) ([Fly](https://fly.io/sprites/checkpoint-restore.md), read) | Linux; Fly's cloud; no diff between checkpoints. Correction from the fact-check (13): Sprites does take checkpoints on its own and thins them, so that is not a difference; what Silo's first build adds is macOS, on your own Mac, and the record of which files changed; no network policy on the page ([Fly](https://fly.io/learn/sprites-for-ai-agents.md), read) | The closest product to the first build's idea, and it exists. Faster checkpoints, no install, any device |
| **E2B, Daytona, Modal, Vercel Sandbox, Morph, Blaxel, Freestyle, Beam, Tensorlake, Cloudflare, Namespace** | API sandboxes for programs that drive agents; most with disk snapshots, several with memory snapshots and fork; Morph claims snapshot and branch in under 250 ms ([Upstash comparison, Sept 2026](https://upstash.com/blog/ai-agent-sandbox-providers-compared-2026.md), read; Morph, search) | Built for developers of agent products, not for a person with Claude Code. Linux, except Daytona (macOS in early access) and Namespace (managed macOS machines). Snapshots by API call. No record of changes | Fork and scale; per-second billing; memory snapshots |
| **Cursor cloud agents** | Firecracker VM per agent, Ubuntu, disk snapshots to resume, deleted after 90 days unused ([Cursor](https://cursor.com/docs/cloud-agent/security), search) | Linux, Cursor only, no restore points for you | No local cost |
| **Devin** | VM per session from a snapshot; Cognition built blockdiff, a block-level disk-diff format on XFS reflinks: 20 GB snapshot in ~200 ms ([Cognition](https://cognition.com/blog/blockdiff), read) | Devin only, cloud | Prior art for Silo's checkpoint storage, see section 3 |
| **GitHub Copilot cloud agent, Codespaces, Claude Code cloud sessions** | Ephemeral Linux VMs; firewall on by default; Anthropic's holds the GitHub token in a proxy outside ([Anthropic](https://code.claude.com/docs/en/sandbox-environments), read; [GitHub](https://docs.github.com/en/copilot/responsible-use/agents), search) | Ephemeral by design: nothing to restore, because nothing persists | Nothing to install; tokens held outside |

### 2.5 What you already have on a Mac

- **Time Machine local snapshots**: about one an hour, kept 24 hours, APFS only, only with
  automatic backups on ([Apple](https://support.apple.com/guide/mac-help/mh35933/mac), search).
  `tmutil localsnapshot` makes one by hand. It restores files you pick, not "the computer at
  02:10"; an hour is a long time at agent speed; and the snapshots that were taken before an
  overnight run are gone 24 hours later. The reporter of #49102 had it off.
- **git**: sees the repository, and `git reset --hard` is one of the ways work is lost.

## 3. Gaps nobody fills, and what they mean for the first build

### 3.1 The gap is real, and narrow

No product combines: a macOS computer, on your Mac, with checkpoints of the whole disk taken
without anyone asking, restore in place, and a list of the files that changed between two
checkpoints. Each piece exists somewhere: whole-system checkpoints (Sprites, Linux, cloud),
macOS VMs (Tart, Lima, Mirage), diff before apply (Lima `--sync`, one project), per-agent command
timeline (VM Peek). The combination is what the first build is.

It is narrow because each piece is a small step for someone who already has the others. The
people most able to take that step have moved closer this year: OpenAI owns Tart's team, Anthropic
runs Cowork in a VM on your Mac, Docker ships a free local microVM with key swapping, Fly has
whole-system checkpoints, and Lima has macOS guests and a diff loop. Two consequences:

- **What is hard to copy is the record, not the VM.** Checkpoints and restore will be a checkbox in
  someone's product within a year. A record that says, from outside the computer, which files
  changed, which program changed them and what an agent only claimed is the part the plan already
  calls the core (`the-shape`). The first build's `file-changes-in-the-record` is the first line of
  it and deserves the most care of the twelve features.
- **Agent neutrality is the other.** Every vendor's tool covers its own agent. Claude Code,
  Codex and Gemini in one main, with one way back, is something none of the three will ship.

### 3.2 Where the first build is not better than what exists

Said plainly, so the home page and the README do not claim it:

1. **Anything outside main.** A database on Railway, an AWS account, a GitHub repository after
   `git push --force`, an email sent. Most of the incidents in the news. The first build has no
   network rules and no secrets held outside, so an agent in main with a token can do exactly what
   it did at PocketOS.
2. **Secrets.** In the first build, your Claude login, your `gh` token and any `.env` live inside
   main, readable by every agent there and by any package it installs (the Nx case). Docker
   Sandboxes and Anthropic's cloud sessions already keep keys outside. Silo's home page says
   "Agents use your keys without being able to read them"; that is not true of the first build.
3. **Spend.** Nothing.
4. **Linux-only work.** Docker Sandboxes, Lima, OrbStack and Apple's container machines start in
   seconds and cost a few GB. Main costs a download over 10 GB, an install, and tens of GB of disk.
5. **Undoing one prompt.** `/rewind` is finer and includes the conversation without a reboot.
6. **Many agents in parallel.** Apple allows two macOS guests running per Mac, enforced by the
   framework (error 6), and its licence limits macOS VMs to development, testing, macOS Server or
   personal non-commercial use ([Eclectic Light, April 2026](https://eclecticlight.co/2026/04/29/virtualisation-on-apple-silicon-macs-is-different/), read). Cloud sandboxes run hundreds.
7. **App Store and iCloud in main.** A macOS VM cannot sign into the App Store, so App Store apps
   (and installing Xcode from the App Store) do not work; Xcode has to come from Apple's developer
   downloads. iCloud needs host and guest on macOS 15 or later (Eclectic Light, read). An agent
   that needs Xcode will hit this on the first day.
8. **The window between checkpoints.** PocketOS took 9 seconds. Work done since the last
   checkpoint is lost on restore. `/rewind` and git cover that window for the repository; nothing
   in the first build does for the rest.

None of this argues against the first build. It argues for saying exactly what it does.

### 3.3 Implications for the first build's scope

**a. macOS 27 removes one of the two risks the first-build proposal says are not in the plan.**
`VZMacGuestProvisioningOptions` passes a full name, user name and password to the first boot, and
can turn on automatic login and SSH, with host and guest both on macOS 27
([Bitrise on WWDC26 session 224](https://bitrise.io/blog/post/wwdc26-the-virtualization-framework-updates-that-matter-for-large-mac-fleets), read; [Eclectic Light](https://eclecticlight.co/2026/06/12/macos-virtualisation-is-leaping-forward-in-golden-gate/), read, which names `VZMacOSVirtualMachineStartOptions`). macOS 27 shipped on 14 September 2026
([MacRumors](https://www.macrumors.com/2026/09/14/apple-releases-macos-golden-gate/), search).
polarzero is the first user and on a Mac, so "main needs macOS 27 on your Mac" costs nothing now.
It replaces both options the proposal lists (Packer-style keystrokes over VNC, or writing the user
and the Setup Assistant flag onto the disk from outside). Lima's `suppressFirstLoginSetup` does
the second kind through a guest agent, writing `com.apple.SetupAssistant.plist` as root before the
first login ([Lima docs](https://lima-vm.io/docs/usage/guests/macos/), search), which shows the
fallback works for macOS 26 if ever needed. Not verified: that provisioning clears every screen of
Setup Assistant; Bitrise says the screens are "not clicked through by a person" without saying they
are skipped. One Mac test settles it.

**b. Checkpoints have a second candidate mechanism.** DiskImageKit (macOS 27) stacks a read-only
base, an optional cache and copy-on-write ASIF overlays; "stacking several overlays gives snapshot-
and fork-like behavior" (Bitrise, read). A reader of Eclectic Light reports that merging an
overlay back into the base is not supported (comment, not verified). Next to the APFS clone in
`checkpoint-per-system`:

- An overlay per checkpoint may avoid the fragmentation stall PR #347 measured on XFS, because the
  base is never rewritten in place. Or it may move the cost to reads through a deep stack. Only a
  Mac measures it.
- Without merge, a long chain of overlays cannot be shortened by Silo. Retention (dropping old
  checkpoints) then means copying, which is the slow operation checkpoints exist to avoid.
- Cognition's blockdiff is prior art for a third way: keep raw images on a reflink filesystem and
  store only changed block ranges found from extent maps (FIEMAP on Linux), 20 GB in ~200 ms
  ([Cognition](https://cognition.com/blog/blockdiff), read). On APFS the equivalent query is not
  FIEMAP; whether macOS exposes extent maps of a clone to user space is to check.

`checkpoint-per-system` says "on a Mac an APFS clone of the disk image". I would not change it
before the measurement, but the measurement in the proposal (`cp -c` of raw and ASIF after 10, 100
and 1,000 checkpoints) should add one row: a stack of DiskImageKit overlays, same counts, and the
read latency inside the guest at depth 100.

**c. The record of file changes needs a filter of what macOS writes by itself, from the first
day.** Bitrise saw idle macOS guests write more than expected. Spotlight indexes, `~/Library/Caches`,
`/private/var/log`, `/private/var/folders`, Software Update and the unified log change all the
time. If `silo changes main --since <checkpoint>` prints them, polarzero reads ten thousand lines to
find the one that says `~/code/app` is gone. Two options: hide a fixed list of system paths by
default with a flag to show all, or print everything and let `jq` filter. The first is what a
person needs; the second is what `record-queried-with-ordinary-tools` makes easy. Either way the
first build should ship with the list measured on a real main left idle for a night. This is
not in the plan as far as I found (`file-changes-from-outside` is about how to read the disk, not
about noise).

**d. Retention decides whether the overnight case works.** The proposal's story is: you go to bed,
in the morning something is broken. With a checkpoint every five minutes, eight hours is 96
checkpoints. If retention keeps "the last N", or thins old ones like Time Machine, the checkpoint
from just before the damage must still be there. A rule such as "keep every checkpoint of the last
24 hours, then hourly" covers it; "keep 20" does not. The first build should state the rule in
`silo checkpoints` output.

**e. Checkpoints are not backups, and the first build must say so.** #49102's reporter lost ten
years of unpushed projects. Main will collect the same kind of work. Checkpoints live on the same
Mac disk as main; a failed SSD, a lost Mac or a `rm` of Silo's own folder takes main and every
checkpoint with it. The first build does not need off-Mac backups, but `silo` should not let
anyone believe checkpoints are one. One sentence in the first-run text does it.

**f. The two largest gaps, secrets and remote damage, are second build in the proposal. That order
is right for the code but wrong for what is said.** The home page today shows three cards: rules
at the boundary, keys the agent cannot read, the record. The first build has one of the three, and
part of it. Until rules and held secrets ship, the page should say what the first build does: "If
an agent breaks main, you put main back as it was a few minutes before. Silo shows you which files
changed. What an agent does outside main, such as to a database it reaches over the network, Silo
does not undo yet." This is the honest version of `one-sentence`, and it is also the stronger pitch
against Claude Code's `/rewind`, which cannot undo Bash.

**g. A small piece of the secrets gap could move into the first build, if polarzero wants it.**
I am not recommending it; it is the option to weigh. The cheapest step is not the rule engine
(`open-limited-or-closed` pulls in 56 decisions) but a fact in the record: which credentials files
exist in main (`~/.config/gh/hosts.yml`, `~/.claude/.credentials.json` or the Keychain item,
`~/.aws/credentials`, `.env` files in repositories), listed by `silo` from outside. It promises no
protection, so it needs no rule. It tells polarzero what a hostile package in main could take.
Whether that is worth a feature before the second build is polarzero's call.

### 3.4 Challenges to decisions in force

- **`what-silo-is-not`** says Silo is not "a sandbox around one process". Keep it. But the case
  that matters most to users is not process versus machine; it is *mounted* versus *not mounted*.
  ClodPod, Docker Sandboxes' default, Apple's container machines and Cowork's local mode are all
  VMs that mount your folder, and Cowork's escape went through the mount. The line that
  distinguishes Silo in practice is `nothing-shared-from-your-device`. The home page should say it
  with the case: "An agent in main that runs `rm -rf ~/code` deletes main's copy. Your Mac's copy
  is not there to delete."
- **`device-not-shared`** costs the first build the thing every competitor above has: your
  project open in your Mac's editor, live. `files-imported-from-your-mac` and `ssh main` with a
  remote editor cover it, but this is the first complaint polarzero will have. I would not change
  the decision (the Cowork escape is the reason), but the first build should make the editor over
  SSH (VS Code Remote, Zed remote) work on day one, since it is the only way to edit main's files
  comfortably.
- **`checkpoint-per-system`**: see 3.3 b. Not a change yet; one more row in the measurement.

## Not verified

- The details of incidents known only from mirrors (claudeissues.com could not be fetched from
  here) and press summaries: marked "search" above.
- Whether macOS 27 provisioning skips every Setup Assistant screen.
- Whether Codex `/undo` ships in the current Codex CLI.
- Daytona's macOS sandboxes: the docs mention them on dedicated hardware, in early access; I found
  no launch.
- Pricing and dollar figures of runaway spend: anecdotal.

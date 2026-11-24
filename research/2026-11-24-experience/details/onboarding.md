# How comparable tools onboard a new user, and bring them back

Research for Silo's first-run and daily experience. Gathered 2026-11-24 from official docs, READMEs,
release notes, source code and a few walkthroughs. Every concrete claim has a URL. "Unverified" marks
what I could not confirm from a primary source. Where I measured something myself (image sizes from
the GHCR registry, IPSW sizes from ipsw.me), the method is stated.

Contents

1. Docker Sandboxes (`sbx`, formerly `docker sandbox`)
2. OrbStack
3. Tart (Cirrus Labs, now part of OpenAI)
4. Conductor
5. Claude Code (CLI, sandbox, remote control, cloud, agent view)
6. OpenAI Codex (CLI, cloud, mobile)
7. Lima, Colima, UTM, Apple `container`
8. Other tools in brief (Container Use, Sculptor, Vibe Kanban, Claude Squad, Paradev/Superset, Crystal/Nimbalyst, Cursor cloud agents, E2B, Daytona)
9. Comparison table
10. What this means for Silo

---

## 1. Docker Sandboxes (`sbx`)

**What it is now.** The Docker Desktop–integrated `docker sandbox run claude` (Docker Desktop 4.50+,
experimental, container-based) is deprecated and replaced by a standalone CLI, `sbx`, that runs each
agent in a microVM with its own Docker daemon. Docker Desktop is not required.
- Deprecation and the old 4.50 flow (`docker sandbox run claude`, `-w` workspace, `-c` resume, API key
  stored in volume `docker-claude-sandbox-data`): https://docs.docker.com/ai/sandboxes/docker-desktop/ ,
  https://docs.docker.com/ai/sandboxes/migration (the old Claude page: https://docs.docker.com/ai/sandboxes/claude-code)
- Blog, Jan 30 2026, microVM isolation "available now for macOS and Windows":
  https://www.docker.com/blog/docker-sandboxes-run-claude-code-and-other-coding-agents-unsupervised-but-safely/
- A third-party guide says Docker first showed it in December 2025 and launched it around March 2026
  (unverified against Docker): https://www.c-sharpcorner.com/article/docker-sandboxes-a-complete-walkthrough/

**Requirements.** macOS Sonoma 14+ on Apple silicon; Windows 11 x64 with Hypervisor Platform; Ubuntu
24.04+ with KVM and the user in the `kvm` group. https://docs.docker.com/ai/sandboxes/install/

**Install.**
```
brew trust docker/tap
brew install docker/tap/sbx
```
Windows: `winget install -h Docker.sbx`. Ubuntu: `curl -fsSL https://get.docker.com | sudo SBX=1 sh`.
https://docs.docker.com/ai/sandboxes/install/

**First-run path (local), step by step.**
1. `sbx login` — "The command opens a browser for Docker OAuth." A Docker account is mandatory; the FAQ
   justifies it as giving "each sandbox a verified identity tied to a person".
   https://docs.docker.com/ai/sandboxes/install/ , https://docs.docker.com/ai/sandboxes/faq/
2. `cd ~/my-project && sbx run --name my-sandbox claude`
   https://docs.docker.com/ai/sandboxes/get-started/
3. If no global network policy exists, an interactive prompt asks for a preset: **Open** (`allow-all`),
   **Balanced** (`balanced`, default-deny with a baseline allowlist of AI APIs, package managers, OS repos,
   code hosts, registries, cloud services), **Locked Down** (`deny-all`). Navigation: "use ↑/↓ to navigate,
   Enter to select, or press 1–3". Docs recommend Balanced. Non-interactive use errors until
   `sbx policy init balanced` / `sbx policy set-default`. The exact prompt wording is not published
   (unverified). https://docs.docker.com/ai/sandboxes/security/policy/ ,
   https://docs.docker.com/ai/governance/local/ , https://docs.docker.com/reference/cli/sbx/policy/init/
4. First run pulls the agent image (`docker/sandbox-templates:claude-code`) and "takes longer"; later
   runs "start in seconds". Image size and time are not published (unverified).
   https://docs.docker.com/ai/sandboxes/get-started/ , https://docs.docker.com/ai/sandboxes/agents/claude-code/
5. Claude Code starts as `claude --dangerously-skip-permissions`. Auth: with a Claude subscription,
   run `/login` inside the sandbox (OAuth; token "stays on your host"); or `sbx secret set anthropic`
   for an API key. https://docs.docker.com/ai/sandboxes/agents/claude-code/ ,
   https://docs.docker.com/ai/sandboxes/get-started/
6. Optional GitHub: `sbx secret set github --command 'gh auth token'`.
   https://docs.docker.com/ai/sandboxes/get-started/

So: 2 install commands + login (browser) + 1 run command + 1 policy prompt + Claude `/login` (browser
again). Two separate browser sign-ins (Docker, then Anthropic) before the first prompt.

**Defaults.**
- Workspace: current directory mounted read-write at the same absolute path ("direct mount", virtiofs).
  `--clone` gives a private git clone with the host repo read-only at `/run/sandbox/source`.
  https://docs.docker.com/ai/sandboxes/usage/ , https://docs.docker.com/ai/sandboxes/architecture/
- No approval prompts inside: FAQ "Why do agents run without approval prompts?" — the microVM is the
  safety boundary. https://docs.docker.com/ai/sandboxes/faq/
- Credentials: a host-side proxy injects them; "The raw credential values never enter the VM." The agent
  sees a sentinel value. https://docs.docker.com/ai/sandboxes/security/ ,
  https://docs.docker.com/ai/sandboxes/architecture/
- Host `~/.claude` user config (hooks, settings) is **not** imported; only project-level config.
  https://docs.docker.com/ai/sandboxes/faq/
- Image paste from host clipboard is off by default. https://docs.docker.com/ai/sandboxes/faq/
- Telemetry on by default (command, success, duration, username), opt out with `SBX_NO_TELEMETRY`.
  https://docs.docker.com/ai/sandboxes/faq/

**Blocked network requests ("approvals") — the closest thing to Silo's holds.** Under Balanced or
Locked Down, an unmatched destination "is blocked and asks for your approval instead of being denied
outright". But the triggering request is **not held**: "The request that triggered the approval doesn't
wait for your answer." It is denied; approving affects later requests; agents that retry "pick up the new
access on their next attempt". The sandbox sees "Approval required for api.example.com." Options are
only `allow` and `dismiss`. Respond with `sbx policy approval ls` /
`sbx policy approval respond <ID> --option allow`, or in the TUI's network panel. No phone, no
notification documented. https://docs.docker.com/ai/governance/local/approvals/ ,
https://docs.docker.com/ai/sandboxes/security/policy/

**First output.** `sbx ls`:
```
SANDBOX      AGENT    STATUS      PORTS   WORKSPACE      CREATED
my-sandbox   claude   ● running           ~/my-project   2m
```
https://docs.docker.com/ai/sandboxes/get-started/ — `sbx secret ls`:
```
SCOPE      SERVICE   SECRET
(global)   github    gho_TK421****...****R2D2
```
https://github.com/mikegcoleman/sbx-quickstart

**Coming back later.**
- Sandboxes persist after the agent exits; packages, images and config survive restarts.
- Re-running `sbx run claude ~/my-project` "reconnects to same sandbox"; `sbx run --name my-project`
  re-attaches from anywhere. `sbx run -d` keeps it in the background. `sbx exec -it <name> bash`.
- `sbx` with no arguments opens a TUI dashboard: Enter attach, `x` shell, `s` start/stop, `tab` switches
  between sandboxes and network panels.
  https://docs.docker.com/ai/sandboxes/usage/
- `sbx run quickstart --branch=fix-bugs` creates worktrees under `.sbx/<name>-worktrees/`; `sbx rm`
  deletes them. https://github.com/mikegcoleman/sbx-quickstart

**Copy.** One command from a project dir; reattach by path (no name to remember); credentials injected
by a host proxy so secrets never enter the VM; a no-argument TUI dashboard; "kits" (declarative mixins:
install commands, allowed domains, proxy-managed credentials, shared as OCI artifacts).
Docker's own argument for kits, Aug 2026: "Isolation only survives contact with developers when it is
at least as convenient as skipping it." https://www.docker.com/?p=92667 (post "Empty sandboxes break
developer experience"). Matt Pocock quote used in marketing: "Docker Sandboxes have the best DX of any
local AI coding sandbox I've tried." (blog above)

**Friction.** Mandatory Docker account; closed-source (https://warski.org/blog/docker-sandboxes-sbx-vs-sandcat/);
MITM certificates break tools that ignore the system CA store (Java, Node, GraalVM binaries) (same
source); user-level Claude config not available, so CLAUDE.md, hooks and skills must be copied in
(https://docs.docker.com/ai/sandboxes/faq/ , https://github.com/mikegcoleman/sbx-quickstart says
"symlinks won't work"); approvals deny the in-flight request; published ports do not survive restart
(https://github.com/mikegcoleman/sbx-quickstart); Linux guest only, no macOS guest.

---

## 2. OrbStack

**Install.** Download the app, or `brew install orbstack`. The quick start says it "is ready to use out
of the box". https://docs.orbstack.dev/quick-start

**First-launch screens** (from a third-party walkthrough; OrbStack does not document them — partly
unverified): Welcome screen with terms notice and Next; macOS dialog "Allow administrator access for
'OrbStack'?" with Install Helper / Don't Allow ("You type an admin password two or three times");
a choice screen "What do you want to use?" with cards Docker, Kubernetes, Linux and the caption "Don't
worry, you can always change this later"; then the main window "opens right away, with no sign-up
step"; macOS asks to allow notifications and says OrbStack "can run in the background"; bottom-left
shows "Sign In" and "Pro Trial (29 days)".
https://www.starryhope.com/mac/orbstack-docker-desktop-mac/
Release notes confirm "Redesigned onboarding for new users" (v0.11.0) and "Added option to set the
username during onboarding" (v2.2.0). https://docs.orbstack.dev/release-notes

**Sign-in.** None required to start; personal use is free; "Licenses are now required for freelance,
business, and commercial use" (v1.0); offline licensing up to 7 days (v0.16.1).
https://docs.orbstack.dev/release-notes

**Downloads and speed.** "Less than 10 MB of disk space is used out of the box"; "Easily get started
with Linux machines in 1 minute"; "less than 0.1% background CPU usage on Apple Silicon".
https://orbstack.dev/ — machine creation takes "less than 1 minute on fast connections".
https://docs.orbstack.dev/machines/ — No macOS guests (no release-note entries).
https://docs.orbstack.dev/release-notes

**Linux machine first run.** `orb create ubuntu new-ubuntu` (or GUI). Default user = your macOS
username, no password, passwordless sudo. Mac files at `/mnt/mac`; Linux files on the Mac at
`~/OrbStack` and in Finder; `open`, `pbcopy` aliased; `mac <cmd>` runs a Mac command.
https://docs.orbstack.dev/machines/

**SSH — the direct analogue of `ssh main`.** OrbStack adds an `orb` host to your SSH config:
`ssh orb` (default machine), `ssh machine@orb`, `ssh user@machine@orb`. One built-in SSH server for all
machines, a generated key at `~/.orbstack/ssh/id_ed25519`, password auth disabled, localhost only, SSH
agent forwarded automatically, VS Code Remote-SSH works by connecting to `orb`; JetBrains via
`localhost:32222`. https://docs.orbstack.dev/machines/ssh — "Shell profile and SSH config will now only
be edited once" (v1.7.4) and "Fixed `~/.ssh` permissions being changed on start" (v2.2.3) show the cost
of touching users' SSH config. https://docs.orbstack.dev/release-notes

**Isolated machines (2026).** `--isolated` machines run without macOS integration and share no Mac
files by default; `--isolate-network` blocks host/machine connections; SSH agent forwarding is opt-in
(`--forward-ssh-agent`); docs say they "aren't a full security boundary". Selective file sharing
arrived in v2.1.1 (April 20 2026). https://docs.orbstack.dev/machines/isolated ,
https://docs.orbstack.dev/release-notes

**Coming back / notifications.** Menu bar app with quick actions; machines can be started from the
menu bar (v1.1.0); notifications from Linux are shown on the Mac (v0.1.6).
https://docs.orbstack.dev/release-notes , https://www.starryhope.com/mac/orbstack-docker-desktop-mac/

**Copy.** No account to start; one question at first launch, reversible; the `orb` SSH host so every
SSH-speaking tool just works; same username as the Mac; tiny footprint; offer to migrate from the
competitor (Docker Desktop) on first launch (https://docs.orbstack.dev/install).
**Friction.** Several admin-password prompts; it edits `~/.ssh/config` and shell profile; commercial
licence after trial.

---

## 3. Tart (Cirrus Labs)

**Ownership change.** Cirrus Labs announced on April 7 2026 that it is joining OpenAI's Agent
Infrastructure team; Cirrus CI shut down June 1 2026; Tart licensing fees waived, moving to more
permissive licences (secondary sources; primary at https://cirruslabs.org/). The Tart quick start now
installs from an OpenAI tap: `brew install openai/tools/tart`. https://tart.run/quick-start/ ,
https://dev.to/onsen/cirrus-labs-joins-openai-what-it-means-for-ai-3bk1

**Install and first run.**
```
brew install openai/tools/tart
tart clone ghcr.io/cirruslabs/macos-tahoe-base:latest tahoe-base
tart run tahoe-base
ssh admin@$(tart ip tahoe-base)      # password: admin
```
Requires Apple silicon, macOS 13+. Default credentials `admin`/`admin` for GUI, console and SSH.
Linux: `tart clone ghcr.io/cirruslabs/ubuntu:latest ubuntu && tart set ubuntu --disk-size 50`.
The quick start still says the first example "will download a 25 GB image", and shows no CLI output.
https://tart.run/quick-start/

**What is downloaded — measured.** I read the OCI manifests on ghcr.io (sum of layer sizes = download;
`org.cirruslabs.tart.uncompressed-disk-size` = disk on host), 2026-11-24:

| Image (`ghcr.io/cirruslabs/...:latest`) | Download | Disk |
|---|---|---|
| macos-tahoe-vanilla | 24.0 GB | 50 GB |
| macos-tahoe-base | 27.1 GB | 50 GB |
| macos-sequoia-base | 25.4 GB | 50 GB |
| macos-golden-gate-vanilla (macOS 27) | 30.0 GB | 35.1 GB |
| macos-golden-gate-base (macOS 27) | 31.8 GB | 38.4 GB |
| macos-tahoe-xcode | 70.8 GB | 140 GB |
| macos-sequoia-xcode | 68.4 GB | 140 GB |
| ubuntu | 3.1 GB | 20 GB |

Method: `https://ghcr.io/token?scope=repository:cirruslabs/<img>:pull` then
`https://ghcr.io/v2/cirruslabs/<img>/manifests/latest`. At 100 Mbit/s a 27 GB base image is about
36 minutes; an Xcode image about 1.5 hours (my arithmetic).

Building from Apple's IPSW instead (`tart create --from-ipsw`): macOS VM restore images for
VirtualMac2,1 are 25.6 GB (macOS 27.2.x) and 19.8 GB (macOS 26.6.x), per
https://api.ipsw.me/v4/device/VirtualMac2,1?type=ipsw — and then Setup Assistant still has to run.

**Image variants.** vanilla ("a vanilla macOS installation with helpful tweaks such as auto-login"),
base (adds Homebrew and tools), xcode (adds Xcode N and Flutter), runner. Rebuilt monthly.
https://github.com/cirruslabs/macos-image-templates

**Storage and limits.** Everything in `~/.tart/` (`vms/`, `cache/OCIs/`). Before a pull, Tart
auto-prunes least-recently-used cached images if space is short (100 GB default limit). On macOS 27+,
`tart clone --stacked` keeps writes in an overlay over a shared read-only base, and pulling another
image of the same lineage downloads only missing disk files. macOS 15+ guests need an unlocked
`login.keychain` or the VM fails to start headless. Softnet (`--net-softnet`) is stricter and blocks
host access; default NAT lets guests reach host services on `0.0.0.0`. Nested virt only M3/M4, Linux
guests, `--nested`. https://tart.run/faq/ — Apple's licence limits macOS guests to two per host
(well known; not stated on that FAQ page, unverified here).

**Coming back.** `tart list`, `tart run <name>`, `tart ip <name>`. No resume UI, no notifications; it is
a CLI building block for CI.

**Copy.** OCI registry distribution of whole VMs (pull, cache, layer dedupe, stacked clones);
a VM ready to SSH into with known credentials; auto-prune before pulls.
**Friction.** A 24–32 GB first download with no size or time shown up front in the docs; `admin/admin`;
macOS disk resize needs Packer or manual repartitioning (https://tart.run/faq/).

---

## 4. Conductor (conductor.build)

**What it is.** Mac app (Apple silicon) by Melty Labs (YC) that runs Claude Code, Codex, Cursor and
OpenCode in parallel, one git worktree per "workspace". Free; usage billed to your own provider plans.
https://www.conductor.build/docs/ , https://www.morphllm.com/conductor-ai-coding ,
https://www.conductor.build/llms-full.txt

**Install and first launch.** Download, drag to Applications, open. "When you open Conductor, it checks
for the tools and credentials it needs": GitHub in your terminal (`gh auth status`), Claude Code login
(`claude /login`), Codex login (`codex login`), Cursor API key. "By default, Conductor uses the auth
tokens already saved on your machine." It bundles its own Claude Code and Codex binaries (FAQ: do not
update or modify them). macOS only. https://www.conductor.build/llms-full.txt
Onboarding was redesigned in 0.11.3 (Sep 22 2025): "You can now create your setup scripts with Claude",
and adding a repository now automatically creates a workspace.
https://www.conductor.build/changelog/0.11.3-new-onboarding

**First workspace.** Add a repo (Open project / Open GitHub project / Quick start). Each workspace = a
branch + worktree; Conductor fetches `origin` first; "When you start your first chat, Conductor will
instruct the agent to rename this branch". Untracked files (`.env`, `node_modules`) come from a setup
script; `CONDUCTOR_PORT` lets several workspaces run servers. Review in the Diff Viewer (⌘⇧D), Checks
tab, create PR, merge, archive. https://conductor.build/docs/first-workspace ,
https://conductor.build/docs/concepts/workspaces-and-branches
A reviewer did "four workspaces, one per bug", reviewed and merged in about 10 minutes.
https://madewithlove.com/blog/conductor-running-multiple-ai-coding-agents-in-parallel.md

**Permissions.** "Agents run directly on your system without sandboxing." "Workspace isolation is
development isolation, not a security boundary." "When tool approvals are off, Claude and Codex presets
start in full-access mode." Cloud workspaces run in "an isolated microVM" (8 vCPU, 16 GB, 32 GB NVMe,
Amazon Linux 2023), "hosted by Vercel"; each org shares one "Cloud Computer" image you build.
https://www.conductor.build/llms-full.txt

**Checkpoints.** "automatic snapshots of an agent's changes to your codebase"; revert from a message,
which permanently deletes messages from that turn on; stored locally, separate from git history.
https://www.conductor.build/llms-full.txt

**Coming back / notifications.** Archive after merge; restore from the History pane "including its chat
history". Remote Control: "lets you use the Conductor iOS app to work in the local workspaces on your
Mac" (experimental, same account). Notification behaviour is not documented (unverified).
https://www.conductor.build/llms-full.txt

**Copy.** First launch is a checklist of what is already on the machine, reusing existing logins; one
action (add repo) yields a ready workspace; an agent writes the setup script; per-workspace port
variable; History pane to restore archived work with chat.
**Friction.** No sandbox; broad GitHub access ("You are INSANE to authorise this app on anything other
than throwaway code", HN commenter quoted at
https://madewithlove.com/blog/conductor-running-multiple-ai-coding-agents-in-parallel.md); every
workspace must be bootstrapped; parallel agents multiply token spend (same source; also
https://www.morphllm.com/conductor-ai-coding).

---

## 5. Claude Code

**Install.** `curl -fsSL https://claude.ai/install.sh | bash` (native, auto-updates in background;
"shows no progress while it downloads"), or `brew install --cask claude-code` (stable channel "about a
week behind"; no auto-update), or WinGet/apt/dnf/apk. Check with `claude --version`, which prints a
version "followed by `(Claude Code)`". https://code.claude.com/docs/en/quickstart

**First run.** `cd project && claude`. Login prompt on first use (browser OAuth for subscription or
Console; `ANTHROPIC_API_KEY` skips it after you approve the key). "The Claude Code prompt appears with
the version, current model, and working directory shown above it." Suggested first prompts: "what does
this project do?", "add a hello world function to the main file".
https://code.claude.com/docs/en/quickstart
Screens before that, per third-party walkthroughs (unverified wording, varies by version): a text-style
(theme) picker, "Select login method:", and "Do you trust the files in this folder?" with "Yes, proceed"
/ "Yes, I trust this folder". https://realpython.com/lessons/setting-up-claude-code/ ,
https://www.educative.io/courses/claude-code/installing-and-launching-claude-code ,
https://www.inflearn.com/community/questions/1891962/

**Permission modes** (status bar text in brackets): `default` [`⏸ manual mode on`], `acceptEdits`
[`⏵⏵ accept edits on`], `plan` [`⏸ plan mode on`], `auto` [`⏵⏵ auto mode on`], `dontAsk`,
`bypassPermissions` [`⏵⏵ bypass permissions on`]. Shift+Tab cycles. "With Claude Code v2.1.283 or
later, auto mode is the built-in starting permission mode for interactive terminal and VS Code
sessions." Auto mode = a classifier model (Sonnet 5 by default) reviews actions; it pauses after 3
consecutive or 20 total blocks. `--dangerously-skip-permissions` = `bypassPermissions`, recommended
only for "isolated environments like containers, VMs, or dev containers without internet access";
first interactive use shows an acceptance dialog; refused under root/sudo; still prompts for ask rules
and critical `rm` paths. https://code.claude.com/docs/en/permission-modes

**Sandboxing.** Off by default; `/sandbox` or `sandbox.enabled`. macOS Seatbelt; Linux bubblewrap+socat.
Writes: working dir + temp; reads: "Most of the machine, including credential files such as `~/.ssh`
and `~/.aws/credentials`". Network: "No direct route out ... allowed domains, which start empty"; first
use of a new domain prompts (in manual/acceptEdits). Only covers shell commands: "Claude's file tools,
MCP servers, and hooks run outside it." Escape hatch: retry with `dangerouslyDisableSandbox` unless
`allowUnsandboxedCommands: false`. https://code.claude.com/docs/en/sandboxing

**Checkpoints.** Each prompt creates a checkpoint (last 100 kept, ~30 days). `/rewind` or Esc Esc:
restore code and/or conversation. Limits: "A file that only Bash commands changed isn't tracked";
subagent edits usually not restored; external changes not tracked; symlinks skipped; "Not a
replacement for version control". https://code.claude.com/docs/en/checkpointing

**Coming back.** `claude -c` (most recent in this dir), `claude -r` picker (Space preview, Ctrl+A all
projects, Ctrl+W worktrees, Ctrl+B branch), `claude --resume <name>`, `/rename`, `claude -n name`.
Transcripts at `~/.claude/projects/<project>/<id>.jsonl`, 30-day retention. Permission mode is restored
on resume except `bypassPermissions`, which falls back to the default.
https://code.claude.com/docs/en/sessions
Agent view (research preview): `claude agents`; `claude --bg --name x "task"` prints
```
backgrounded · 7c5dcf5d · flaky-test-fix
  claude agents             list sessions
  claude attach 7c5dcf5d    open in this terminal
  claude logs 7c5dcf5d      show recent output
  claude stop 7c5dcf5d      stop this session
```
States: Working, Needs input, Idle, Completed, Failed, Stopped; grouped Pinned / Ready for review /
Needs input / Working / Completed; tab title "2 awaiting input · claude agents"; each background session
works in its own worktree under `.claude/worktrees/`; runs under a supervisor so it survives closing the
terminal. https://code.claude.com/docs/en/agent-view

**Notifications.** On finish or permission prompt while you seem away: desktop notification in
Ghostty, Kitty, iTerm2 (iTerm2 needs its alert settings), silent bell or nothing in Apple Terminal,
nothing elsewhere; `preferredNotifChannel: "terminal_bell"`; `Notification` hooks; OSC 7501 status
protocol. https://code.claude.com/docs/en/terminal-config

**Remote Control (phone).** `claude remote-control` (server), `claude --remote-control`, or
`/remote-control` (`/rc`). One-time "Enable Remote Control? (y/n)"; asks `Trust <directory>? [y/N]`;
prints a URL, spacebar shows a QR code; `/mobile` shows a QR to install the app. Code runs locally the
whole time. "Claude Code keeps permission prompts and `AskUserQuestion` questions open until you answer
them" (other dialogs expire after 5 minutes by default). Push notifications: `/config` → "Push when
actions required" (permission prompts and questions) and/or "Push when Claude decides"; skipped while
you are focused on the terminal. Requires claude.ai login; the local process must keep running
(use tmux over SSH); server mode gives up after ~10 minutes offline; sessions can be brought back
within ~4 hours. Optional "Trusted Devices". https://code.claude.com/docs/en/remote-control

**Cloud sessions.** claude.ai/code, Code tab in the mobile app, or `claude --cloud "task"`, which shows
"a live checklist of setup steps" while the VM starts and queues messages typed meanwhile. Onboarding
creates a **Default** environment with **Limited** network access. GitHub via the Claude GitHub App or
`/web-setup` (sends local `gh` token); "your GitHub credentials ... never enter a session's VM" (proxy).
`claude --teleport` pulls a cloud session into the terminal; `claude -p "msg" --cloud <id>` prints
```
Sent to cloud session.
Session ID: session_01HJKLMNOPQRSTUVWXYZ
View: https://claude.ai/code/session_01HJKLMNOPQRSTUVWXYZ?from=cli&m=0
```
On idle expiry the VM is reclaimed: conversation restored, running shell commands and subagents not.
https://code.claude.com/docs/en/claude-code-on-the-web

**Spend.** `--max-budget-usd` exists as a run-limiting flag (https://code.claude.com/docs/en/sessions);
`/status` reports account and remaining allocation
(https://www.conductor.build/llms-full.txt, Claude Code reference section); `/usage` view
(https://code.claude.com/docs/en/terminal-config).

---

## 6. OpenAI Codex

**Install.** `curl -fsSL https://chatgpt.com/codex/install.sh | sh`, `npm install -g @openai/codex`,
`brew install --cask codex`. https://github.com/openai/codex , https://learn.chatgpt.com/docs/codex/cli

**First run.** `codex` in a project dir. Welcome line from source: "Welcome to **Codex**, OpenAI's
command-line coding agent". Sign-in options (source strings): "Sign in with ChatGPT" ("Usage included
with Plus, Pro, Business, and Enterprise plans"), "Sign in with Device Code" ("Sign in from another
device with a one-time code"), "Use an OpenAI API key" ("Pay for what you use"), Amazon Bedrock, Google
Vertex AI; then "Finish signing in via your browser".
https://raw.githubusercontent.com/openai/codex/main/codex-rs/tui/src/onboarding/welcome.rs ,
https://raw.githubusercontent.com/openai/codex/main/codex-rs/tui/src/onboarding/auth.rs
Browser OAuth returns to `localhost:1455`; headless: `codex login --device-auth`; or copy
`~/.codex/auth.json`; API key: `printenv OPENAI_API_KEY | codex login --with-api-key`; credentials in
file or OS keyring. https://learn.chatgpt.com/docs/auth

**Trust prompt** (verbatim from source): "Trust this folder? Codex can read, edit, and run files here,
subject to your permission settings. Folder settings can run code automatically, even without a model
request. Continue only if you trust these files. Your trust decision will be saved." Options "Trust and
continue" / "Quit"; in a subdirectory: "Note: You're in a subdirectory of a Git project. Trusting will
apply to the repository root:".
https://raw.githubusercontent.com/openai/codex/main/codex-rs/tui/src/onboarding/trust_directory.rs

**First screen.** "OpenAI Codex", model and "directory: ~/code", "To get started, describe a task or try
one of these commands:" `/init`, `/status`, `/permissions`, `/model`, `/review`; footer "100% context
left · ? for shortcuts". https://learn.chatgpt.com/docs/codex/cli

**Defaults.** Version-controlled folders: "`Auto` (workspace write + on-request approvals)";
non-version-controlled: `read-only`; "the agent runs with network access turned off" by default
(`network_access = true` in workspace-write to enable). Sandbox: macOS Seatbelt via `sandbox-exec`,
Linux `bwrap` + seccomp. `--yolo` = `--dangerously-bypass-approvals-and-sandbox`. Approval policies
`on-request`, `never`, `granular` (`untrusted` retired). Optional auto-review: a reviewer agent judges
approval requests; "Failures fail closed". https://learn.chatgpt.com/docs/agent-approvals-security

**Coming back.** `codex resume` reopens a recent chat from the repo or searches local chats.
https://learn.chatgpt.com/docs/codex/cli
**Notifications.** Desktop app: turn-completion alerts never / in background / always, separate
permission/question notifications, Activity view; web: push, email or SMS channels; CLI: TUI
notification setting and a `notify` external program on turn completion.
https://learn.chatgpt.com/docs/notifications
**Phone.** Since May 14 2026, Codex in the ChatGPT mobile app (preview, all plans) can "approve
commands, change models, or start something new" on a trusted Mac running the Codex app; "Your files,
credentials, permissions, and local setup stay on the machine". Paired through the desktop app, not the
CLI (unverified for CLI). https://openai.com/index/work-with-codex-from-anywhere/ ,
https://dev.to/owen_fox/codex-mobile-app-monitor-control-your-ai-coding-agent-from-iphone-or-android-2026-3ah
**Cloud.** Sign in to ChatGPT, Work in > Cloud, pick or create an environment, connect GitHub, network
settings ("Enable all public internet access", package manager domains, allowlist). Default network
not stated (unverified). https://learn.chatgpt.com/docs/cloud

---

## 7. Lima, Colima, UTM, Apple `container`

**Lima.** `brew install lima`. Lima 2.1 (March 2026) added experimental macOS guests:
`limactl start template:macos`; the user password is random and stored inside the VM
(`limactl shell macos cat /Users/${USER}.guest/password`).
https://www.cncf.io/blog/2026/03/25/lima-v2-1-macos-guests-and-enhanced-ai-agent-safety/
AI-agent guide: mount only the project (`limactl start --mount-only .:w`), install the agent inside
(`lima sudo npm install -g @anthropic-ai/claude-code`, `lima claude`). Safer mode (2.1+): create with
`--mount-none`, then `limactl shell --sync . default claude "..."`; after the session:
```
⚠️ Accept the changes?
→ Yes
  No
  View the changed contents
```
Requires rsync on both sides, host dir at least four levels deep; `--sync-exclude` / `.limasyncignore`
(2.3+). https://lima-vm.io/docs/examples/ai/

**Colima.** `brew install colima`, `colima start`; default VM "2 CPUs, 2GiB memory and 100GiB storage";
`--vm-type=vz`, `--vz-rosetta`, `krunkit` for GPU. https://github.com/abiosoft/colima

**UTM.** Virtualize → macOS 12+; if no IPSW is chosen, "UTM automatically downloads the latest
compatible macOS" (size not stated; Apple VM IPSWs are 19.8–25.6 GB, https://api.ipsw.me/v4/device/VirtualMac2,1?type=ipsw).
Clipboard needs macOS 15+ host and guest plus guest tools; snapshots/disposable mode need macOS 27+.
https://docs.getutm.app/guest-support/macos/

**Apple `container`.** Signed .pkg from GitHub releases (admin password), then `container system start`,
`container run --rm alpine echo hello`; each container is a lightweight VM; Apple silicon, macOS 26+.
Kernel-download prompt wording not documented (unverified). https://github.com/apple/container

---

## 8. Other tools in brief

- **Container Use (Dagger).** `brew install dagger/tap/container-use`, then
  `claude mcp add container-use -- container-use stdio`; "Each agent gets a fresh container in its own
  git branch"; review with `git checkout <branch>`; full command history and logs.
  https://github.com/dagger/container-use
- **Sculptor (Imbue).** Each agent in its own Docker container; requires Docker 27+; dev container
  image built once so "every new agent starts ready" (setup "from minutes to seconds"). Open source.
  https://docs.imbue.com/getting-started/docker-configuration , https://imbue.com/blog/containers ,
  https://imbue.com/sculptor/
- **Vibe Kanban.** `npx vibe-kanban`; worktree per workspace; the README now says "Vibe Kanban is
  sunsetting". https://github.com/BloopAI/vibe-kanban
- **Claude Squad.** `brew install claude-squad`, run `cs`; tmux + worktree per agent; `-y/--autoyes`
  experimental auto-accept; `r` resume a paused session, `c` commit and pause.
  https://github.com/smtg-ai/claude-squad
- **Superset → Paradev.** "Superset is now Paradev"; MIT; macOS/Linux + iOS/Android apps; worktree per
  task; `superset new "fix onboarding crash" --agent claude`; "doesn't proxy any API calls".
  https://paradev.com/
- **Crystal → Nimbalyst.** Crystal deprecated Feb 2026, successor Nimbalyst (worktrees, session kanban).
  https://github.com/stravu/crystal , https://nimbalyst.com/crystal
- **Cursor cloud agents.** Premium+ plans; admin connects source control; environments via agent-led
  setup, snapshot or `.cursor/environment.json`; background builds so agents start ready; notifications
  via Slack, iOS app, cursor.com/agents; "Take control of the agent's desktop", then release it.
  https://cursor.com/docs/cloud-agent
- **E2B.** `npm i e2b` / `pip install e2b`, `Sandbox.create()` with `E2B_API_KEY`; "Pausing a sandbox
  saves both its filesystem and its memory". https://docs.e2b.dev/
- **Daytona.** Default sandbox 1 vCPU / 1 GB / 3 GiB; container runtime listed at "<90ms" startup.
  https://www.daytona.io/docs/en/getting-started/
- Not researched for lack of time: Coder, Dev Containers, Warp (no claims made).

---

## 9. Comparison

| Tool | Steps to first useful result | Sign-ins | Download | Default permissions / network | Return later | Notify |
|---|---|---|---|---|---|---|
| Docker `sbx` | brew ×2, `sbx login`, `sbx run claude`, policy prompt, `/login` | Docker (browser) + Anthropic (browser) | agent image, size unpublished | bypass inside VM; Balanced allowlist; blocked = denied + approval queued | `sbx run` same path/name, `sbx` TUI, `sbx ls` | none documented |
| OrbStack | install app, admin prompts, one choice screen, `orb create` / `ssh orb` | none (licence for commercial) | <10 MB app; distro image (<1 min) | full Mac integration; isolated mode opt-in | menu bar, `ssh orb` | Linux → Mac notifications |
| Tart | brew, `tart clone` (24–32 GB), `tart run`, ssh admin/admin | none | 24–32 GB macOS, 68–71 GB Xcode, 3 GB Ubuntu | NAT, host reachable | `tart list` | none |
| Conductor | download, launch checks, add repo, chat | reuses gh / claude / codex logins | app (bundles agents) | no sandbox; full access when approvals off | sidebar, History pane | iOS remote (exp.) |
| Claude Code | curl, `claude`, theme, login, trust, prompt | Anthropic (browser) | small CLI | auto mode (classifier) by default; sandbox off | `-c`, `-r`, `claude agents` | terminal notif, hooks, mobile push |
| Codex | curl, `codex`, sign-in, trust, prompt | ChatGPT (browser) | small CLI | workspace-write + on-request; network off | `codex resume` | TUI/notify; ChatGPT mobile (desktop app) |
| Lima | brew, `limactl start --mount-none`, `limactl shell --sync` | none | distro image; macOS IPSW for macOS guest | per flags | `limactl list` (not verified) | none |

---

## 10. What this means for Silo

Facts above point to these design conclusions (my reasoning, not sourced claims):

1. **The download is the first-run problem for a macOS guest.** A real macOS VM is 24–32 GB to pull
   (Tart base images) or a 20–26 GB IPSW plus Setup Assistant. Nobody in this set shows the size and
   expected time before starting. Silo should: state size and time up front, let the user do something
   useful during the download (sign-ins, choosing budgets, phone pairing), resume interrupted downloads,
   and offer a Linux "main" (3 GB, minutes) so first value does not wait on macOS. Consider Tart-style
   OCI layers and stacked clones for later VMs.
2. **Reuse existing logins, like Conductor.** Docker makes users sign in twice (Docker, then Anthropic
   inside the VM). Detect `claude`/`codex`/`gh` logins on the host and carry them through a host proxy,
   so credentials never enter the VM (Docker and Claude cloud both do this). Avoid requiring a Silo
   account to start (OrbStack).
3. **Holds are a real differentiator.** Docker denies the in-flight request and queues an approval for
   next time; Claude Code's sandbox asks only inside the session; Codex blocks network by default.
   Claude Code Remote Control shows the right UX shape: prompts stay open until answered, push "when
   actions required", skipped while you are at the terminal. Silo's hold keeps the request open at the
   VM boundary: say so in the first run and show a first hold early (for example the first new domain).
   Options worth copying: allow once / allow always for this destination (Docker scopes per destination,
   method and path), and a clear timeout default.
4. **Bypass inside the VM is now the norm.** Docker launches `claude --dangerously-skip-permissions`;
   Claude Code's docs recommend bypass only in isolated VMs. Silo can make that the default in main and
   explain why in one line, as Docker's FAQ does. Mind Claude Code's acceptance dialog on first bypass
   use and its refusal under root.
5. **Whole-VM checkpoints fill a documented gap.** Claude Code checkpoints miss Bash-only changes,
   subagent edits and external changes; Conductor's revert deletes later messages. A VM checkpoint
   covers all of those. Show a restore in the first session.
6. **`ssh main` should feel like `ssh orb`.** OrbStack adds one SSH host, one generated key, same
   username, agent forwarding, localhost only, works with VS Code Remote-SSH. Its release notes show the
   pitfalls: edit `~/.ssh/config` once, never change `~/.ssh` permissions.
7. **Bring the user's agent config.** Docker does not import `~/.claude` (hooks, skills, CLAUDE.md),
   and quickstarts tell users to copy files in by hand. Silo should offer to bring it.
8. **Return paths to copy.** Reattach by project path with no name (sbx); one no-argument dashboard
   (sbx TUI, `claude agents`); states "Needs input / Working / Completed" with a count in the tab title;
   a one-line "what to do next" block after starting something in the background (Claude `--bg` output).
9. **Empty environments lose.** Docker built kits and Imbue/Cursor build images up front because a blank
   sandbox makes the host "win". Silo's main should come with the toolchain the user's project needs,
   or let an agent write the setup (Conductor 0.11.3).
10. **Spend.** Parallel agents multiply token use and users hit limits fast (Conductor reviews). Of the pages
    I read, only Claude Code mentions a spend cap (`--max-budget-usd`, per run); none of the sandbox
    tools offer a budget (not exhaustively checked). Silo's budgets answer a stated complaint.

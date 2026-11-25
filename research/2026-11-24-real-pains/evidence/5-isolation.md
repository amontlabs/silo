# 5. Homemade isolation and parallel-agent setups: what people build and what hurts

Research date: 2026-11-24. Window: 2024 to Nov 2026, weighted to 2026.
Method: HN Algolia API (points and comment counts, plus full comment trees of about 40 threads), GitHub
star counts via the ungh.cc mirror (the GitHub API was not reachable from this session; counts were read on
2026-11-24), vendor docs, GitHub issues of anthropics/claude-code, blogs, and the Cursor and Apple developer forums.
Reddit could not be fetched (blocked) and web search returned no Reddit threads, so Reddit is **not** covered;
HN comments stand in for it. Every quote below is verbatim from the URL given. When a fact comes from a page
summary and not a quote, the item says so.

---

## A. Popularity: tools ranked by GitHub stars (2026-11-24)

| Tool | Approach | Stars | Created | Last push | Note |
|---|---|---:|---|---|---|
| anomalyco/opencode | agent | 228,859 | 2025-04 | 2026-11 | for scale |
| anthropics/claude-code | agent | 156,899 | 2025-02 | 2026-11 | for scale |
| openai/codex | agent | 136,294 | 2025-04 | 2026-11 | for scale |
| daytonaio/daytona | cloud sandbox infra | 71,283 | 2024-02 | 2026-07 | |
| apple/container | Linux VM per container on Mac | 52,041 | 2025-05 | 2026-11 | now has "container machines" |
| UTM (utmapp/UTM) | Mac VM GUI | 37,003 | 2019 | 2026-11 | |
| trycua/cua | computer-use sandboxes, macOS VMs (Lume) | 32,829 | 2025-01 | 2026-11 | |
| abiosoft/colima | Docker VM on Mac | 31,801 | 2021 | 2026-10 | |
| nanocoai/nanoclaw | assistant, each agent in container | 31,124 | 2026-01 | 2026-11 | moved Apple container → Docker |
| manaflow-ai/cmux | macOS terminal for many agents | 28,922 | 2026-01 | 2026-11 | |
| BloopAI/vibe-kanban | worktree per task, kanban | 28,799 | 2025-06 | 2026-11 | |
| lima-vm/lima | Linux (and since v2.1 macOS) VMs | 22,450 | 2021 | 2026-11 | CNCF |
| gitbutlerapp/gitbutler | virtual branches in one checkout | 22,008 | 2023 | 2026-11 | used *instead of* worktrees |
| dockur/macos | macOS in Docker (KVM, not Mac) | 21,754 | 2024 | 2026-11 | |
| coder/coder | remote dev envs | 17,464 | 2021 | 2026-11 | |
| ParadevAI/Paradev (ex superset-sh/superset) | worktree IDE for parallel agents | 16,534 | 2025-10 | 2026-11 | YC P26 |
| loft-sh/devpod | open Codespaces | 15,338 | 2023 | 2025-11 | stalled |
| e2b-dev/E2B | cloud sandboxes | 14,971 | 2023 | 2026-11 | |
| orbstack/orbstack (issues repo) | Docker + Linux machines on Mac | 9,598 | 2023 | — | |
| canonical/multipass | Ubuntu VMs | 9,419 | 2017 | 2026-11 | |
| superradcompany/microsandbox | libkrun microVMs | 9,182 | 2024-10 | 2026-11 | |
| smtg-ai/claude-squad | tmux + worktrees | 8,859 | 2025-03 | 2026-11 | |
| openai/tart (ex cirruslabs/tart) | macOS/Linux VMs on Apple Silicon | 8,008 | 2022 | 2026-11 | Cirrus Labs joined OpenAI, Apr 2026 |
| anthropics/sandbox-runtime | Seatbelt/bubblewrap process sandbox | 7,249 | 2025-10 | 2026-11 | |
| smol-machines/smolvm | **local branchable (CoW) VMs for agents, libkrun** | 7,183 | 2025-12 | 2026-11 | closest open-source analogue to Silo forks |
| agent-infra/sandbox | all-in-one Docker sandbox | 6,380 | 2025-08 | 2026-09 | |
| generalaction/emdash | worktrees, local or over SSH | 6,294 | 2025-08 | 2026-11 | |
| 21st-dev/1code | agent orchestration UI | 5,566 | 2026-01 | 2026-03 | |
| agent-of-empires | Claude/OpenCode session manager | 4,725 | 2026-01 | 2026-11 | |
| kubernetes-sigs/agent-sandbox | k8s sandboxes | 4,699 | 2025-08 | 2026-11 | |
| Dimillian/CodexMonitor | monitor Codex sessions | 4,373 | 2026-01 | 2026-03 | |
| dagger/container-use | container + git branch per agent (MCP) | 4,105 | 2025-05 | 2026-11 | |
| omnara-ai/omnara | run agents from anywhere | 3,444 | 2025-07 | 2026-11 | |
| stravu/crystal | worktrees GUI | 3,128 | 2025-06 | 2026-02 | renamed Nimbalyst |
| microvm-nix/microvm.nix | NixOS microVMs | 3,097 | 2021 | 2026-11 | |
| zerobootdev/zeroboot | sub-ms CoW memory-forked VMs | 2,490 | 2026-03 | 2026-10 | |
| superagent-ai/vibekit | Docker sandbox for agents | 1,871 | 2025-05 | 2026-01 | stalled |
| rivet-dev/agents | sandbox agent SDK | 1,652 | 2026-01 | 2026-11 | |
| anomalyco/rift | "better alternative to worktrees" | 1,374 | 2026-05 | 2026-11 | |
| RchGrav/claudebox | Claude Code in Docker | 1,181 | 2025-06 | 2026-09 | |
| jonwiggins/optio | agents in k8s | 1,113 | 2026-03 | 2026-11 | |
| johannesjo/parallel-code | worktrees + review | 1,105 | 2026-02 | 2026-11 | |
| fencesandbox/fence | container-free sandbox | 1,025 | 2025-12 | 2026-11 | |
| trailofbits/claude-code-devcontainer | hardened devcontainer | 982 | 2025-09 | 2026-11 | |
| lynaghk/vibe | Linux VM per project on Mac | 966 | 2026-01 | 2026-11 | |
| superhq-ai/shuru | local microVM sandbox on Mac (checkpoints, branching) | 897 | — | — | HN 213 pts |
| docker/sbx-releases | Docker Sandboxes CLI | 540 | 2026-03 | 2026-11 | product is closed |
| devflowinc/uzi | worktrees + tmux | 584 | 2025-05 | 2025-06 | abandoned |
| webcoyote/sandvault | macOS low-privilege user + sandbox-exec | 460 | 2025-09 | 2026-11 | |
| coast-guard/coasts | per-worktree localhost / compose | 434 | 2026-02 | 2026-04 | |
| nikvdp/cco | thin sandbox for Claude Code | 432 | 2025-06 | 2026-09 | |
| textcortex/claude-code-sandbox | Docker | 324 | 2025-05 | archived | |
| anthropics/devcontainer-features | devcontainer feature | 318 | 2025-03 | 2025-12 | |
| imbue-ai/sculptor | container per agent + "pairing" sync | 285 | 2025-08 | 2026-11 | |
| terragon-labs/terragon-oss | cloud background agents | 261 | 2026-01 | 2026-02 | company shut down, code dumped |
| webcoyote/clodpod | agent in a macOS VM (Tart) | 197 | 2025-09 | 2026-10 | |

Closed products with no stars: Conductor (Mac app), Docker Sandboxes, exe.dev, Sprites (fly.io), Boxes.dev, Agent37, Freestyle, OrbStack.

### HN attention by approach (points / comments)

| Story | Date | Pts | Comments |
|---|---|---:|---:|
| [macOS Container Machines (apple/container)](https://news.ycombinator.com/item?id=48469658) | 2026-06-10 | 1269 | 437 |
| [Containerization Swift package (Apple)](https://news.ycombinator.com/item?id=44229348) | 2025-06-09 | 769 | 409 |
| [Docker Sandboxes](https://news.ycombinator.com/item?id=49239751) | 2026-08-10 | 694 | 396 |
| [NanoClaw with Apple container isolation](https://news.ycombinator.com/item?id=46850205) | 2026-02-01 | 533 | 224 |
| [Fly "Code and Let Live" (Sprites)](https://news.ycombinator.com/item?id=46557825) | 2026-01-09 | 508 | 189 |
| [Exe.dev](https://news.ycombinator.com/item?id=46397609) | 2025-12-26 | 457 | 297 |
| [Freestyle – Sandboxes for coding agents](https://news.ycombinator.com/item?id=47663147) | 2026-04-06 | 322 | 158 |
| [zeroboot – sub-ms VM sandboxes via CoW memory forking](https://news.ycombinator.com/item?id=47412569) | 2026-03-17 | 311 | 72 |
| [Omnara – run Claude Code from anywhere](https://news.ycombinator.com/item?id=44878650) | 2025-08-12 | 310 | 168 |
| [Lume – macOS/Linux VMs on Apple Silicon](https://news.ycombinator.com/item?id=42908061) | 2025-02-02 | 309 | 75 |
| [OrbStack](https://news.ycombinator.com/item?id=41421846) | 2024-09-02 | 307 | 156 |
| [Switching to Claude Code and VSCode inside Docker](https://news.ycombinator.com/item?id=44533044) | 2025-07-11 | 265 | 161 |
| [Tart](https://news.ycombinator.com/item?id=39059100) | 2024-01-19 | 261 | 135 |
| [Conductor (Mac app, many Claude Codes)](https://news.ycombinator.com/item?id=44594584) | 2025-07-17 | 228 | 115 |
| [Shuru – local-first Linux microVMs for macOS](https://news.ycombinator.com/item?id=47113567) | 2026-02-22 | 213 | 66 |
| [Emdash](https://news.ycombinator.com/item?id=47140322) | 2026-02-24 | 206 | 71 |
| [Vibe Kanban](https://news.ycombinator.com/item?id=44533004) | 2025-07-11 | 195 | 132 |
| [Cua: drive macOS apps in background](https://news.ycombinator.com/item?id=47936312) | 2026-04-28 | 192 | 43 |
| [Parallel coding agents with tmux and Markdown specs](https://news.ycombinator.com/item?id=47218318) | 2026-03-02 | 189 | 122 |
| [Sculptor (Imbue)](https://news.ycombinator.com/item?id=45427697) | 2025-09-30 | 176 | 85 |
| [Embracing the parallel coding agent lifestyle (Simon Willison)](https://news.ycombinator.com/item?id=45489884) | 2025-10-06 | 174 | 138 |
| [NanoClaw moved from Apple Containers to Docker](https://news.ycombinator.com/item?id=47113731) | 2026-02-22 | 169 | 141 |
| [Agent37 – low-cost agent sandboxes](https://news.ycombinator.com/item?id=50059096) | 2026-10-12 | 163 | 136 |
| [Worktrees + tmux "LLM codegen go brrr"](https://news.ycombinator.com/item?id=44116872) | 2025-05-28 | 156 | 73 |
| [Lume 0.2 – macOS VMs with unattended setup](https://news.ycombinator.com/item?id=46670181) | 2026-01-18 | 154 | 44 |
| [Coding Agent VMs on NixOS with microvm.nix](https://news.ycombinator.com/item?id=46844410) | 2026-02-01 | 109 | 48 |
| [Superset launch (YC P26)](https://news.ycombinator.com/item?id=48236770) | 2026-05-22 | 108 | 135 |
| [Boxes.dev – run agents in the cloud](https://news.ycombinator.com/item?id=48399358) | 2026-06-04 | 105 | 79 |
| [Coasts – containerized hosts per worktree](https://news.ycombinator.com/item?id=47575417) | 2026-03-30 | 99 | 38 |
| [machine0 – persistent NixOS VMs from the CLI](https://news.ycombinator.com/item?id=48543245) | 2026-06-15 | 96 | 37 |
| [Container Use (Dagger)](https://news.ycombinator.com/item?id=44193933) | 2025-06-05 | 82 | 17 |
| [Ask HN: How are you sandboxing coding agents?](https://news.ycombinator.com/item?id=46400129) | 2025-12-27 | 46 | 32 |
| [Git worktrees are not an isolation boundary](https://news.ycombinator.com/item?id=49110389) | 2026-07-30 | 32 | 35 |
| [Ask HN: Why are so many rolling their own agent sandbox?](https://news.ycombinator.com/item?id=46699324) | 2026-01-20 | 32 | 18 |

Reading of popularity: the most adopted *parallel* layer is the **git worktree manager** (vibe-kanban ~29k, cmux ~29k, Paradev/Superset ~16.5k, claude-squad ~9k, emdash ~6k, Conductor closed but VC-funded). The most adopted *isolation* layer is **Docker/devcontainer** (Docker Sandboxes, Anthropic's reference devcontainer, claudebox, trailofbits). **Local VMs for agents** are the fastest-rising niche in 2026 (apple/container machines, smolvm 7k in under a year, shuru, vibe, Lima's agent features, microvm.nix), and HN's single biggest story in this space (1269 pts) was a Mac-native Linux VM. Cloud persistent VMs reached by SSH (exe.dev, Sprites, Boxes) get strong HN attention.

---

## B. Evidence items

Format: **summary** · URL · date · setup type · quote · engagement · pain or reason.

### B1. Docker, devcontainers and their pains

1. **Anthropic's own devcontainer doc says sign-in is lost on every rebuild unless you add a named volume and `CLAUDE_CONFIG_DIR`.**
   https://code.claude.com/docs/en/devcontainer · current doc (Nov 2026) · devcontainer
   > "By default, the container's home directory is discarded on rebuild, so engineers must sign in again each time."
   Engagement: official doc. Pain: auth persistence; also warns that `--dangerously-skip-permissions` in a container still lets a malicious project exfiltrate `~/.claude` credentials.

2. **Mac Claude Code deletes the credentials file the Linux container needs (Keychain).**
   https://github.com/anthropics/claude-code/issues/10039 · 2025-10-21 · Docker on Mac
   > "When I login to `claude` on my mac my credentials are stored into my keychain and the credentials file in `$HOME` gets deleted."
   Engagement: 2 👍, 7 comments; closed "not planned". Pain: auth across host ↔ container ("I have to login each time").

3. **OAuth does not persist in Docker even with the file mounted.**
   https://github.com/anthropics/claude-code/issues/22066 · 2026-01-31 · Docker
   > "OAuth authentication does not persist between container runs, despite the credentials file being correctly saved and mounted."
   Engagement: closed as duplicate of #1736 (a longer-running issue). Pain: auth.

4. **Login callback hangs inside a devcontainer (callback server listens on 127.0.0.1).**
   https://github.com/anthropics/claude-code/issues/31007 · 2026-03-05 · devcontainer on macOS
   > "when I get redirected to the callback URL (localhost with port 50XXX or something) my browser hangs."
   Engagement: auto-closed. Pain: browser sign-in across a VM/container boundary.

5. **Anthropic invalidates OAuth tokens moved to a machine other than the one that created them.** (Coasts founders tried to run Claude Max inside their containerized hosts.)
   https://news.ycombinator.com/item?id=47575434 · 2026-03-30 · Docker-in-Docker per worktree
   > "because the Coast runtime doesn't match the host machine the OAuth token is created on, Anthropic rapidly invalidates the OAuth tokens."
   Engagement: Show HN 99 pts / 38 comments. Pain: **subscription sign-in inside a different machine is fragile — directly relevant to Silo**.

6. **Lima users must sign in again inside the VM because the Mac keeps Claude auth in Keychain.**
   https://workmux.dev/guide/sandbox/lima.md · 2026 · Lima VM
   > "Claude stores auth in macOS Keychain, which is not accessible from the Linux VM."
   Pain: auth per VM. Also notes Lima's broad writable mounts let a rooted guest change host git hooks/config.

7. **Docker Sandboxes launch: the top complaint is that a local tool requires a login.**
   https://news.ycombinator.com/item?id=49239751 · 2026-08-10 · microVM per agent (Docker)
   > "Ah let's see, do they still want you to LOGIN, in order to use a local dev tool? Yes, yes they do. No thanks Docker."
   Engagement: 694 pts / 396 comments (biggest agent-sandbox launch of 2026). Pain: account lock-in, closed source; also "login was broken because they 'didn't consider' it would be run in a headless environment" ([49254012](https://news.ycombinator.com/item?id=49254012)).

8. **Same thread: memory cost of one VM per agent.**
   https://news.ycombinator.com/item?id=49241170 · 2026-08-10 · VMs per agent
   > "If you spawn 10 sandbox VMs, you already pay 5 GB RAM just to sit there idle."
   The commenter's fix: one Ubuntu VM with many LXC containers inside. Follow-up: memory release from VMs is "not a solved problem" ([49241257](https://news.ycombinator.com/item?id=49241257)). Pain: **RAM per VM — the core Silo trade-off; the fix people choose is close to Silo's "one main VM, many agents" model.**

9. **Docker Sandboxes mounts are limited.**
   https://news.ycombinator.com/item?id=49240288 · 2026-08-10
   > "you could not configure custom volume mounts, making more complex setups impossible. For work I need two directories"
   (A Docker employee replied it's now possible with extra arguments.) Pain: multi-repo access.

10. **Devcontainer CLI: port forwards must be rebuilt and unique per machine.**
    https://news.ycombinator.com/item?id=44549347 · 2025-07-13 · devcontainer without VS Code
    > "you have to rebuild the container when the port forwarding configuration changes, and the port forwards have to be unique on the entire machine."
    Engagement: thread 265 pts / 161 comments ("Switching to Claude Code and VSCode Inside Docker", https://timsh.org/claude-inside-docker/). Pain: ports, non-VS Code editors.

11. **Containers break the Mac app workflow.**
    https://news.ycombinator.com/item?id=44548403 · 2025-07-13
    > "Many people use a mix of web apps (AI studio), Mac apps (Wispr Flow), and other UI tools (repo prompt) in their workflow which can’t be put in a"
    Pain: tools that live on the Mac host don't follow the agent into a container.

12. **OrbStack's file sharing chokes on file watchers.**
    https://news.ycombinator.com/item?id=48472352 · 2026-06-10 · OrbStack
    > "containers that need file watchers like vite dev server, or frankenphp in watch mode will overload OrbStack real quick"
    Pain: bind-mount speed and watchers on macOS. Also: OrbStack's FS "is not POSIX compliant and causes breakage with some build systems" ([47114955](https://news.ycombinator.com/item?id=47114955), 2026-02-22).

13. **A dev-tool startup avoids Docker on Mac entirely.**
    https://news.ycombinator.com/item?id=47580145 · 2026-03-30
    > "We’ve tried to stay away from Docker as much as we can though because of the still pretty bad experience on Mac."
    Pain: Docker on macOS in general.

14. **Ask HN "How are you sandboxing coding agents?": a Mac developer needed Xcode and the iOS simulator, so containers were out.**
    https://news.ycombinator.com/item?id=46400151 · 2025-12-27 · macOS VM (ClodPod) and low-privilege user (SandVault)
    > "I use a Mac, and wanted to be able to run MacOS programs like Xcode and iOS simulator, so I wrote a couple of different sandbox projects"
    He uses the user-account option more "because it's easier to setup and doesn't require the overhead of a full VM". Thread: 46 pts / 32 comments; answers span Firecracker, Docker, LXC on Proxmox, bubblewrap, podman, separate Linux users, Catnip, Docker Sandboxes. Pain: **macOS tooling + VM overhead**.

15. **Same thread: many simply run unsandboxed with backups.**
    https://news.ycombinator.com/item?id=46400574 · 2025-12-27 · none (Time Machine)
    > "Worst thing it's done is back up a database, delete the database, and then run git clean locally which also wiped out the backup"
    Pain: real data loss even with backups; shows the "don't bother" baseline Silo competes with.

16. **Same thread: agents actively try to escape sandboxes.**
    https://news.ycombinator.com/item?id=46402650 · 2025-12-27
    > "cloned a workspace, edited the clone, then replaced the workspace w the clone to bypass file-path deny rules"
    Pain: path-based sandboxes are weak; argues for whole-machine boundaries.

17. **Ask HN "Why is everyone rolling their own sandbox?": no standard exists.**
    https://news.ycombinator.com/item?id=46715839 · 2026-01-22
    > "a bash script and a Dockerfile (coupled with dedicated user on linux system) seemed simpler than discovering and understanding some other, over complicated tool"
    And ([46716023](https://news.ycombinator.com/item?id=46716023)): "There's no "OAuth for syscalls" yet." And ([46769024](https://news.ycombinator.com/item?id=46769024)): agents "fail because the environment is flaky: missing deps, slow setup, weird state". Engagement 32 pts / 18. Pain: fragmentation; environment readiness matters more than isolation.

18. **Sculptor (Imbue): containers per agent with "pairing" sync back to your worktree; commenters ask about shared databases.**
    https://news.ycombinator.com/item?id=45428089 · 2025-09-30 · Docker + worktree sync
    > "So... are we all just working on various ways of using Claude Code in docker with git worktrees? Is that like, the whole world's project this month?"
    Engagement: 176 pts / 85; repo only 285 stars. Pain: databases ("sharing a postgres across containers may require a bit of manual tweaking", [45428341](https://news.ycombinator.com/item?id=45428341)).

19. **NanoClaw moved from Apple Containers to Docker for reach.**
    https://news.ycombinator.com/item?id=47113731 · 2026-02-22 · Apple container → Docker
    Engagement 169 pts / 141; NanoClaw has 31k stars. Signal: Mac-only isolation was dropped for portability once the project grew (the author's tweet is linked there). Pain: Mac-only runtimes limit audience.

20. **Agents need the whole stack, not just files.** (Ask HN on established codebases.)
    https://news.ycombinator.com/item?id=46292682 · 2025-12-16
    > "AFAICT, there’s no service that lets me: give a prompt, write the code, spin up all this infra, run Playwright, handle database migrations, and let me manually poke at the system."
    Engagement 70 pts / 66. Pain: **multi-service environments + browser testing + manual poking — the strongest description of the job Silo's "whole computer" does.**

### B2. Local VMs (Lima, Tart, Apple container, microVMs) and their pains

21. **Apple ships "container machines": persistent Linux VMs on the Mac. Memory defaults to half the host.**
    https://github.com/apple/container/blob/main/docs/container-machine.md · HN https://news.ycombinator.com/item?id=48469658 · 2026-06-10 · Apple Virtualization (Linux only)
    > "Container machines are fast, lightweight and persistent." / "Memory defaults to half of host memory."
    Engagement: **1269 pts / 437 comments**. No snapshots or forks are documented; nested virtualization needs M3+. Signal: Apple now gives away a persistent Linux VM; Silo's Linux main VM must beat it on forks, recipes, macOS guests and agent setup.

22. **Michael Stapelberg: ephemeral NixOS microVMs per agent, because rebuilding stateful VMs is painful.**
    https://michael.stapelberg.ch/posts/2026-02-01-coding-agent-microvm-nix/ · 2026-02-01 · microvm.nix (Linux host)
    > "Instead of setting up a stateful VM and re-installing it when needed (ugh!), I prefer the model of ephemeral VMs"
    HN 109 pts / 48. Setup touches six files plus a manual SSH host key per VM, and each VM gets 4 GB RAM. A Mac user asks ([46889551](https://news.ycombinator.com/item?id=46889551)) "Is there a way to make this work with macOS hosts". Pain: setup effort; Nix recipes are attractive but Mac support is missing.

23. **lynaghk/vibe: one-command Linux VM per project on Mac. Its gaps are exactly the networking Silo needs.**
    https://github.com/lynaghk/vibe · 2026 · Virtualization.framework, VirtioFS shares
    > "Virtualization is more secure against malicious escapes than containers or the MacOS sandbox framework."
    Limits per README: "The host cannot reach the Vibe VMs, nor can VMs reach each other"; no port forwarding yet; ~10 s boot on M1 Air; `~/.claude.json` needs a symlink hack. Positive: APFS copy-on-write keeps a 100 GiB image at ~2.5 GB on disk. 966 stars.

24. **Shuru: Linux microVMs on macOS, ephemeral by default, with checkpoints and branching.**
    https://news.ycombinator.com/item?id=47113567 · 2026-02-22 · Virtualization.framework
    First question: "What is the benefit of this over lima, for example?" ([47114097](https://news.ycombinator.com/item?id=47114097)); author: "the difference is mostly in defaults". Engagement 213 pts / 66; repo 897 stars. Signal: demand for good defaults over raw Lima.

25. **smolvm: local, portable VMs that branch as live copy-on-write children.** (Direct analogue of Silo forks.)
    https://github.com/smol-machines/smolvm · 2025-12 onward · libkrun on Hypervisor.framework
    > "branches are copy-on-write children of a live machine."
    7,183 stars in under a year; boots "in under a second"; memory "elastic via virtio balloon"; no SSH described. Competitive signal.

26. **zeroboot: sub-millisecond VM sandboxes by CoW memory forking.**
    https://news.ycombinator.com/item?id=47412569 · 2026-03-17 · Linux/KVM
    Engagement 311 pts / 72; 2,490 stars. Signal: forking is a hot idea, though for server sandboxes, not a dev's Mac.

27. **Atomic Object: a Lima VM so the agent sees only the project.**
    https://spin.atomicobject.com/lima-ai-coding-agent-sandbox/ · 2026-03-16 · Lima
    > "there was no feedback and it would occasionally time out."
    (About provisioning tools inside the VM.) Adding a second project means editing config, mounts and aliases by hand. Pain: provisioning UX, multi-repo in one VM.

28. **"I gave my coding agent its own computer" (Linux VM on Apple Silicon, snapshot before each session).**
    https://dev.to/deland/i-gave-my-coding-agent-its-own-computer-4l99 · 2026 (posted Aug 17, edited Oct 8) · Virtualization.framework
    > "It was giving the agent its own computer."
    Pains listed (summarized from the page): disk images grow and don't shrink; a writable share of the real project defeats isolation; taking the snapshot after the mistake cost a session. Signal: people want snapshots and are learning disk hygiene by hand.

29. **Lume 0.2 (Cua): macOS VMs with unattended setup; users asked for external drives because Mac disks fill.**
    https://news.ycombinator.com/item?id=46670181 · 2026-01-18 · macOS VMs on Virtualization.framework
    > "macOS disk space is always tight, so from user feedback we added support for external drives."
    154 pts / 44. Pain: **disk size of macOS VMs on laptops**.

30. **Flavio Copes, "testvm": a Tart macOS VM so agents can test Mac apps without stealing his focus.**
    https://flaviocopes.com/mac-test-vm/ · updated 2026-10-06 · Tart over SSH
    > "The app jumps in front of whatever I'm doing and takes the focus."
    > "It's a 25 GB download, which took about half an hour here."
    He considered buying a Mac mini first. VM uses 4 cores, 8 GB RAM, 33 GB disk, ~15 s start. Pain: **focus stealing (reason for a VM) vs first-run download and disk (cost of a VM)**.

31. **Apple confirms the hard cap of two macOS VMs per Mac (Linux VMs uncapped).**
    https://developer.apple.com/forums/thread/845455 · Sep 2026 · Virtualization.framework
    > "Virtualization framework has a hard cap at two per machine."
    Apple DTS adds that the cap applies to Mac VMs only, with "no fixed cap for Linux VMs". The poster could not find any way to get Apple's written permission for more. Pain: **limits Silo's macOS forks to two running at once**.

32. **Cua: computer use on the host takes over the human's session, which is why people used VMs.**
    https://news.ycombinator.com/item?id=47936312 · 2026-04-28 · macOS GUI automation
    > "Your cursor moves, keyboard focus gets stolen, windows jump to the front, and you have to stop working until the agent is done."
    192 pts / 43. Signal: a macOS VM is today's standard fix for agents that use the GUI, but background-driving tools are emerging as a lighter alternative.

33. **Claude Code Desktop's iOS Simulator pane works only in local sessions, not over SSH.**
    https://code.claude.com/docs/en/desktop-ios-simulator.md · Nov 2026 · Claude Desktop on macOS
    > "In cloud and SSH sessions, Claude runs on a machine that can't reach the simulators on your Mac."
    It also states that parallel sessions reuse an already running simulator unless you name a different one. Pain for Silo: **an SSH-reached VM loses this first-party feature unless the VM runs Xcode and its own simulators, and the agent drives them through computer use or simctl.**

34. **Parallel agents race for the same iOS simulator.**
    https://github.com/kyleve/Stuff/pull/141 · 2026-07-26 · worktrees + clones on one Mac
    > "Agents working in parallel clones and worktrees raced each other booting it"
    Fix: one simulator per checkout by UDID. Pain: shared Mac-global state (simulators).

35. **Xcode keeps global state that worktrees don't isolate.**
    https://developer.apple.com/forums/thread/833677 · Jun 2026 · worktrees with Xcode
    > "While Git handles the isolated source files perfectly, Xcode struggles because it relies heavily on global, centralized states behind the scenes."
    It adds that DerivedData collisions cause "incremental build corruption". Pain: **this argues for a macOS VM fork per agent for iOS work, within the 2-VM cap**.

36. **Cirrus Labs (Tart) joined OpenAI's Agent Infrastructure team; Cirrus CI shut down on 2026-06-01.**
    https://cirruslabs.org/ (announcement), HN https://news.ycombinator.com/item?id=47730194 ("Cirrus Labs to join OpenAI"; id taken from HN mirrors) · 2026-04-07 · Tart
    Tart now lives at openai/tart (8,008 stars). Signal: a frontier lab bought the leading macOS-VM tooling for agents; HN worried Tart might go unmaintained.

37. **Lima v2.1 adds experimental macOS guests and agent hardening.**
    https://www.cncf.io/blog/2026/03/25/lima-v2-1-macos-guests-and-enhanced-ai-agent-safety/ · 2026-03-25 · Lima
    > "now experimentally supports running macOS guests using the vz driver"
    Signal: the open-source default is moving toward Silo's space (macOS and Linux VMs for agents).

### B3. Spare machines, VPS and cloud VMs

38. **Mac mini shortage driven by people buying always-on agent hosts (OpenClaw).**
    https://www.tomshardware.com/tech-industry/artificial-intelligence/openclaw-fueled-ordering-frenzy-creates-apple-mac-shortage-delivery-for-high-unified-memory-units-now-ranges-from-6-days-to-6-weeks · and https://techcrunch.com/2026/04/24/mac-mini-price-expensive-ebay-shortage-ai-memory/ · Apr–May 2026 · dedicated Mac
    Per those reports (search summary), Tim Cook said on the Q2 2026 call that Mac mini and Mac Studio were sold out and could stay so for "several months"; high-memory configurations had waits of weeks. Signal: **people buy a second Mac to give agents their own computer**. Silo's pitch "a whole computer for agents on the Mac you already own" competes with that purchase.

39. **A Linux box freed up by a Mac mini becomes an agent playground.**
    https://news.ycombinator.com/item?id=50061261 · 2026-10-12 · spare machine
    > "I got a mac mini to use as my own personal driver and now my Linux box is free, so I want to have a local setup where I keep my personal machine a bit stricter while giving near free-reign for agents"
    Pain: wants hands-off separation without a second desk.

40. **Hetzner box + tmux + phone, so "the laptop's specs stop mattering".**
    https://jeangalea.com/claude-code-hetzner-box/ · 2026-06-19 · VPS over SSH
    > "The laptop's specs stop mattering, because nothing heavy runs on the laptop."
    Pains named (page summary): a headless server has no Keychain, 1Password or Touch ID, so secrets are a risk; unattended jobs can't use a forwarded key; no offline use. Cost: a few euros a month.

41. **exe.dev: persistent VMs reached over SSH.**
    https://news.ycombinator.com/item?id=46397609 · 2025-12-26 · cloud VMs over SSH
    457 pts / 297 comments; exe.dev's blog later charted repeatedly (e.g. "Devtools must be open source", 733 pts). Signal: **SSH-first persistent VMs are something people pay for**.

42. **Sprites (fly.io): persistent sandboxed VMs that start fast, with checkpoints. Simon Willison's praise is the top comment.**
    https://news.ycombinator.com/item?id=46561163 · 2026-01-09 · cloud VM
    > "a cheap and convenient way to run Claude Code / Codex CLI / etc in YOLO mode in a persistent sandboxed VM with a restricted blast radius"
    508 pts / 189 comments. Others in the thread: "I really hate that modern development means not having persistent disk." ([46562873](https://news.ycombinator.com/item?id=46562873)); "I want something like this, but running on my own box." ([46563966](https://news.ycombinator.com/item?id=46563966)); "at work I use Claude SSHed to a persistent dev server" ([46573819](https://news.ycombinator.com/item?id=46573819)). Signal: **the persistent-VM-plus-checkpoints model is validated; a local version is explicitly asked for.**

43. **Boxes.dev: cloud boxes forked from a template snapshot (disk and RAM). Users push back on cloud lock-in and secrets.**
    https://news.ycombinator.com/item?id=48399358 · 2026-06-04 · Firecracker microVMs
    > "I might use this if it supported any old cloud or VPS"
    And ([48400878](https://news.ycombinator.com/item?id=48400878)): "How does this handle MCP credentials ... Either way those secrets end up in your cloud?" And "Why would I want this and not the other way around?" ([48401619](https://news.ycombinator.com/item?id=48401619)). 105 pts / 79. Signal: fork-from-snapshot is the speed trick; self-hosting and secrets-in-cloud worry.

44. **Agent37 launch: a MacBook can't carry emulators plus agents, and cloud sandbox credits come with restrictions.**
    https://news.ycombinator.com/item?id=50060249 · 2026-10-12 · cloud sandbox
    > "I've been working with a lot of Android apps and emulation is killing me on top of everything else that I'm running."
    (M1 Pro, 32 GB.) Same comment: Daytona credits "close to useless given their restrictions". Another ([50063391](https://news.ycombinator.com/item?id=50063391)): "scoped, short lived credentials per task. that i would pay" for. 163 pts / 136. Pain: **local RAM ceiling; credential scoping is a pay-for item**.

45. **"Claude Code is too cloudy": the cloud version is too limited, the local one too annoying. The author built a home-machine runner.**
    https://www.brendanlong.com/claude-code-is-too-cloudy.html · 2026-01-24 · home computer + Podman containers
    > "the cloud environment is so limited"
    Pain: cloud sandboxes lack the packages, containers, caches and GPU that a home machine gives.

46. **Terragon (cloud background agents in containers) shut down and dumped its code.**
    https://github.com/terragon-labs/terragon-oss · 2026-01-16 · cloud containers
    Summary of the sources found: a snapshot "at the time of shutdown", provided as-is. Signal: thin cloud wrappers get squeezed by the labs' own background agents.

### B4. Git worktrees and parallel-agent managers

47. **Worktrees + tmux made parallel agents popular; the first reply names the cost.**
    https://news.ycombinator.com/item?id=44118327 · 2025-05-28 · worktrees + tmux (uzi)
    > "Now you can do 4X more code reviews!"
    156 pts / 73. Also: "my bottleneck with LLMs on a real project is reviewing their code, QAing it" ([44119733](https://news.ycombinator.com/item?id=44119733)). Pain: review load.

48. **Conductor launch: cloning per workspace means reinstalling dependencies everywhere.**
    https://news.ycombinator.com/item?id=44628011 · 2025-07-20 · worktrees (Mac app)
    > "you need to run all the dependency installs, etc. for every workspace before being able to test anything."
    He adds that his project has too many dependencies for cloud agents, "plus there would be DNS conflicts, external API conflicts". 228 pts / 115. Conductor's later answer is setup scripts and ten ports per workspace (`CONDUCTOR_PORT`…`+9`, https://www.conductor.build/docs/reference/environment-variables).

49. **Worktrees isolate files, not databases or services.**
    https://news.ycombinator.com/item?id=46424131 · 2025-12-29 · Superset (worktrees)
    > "Most of these agents solutions are focusing on git branches and worktrees, but at least none of them mention databases."
    He would "need ten different copies of my database", plus redis and celery. Another reply ([46427339](https://news.ycombinator.com/item?id=46427339)) wants a tool to "manage the ports (allocate unique set for each worktree, add those to .env)". Show HN 96 pts / 90.

50. **Superset launch: "I've never used lsof so much in my life."**
    https://news.ycombinator.com/item?id=48237005 · 2026-05-22 · worktrees IDE
    > "I agree with the hard part being managing state, especially environments and ports. I've never used lsof so much in my life."
    He also asks whether a remote machine can port-forward so a browser can test the app there. 108 pts / 135.

51. **Emdash: every worktree needs its own ports and Docker volumes.**
    https://news.ycombinator.com/item?id=47146744 · 2026-02-25 · worktrees, local or over SSH
    > "every worktree needs its own ports, and specific particularities (e.g. so docker volumes don't collide)."
    206 pts / 71. Emdash also supports running agents over SSH on a remote machine, which is the shape Silo takes.

52. **Coasts exists only because local runtimes can't be duplicated per worktree.**
    https://news.ycombinator.com/item?id=47575417 · 2026-03-30 · Docker-in-Docker per worktree
    > "You can do it up to a point with port hacking tricks, but it becomes impractical when you have a complex docker-compose with many services and multiple volumes."
    (From the Show HN post text.) 99 pts / 38; 434 stars; inactive since April 2026.

53. **Trigger.dev abandoned worktrees.**
    https://trigger.dev/blog/parallel-agents-gitbutler · 2026-04-16 · worktrees → GitButler virtual branches in one checkout
    > "we spent more time on setup than shipping code."
    Pains named: "Agent B's schema is now out of sync", per-worktree ports and Postgres/Redis copies, fresh installs, and "resolving conflicts in code you didn't write". Signal: **some teams prefer one shared environment with many agents, which is Silo's main-VM model.**

54. **Cursor auto-created worktrees filled a disk.**
    https://forum.cursor.com/t/windows-request-to-disable-automatic-worktree-creation-critical-disk-space-issue/146189 · 2025-12-13 · Cursor parallel agents
    > "Each worktree duplicates the entire repository, consuming significant disk space."
    From the page summary: two worktrees of a ~2 GB repo took 9.82 GB in 20 minutes, and 20+ worktrees reached ~140 GB in a week. Pain: **disk**.

55. **Claude Code feature request: worktrees lack `.venv`, `.env`, `node_modules`.**
    https://github.com/anthropics/claude-code/issues/27744 · 2026-02-22 · Claude Code `--worktree` / subagent isolation
    > "This correctly copies all tracked files but **does not carry over gitignored state**: most critically `.venv/` and `.env`."
    29 👍; closed 2026-08-17 with `.worktreeinclude`, a `WorktreeCreate` hook and `worktree.symlinkDirectories`.

56. **Claude Code feature request: let background agents skip worktrees and reuse `node_modules`.**
    https://github.com/anthropics/claude-code/issues/60113 · 2026-05-18
    > "To actually run the project (start the dev server, run tests, build) inside the worktree, I have to reinstall dependencies from scratch."
    7 👍; shipped as `bgIsolation: "none"` and `symlinkDirectories`. Signal: Anthropic now forces worktree isolation for background agents by default, and users push back on the cost.

57. **"Git worktrees are not an isolation boundary".** Hooks, config and stash are shared.
    https://fletch.sh/blog/git-worktrees-vs-clones-for-ai-agents/ · 2026-07-30 · worktrees vs `git clone --shared`
    > "A worktree was never a boundary."
    HN 32 pts / 35. Commenters: "what’s a pain is having to compile from scratch every time, and not carrying over gitignored files" ([49110949](https://news.ycombinator.com/item?id=49110949)); with nested worktrees "grep would yield results from a worktree and the agent would happily start to work in the wrong worktree" ([49111444](https://news.ycombinator.com/item?id=49111444)); one user wrote hooks to copy `node_modules` and `.env` and give each worktree its own compose project name ([49111504](https://news.ycombinator.com/item?id=49111504)).

58. **"Git worktrees are global mutable state."**
    https://news.ycombinator.com/item?id=45484252 · 2025-10-05 · LXC containers + shallow clones
    > "Git worktrees are global mutable state; all containers on your laptop are contending on the same git database."
    He uses shallow checkouts per LXC container instead.

59. **Teams copy Docker volumes per worktree with wrapper scripts.**
    https://news.ycombinator.com/item?id=46572297 · 2026-01-11
    > "We have a few wrapper scripts around worktree operations that copy some docker volumes (pg data, bundle cache, etc.) from the base and spins up an entirely new stack on different ports"
    Signal: **people are already hand-making "forks" of environment state. A whole-VM CoW fork does this in one step.**

60. **Vibe Kanban: parallel agents clash and the human loses context.**
    https://news.ycombinator.com/item?id=44533339 · 2025-07-11 · worktree per task
    > "whenever i try to parallelize, they clash while editing files simultaneously, i lose mental context of what's going on, they rewrite tests etc."
    195 pts / 132; repo 28.8k stars.

61. **Simon Willison's parallel-agents post: review is the bottleneck, and burnout worries.**
    https://news.ycombinator.com/item?id=45486217 · 2025-10-05
    > "I also fire off tons of parallel agents, and review is hands down the biggest bottleneck."
    Also: "I'm not convinced there is any hope for a productive, long-term, burnout-free parallel agent workflow." ([45531488](https://news.ycombinator.com/item?id=45531488)). 174 pts / 138.

62. **tmux + specs setup: merge conflicts and cognitive load.**
    https://news.ycombinator.com/item?id=47222888 · 2026-03-02
    > "The merge conflicts and cognitive load are indeed two big struggles with my setup."
    189 pts / 122.

---

## C. Synthesis

### C1. What setups are most common (late 2026)

1. **Run on the host, maybe with the built-in sandbox, and rely on git and backups.** Still the majority by the comment evidence (items 15, 47). Nothing to install, so it's the baseline Silo must beat on friction.
2. **Git worktrees + a manager** (vibe-kanban, cmux, Conductor, Superset/Paradev, claude-squad, emdash, Crystal, and now Claude Code's and Cursor's built-in worktrees). The dominant *parallel* pattern by stars and launches. It handles files only.
3. **Docker / devcontainer per agent or per project** (Anthropic's reference devcontainer, Docker Sandboxes, claudebox, container-use, trailofbits, Sculptor). The dominant *isolation* pattern, and Anthropic's documented path for `--dangerously-skip-permissions`.
4. **A local Linux VM** (Lima, OrbStack machines, Apple container machines, vibe, shuru, smolvm, microvm.nix, UTM). Growing fastest in 2026. Chosen for a real kernel boundary and to run Docker inside.
5. **A remote always-on box over SSH + tmux** (Hetzner/VPS, exe.dev, Sprites, Boxes, Codespaces-likes) or a **second physical machine** (Mac mini, old Linux box). Chosen for always-on use, phone access, and to stop the agent taking over the laptop.
6. **A macOS VM** (Tart, Lume, ClodPod, testvm) or a **low-privilege macOS user** (SandVault), for Xcode, the simulator and GUI testing. Small but distinct: Linux containers cannot serve these users.

Most people stack two: worktrees for parallelism plus a container or VM for safety.

### C2. Top recurring pains, ranked (frequency × severity)

| # | Pain | Frequency | Severity | Items |
|---|---|---|---|---|
| 1 | **Runtime state isn't isolated: ports, databases, Redis, docker-compose stacks collide across worktrees** | very high | high (blocks testing) | 20, 48–53, 59 |
| 2 | **Per-copy setup cost: reinstall deps, copy .env, rebuild caches, compile from scratch** | very high | medium–high | 48, 53, 55–57 |
| 3 | **Review overload and human context loss** | very high | high (caps value) | 47, 60–62 |
| 4 | **Sign-in and credentials across the boundary** (Keychain, OAuth callback, tokens invalidated off-host, secrets in cloud) | high | high (breaks the loop every rebuild) | 1–7, 40, 43, 44 |
| 5 | **Disk**: worktrees duplicate repos (140 GB), macOS VM images 25–33 GB, images don't shrink | medium–high | medium | 29, 30, 54, 28 |
| 6 | **Docker-on-Mac friction**: bind-mount speed, file watchers, non-POSIX FS, licence costs, login walls | medium–high | medium | 7, 12, 13 |
| 7 | **macOS-only tooling**: Xcode, simulator, GUI apps, focus stealing; Xcode's global state defeats worktrees | medium (a segment) | very high for that segment (no Linux fix) | 14, 30, 32–35 |
| 8 | **Memory per VM / laptop RAM ceiling** | medium | medium–high | 8, 21, 44 |
| 9 | **Fragmentation / DIY**: no standard, everyone writes scripts | high | low–medium | 17, 22, 27 |
| 10 | **Weak isolation**: worktrees share hooks/config; agents bypass path rules | medium | medium–high | 16, 57, 58 |
| 11 | **Lock-in / cloud mistrust**: closed tools, login walls, start-ups that shut down | medium | medium | 7, 43, 46 |

### C3. What a whole VM with CoW forks on the Mac, reached by SSH, would remove

- **Pain 1 (runtime collisions), largely.** A fork carries its own localhost, Postgres data, Redis, Docker daemon and compose stack, so ports and DBs no longer collide and no port allocator or `.env` rewriting is needed. People already copy pg volumes and bundle caches by script (item 59); a CoW fork does it in one step and cheaply. This is the clearest unmet job in the evidence (items 20, 49, 52).
- **Pain 2 (setup per copy), largely.** A fork inherits installed `node_modules`, `.venv`, build caches and DerivedData. Nothing to reinstall, no `.worktreeinclude`, no symlink hacks (items 55–57).
- **Pain 10 (weak isolation).** A VM is a real boundary: no shared `.git/hooks` with the host, and no path-rule games (items 16, 57).
- **Pain 6 (Docker-on-Mac friction), partly.** If code lives *inside* the VM and editors attach over SSH (VS Code/Cursor Remote-SSH, Zed), there is no bind mount to be slow and file watchers run natively in the guest. Docker runs inside the VM without Docker Desktop's licence or a login wall.
- **Pain 7 (macOS tooling), for up to two forks.** A macOS main VM can run Xcode and simulators. Each fork gets its own DerivedData and simulator set, which ends the races in items 34–35. Agents that use the GUI stop stealing the user's focus (items 30, 32).
- **Pain 9 (DIY).** Nix recipes replace each person's Dockerfile + bash script + Lima YAML.
- **Persistence.** A long-lived main VM answers "I really hate that modern development means not having persistent disk" and "I want something like this, but running on my own box" (item 42) without a monthly cloud bill or secrets leaving the Mac.

### C4. What it would NOT remove, or could make worse

- **Review overload (pain 3).** More parallel forks mean *more* diffs to review. Silo needs a story for reviewing and merging fork results, or it amplifies the top complaint.
- **Merge conflicts between agents.** Forks diverge just like worktrees, and conflicts still arrive at merge time (items 53, 62).
- **Memory per VM (pain 8), worse with each fork.** Every running fork pays a guest kernel and its services, and balloon memory return is "not a solved problem" (item 8). Apple's own default is half the host's RAM (item 21). On a 16–24 GB MacBook, three or four forks with a dev stack each will hit swap. Mitigations people would expect: suspend idle forks, balloon, run many agents in one main VM (as the commenter in item 8 does), and show RAM use per fork.
- **macOS VM cap (two running at once).** Hard-enforced by Virtualization.framework and the EULA (item 31). A macOS main VM plus one macOS fork uses up the allowance; macOS forks beyond that must be suspended or be Linux. This must be clear in the product, not discovered by surprise.
- **Disk and first-run download (pain 5).** A macOS guest is a ~25 GB download and ~33 GB on disk before any work (item 30); Lume users asked for external drives (item 29). CoW forks help, but the base image cost lands on day one. Linux guests are far smaller.
- **Sign-in (pain 4), a real risk.** Each VM is a different machine: Keychain doesn't cross, OAuth callbacks on `localhost` break across boundaries (item 4), and Anthropic may invalidate tokens that move between machines (item 5). Silo must make agents sign in *inside* the VM (or forward credentials safely) and must check that a fork's copied token stays valid. A fork that copies a live token is exactly the off-host token case.
- **File sync to the host editor.** SSH editors work, but Mac-native apps that read local folders (Xcode on the host, Finder, Tower, Wispr Flow, Repo Prompt) do not see files inside the VM (item 11). If Silo shares folders back to the Mac instead, the bind-mount slowness returns.
- **Browser previews.** Each fork's dev server needs to reach a host browser: per-fork URLs or forwarded ports are required (requested in items 23 and 50). Without them, "no port clashes" turns into "can't open the app".
- **First-party features that assume local.** Claude Desktop's iOS Simulator pane does not work in SSH sessions (item 33). Silo users would lose it unless the agent drives simulators inside the macOS VM through computer use or simctl.
- **Competition is moving in.** Apple ships free persistent Linux VMs (item 21); Lima added macOS guests (item 37); smolvm has local CoW branching with 7k stars (item 25); OpenAI owns Tart (item 36); Docker Sandboxes is free locally (item 7). Silo's edge must be the combination: macOS *and* Linux, a long-lived main VM shared by many agents and repos, forks, Nix recipes, and SSH that every agent app already speaks.

### C5. What people say they'd pay for

Moved out of this public repository, as AGENTS.md asks for business material. It is in Linear, in the project Insights.

### C6. Decisions to challenge or test, in Silo terms

- **Sign-in inside forks.** Test whether a Claude subscription token copied into a CoW fork keeps working, or gets invalidated as in item 5. If it breaks, forks need a sign-in step or a credential broker, and "fork in one step" stops being one step.
- **The macOS fork budget.** With the 2-VM cap, "main macOS VM + forks" allows exactly one running macOS fork. Decide whether macOS forks get suspended or queued, and say so in the product.
- **Memory on 16–24 GB Macs.** Measure idle and loaded RAM for the main VM plus N Linux forks. The evidence says this is the first wall users hit with VM-per-agent setups.
- **Review and merge.** Forks multiply output. A fork-diff or fork-merge view is how Silo avoids amplifying the top complaint.

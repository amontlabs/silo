# Parallel coding-agent apps: research report (read 24 Nov 2026)

Method: I used first-party pages (homepages, docs, changelogs, GitHub READMEs and issue lists) through WebFetch. `gh` API access was blocked in this session and `docs.imbue.com` did not resolve (it is not on the session's allowed network domains). Where I fell back to a third-party source, I mark it **[3rd-party]**.

## Conductor (Melty Labs, now owned by Meta)
1. A Mac app that runs several coding agents side by side: "Run parallel Claude Code, Codex, and Cursor agents in isolated workspaces on your Mac" (https://conductor.build). It is for individual developers and teams. It claims "more than 100,000 people use it" **[3rd-party, SiliconANGLE quoting Conductor]** (https://siliconangle.com/2026/11/04/meta-acquires-ai-coding-startup-conductor-whose-app-runs-several-coding-agents-at-once/).
2. Runs on macOS only (Apple Silicon). Conductor Cloud (0.78.0, 30 Jul 2026) runs workspaces in Vercel Sandboxes: "8 vCPUs, 16 GB of RAM, and 32 GB of ephemeral NVMe", in us-east-1, GitHub repos only (https://www.conductor.build/docs/cloud/faq, read via search snippet). No macOS guest.
3. Isolation is git worktrees only. The docs say so plainly: "Workspace isolation is development isolation, not a security boundary"; agents "run under your user permissions" (https://www.conductor.build/docs/concepts/workspaces-and-branches). A cloud workspace runs "in an isolated Linux sandbox" (https://www.conductor.build/docs/cloud-beta). Vercel Sandbox runs on Firecracker microVMs (from Vercel's own docs, not Conductor's).
4. Recording: the agent transcript and the diff per workspace, plus a `.context` folder for notes. Nothing is recorded at the process or network level.
5. Going back: per-turn checkpoints, starting with "checkpointing to Codex sessions… reset to a previous turn" (0.44.0, Mar 2026, https://www.conductor.build/changelog/0.44.0-new-sidebar-rebuilt-composer-codex-checkpoints). This works at chat-turn level; the docs do not say what it restores, and nothing restores the machine.
6. Network limits: not found in docs.
7. Secrets: not found in docs. Agents run as the user, so they see whatever the user's environment holds.
8. Spend: added this month. 0.96.0 (18 Nov): "your usage limits, and the chat's estimated cost". 0.98.0 (20 Nov): a Usage page showing "chats, estimated cost, and tokens… last 7, 30, or 90 days, broken down by model and by workspace" (https://www.conductor.build/changelog/0.98.0-see-where-your-usage-goes). These are estimates only; no budget is enforced.
9. Agents: Claude Code, Codex, Cursor (0.63, Jun) and OpenCode (0.69, Jun). It bundles pinned versions of the agent CLIs, which power users dislike **[3rd-party]** (https://dev.to/gentic_news/conductor-vs-claude-code-pinned-versions-split-the-community-39ia).
10. Price: Teams is $60/user/month **[search snippet of conductor.build/pricing; the page itself rendered no prices]**. Pro users got paid features free after the Meta deal. No cloud compute charge "right now", with usage-based pricing planned.
11. Closed source.
12. 2026 events:
    - $22M Series A in March.
    - **Meta acquisition announced 4 Nov** (changelog 0.93.0, "Conductor is joining Meta"), with sign-ups closed and Cloud shutdown planned for 20 Nov.
    - **Reversed 13 Nov**, changelog 0.94.2: "Conductor isn't shutting down… It's back, and we're building again" (https://www.conductor.build/changelog/0.94.2-conductor-is-back).
    - A "new interface" is promised for early 2027.
- **Overlap with Silo's plans:** many agents at once on the Mac; a spend view broken down by model.
- **What Silo adds:** a real security boundary (Conductor's docs disclaim one); machine-level recording and checkpoints; spend that is enforced, not estimated; network and secret control.
- **Gap or idea for Silo:** a "Transfer to local / cloud" move of a workspace; a per-workspace port variable (`CONDUCTOR_PORT`); a `.context` handoff folder.

## Sculptor (Imbue)
1. "A desktop app for running coding agents in parallel"; an "experimental research preview" (https://github.com/imbue-ai/sculptor).
2. Runs on Mac (Apple Silicon), Linux and Linux ARM64. Intel Mac and Windows are "coming" (https://imbue.com/sculptor/). No macOS guest.
3. Isolation has changed. The marketing page still says "Every agent runs in its own container" (https://imbue.com/sculptor/). The current README describes workspaces as "isolated worktrees of your repo" and lists the "Container Backend: running in Docker or on a remote (experimental)" (https://raw.githubusercontent.com/imbue-ai/sculptor/main/README.md). **So Docker containers, Sculptor's original selling point, appear to be optional and experimental now, with worktrees as the default.**
4. Recording: "saves every agent session with its plans, chats, tool calls, and code changes" (imbue.com/sculptor). This comes from the transcript, inside the agent.
5. Going back: "Fork… spin off a new agent from any point in your session history" is listed as upcoming (imbue.com/sculptor). It works at conversation level.
6. Network limits: not found in docs.
7. Secrets: not found in docs.
8. Spend: not found.
9. Agents: Claude Code and the Pi harness built in; it can run "any terminal-based agents" (README).
10. Price: free (imbue.com).
11. Open source, MIT.
12. Open-sourced in 2026; Pi harness support added.
- "Pairing mode": "With one click, Sculptor brings an agent's work from its container into your local repo", syncing both ways so the agent sees your edits (imbue.com/sculptor).
- **Overlap with Silo's plans:** an isolated environment per agent, with the agent in YOLO mode inside it (Imbue on HN: containers let agents "run in YOLO mode"); fork from history.
- **What Silo adds:** a VM rather than Docker; outside-the-box recording; fork of the machine, not just the chat.
- **Gap or idea for Silo:** **two-way live sync between the agent's box and the user's local checkout ("pairing")**, so the user can run and test the agent's work in their own IDE. Silo should decide how a user "pairs" with a VM. Also: AI "Suggestions" that flag problems in agent code.
- **Lesson:** a container-first product retreated to worktrees, likely because of startup cost and dependency setup. Imbue's docs admit "containers have a startup cost too with building environments and downloading packages" (via search snippet). **Silo must keep VM start and environment setup fast and cached, or it meets the same fate.**

## Crystal, now Nimbalyst (Stravu)
1. Crystal: "Run multiple Codex and Claude Code AI sessions in parallel git worktrees". **Deprecated Feb 2026** and replaced by Nimbalyst (https://github.com/stravu/crystal). Nimbalyst is "an extensible visual workspace for working with coding agents", with editors for markdown, diagrams, mockups and spreadsheets (https://nimbalyst.com).
2. Runs on macOS, Windows and Linux, with an iOS companion app. No macOS guest.
3. Isolation: an optional worktree: "Any session can opt into its own git worktree with one click."
4. Recording: "full visibility into context, actions, and outputs", plus the list of files each session modified. This is transcript level.
5. Going back: not found.
6. Network limits: not found.
7. Secrets: not found.
8. Spend: not found.
9. Agents: Claude Code and Codex; OpenCode, Copilot and Codex via ACP are in alpha.
10. Price: "completely free for individual users"; Teams is "Free during Beta".
11. Open source, MIT (desktop and iOS apps).
12. The Crystal-to-Nimbalyst pivot.
- **What Silo adds:** everything about the machine.
- **Gap or idea for Silo:** none for the core product. Nimbalyst is moving toward documents and planning beside the agent.

## Vibe Kanban (BloopAI)
1. A kanban board on which each issue spawns an agent workspace with "its own branch, terminal, and dev server" (https://github.com/BloopAI/vibe-kanban).
2. Runs locally through `npx`, on Mac, Linux and Windows, or self-hosted in Docker.
3. Isolation: git worktrees (it has an env var for "git worktree cleanup including orphan and expired workspace cleanup").
4. Recording: transcript, plus an inline diff whose review comments go back to the agent; a built-in preview browser.
5. Going back: not found.
6. Network limits: not found.
7. Secrets: not found.
8. Spend: not found.
9. Agents: 10+, including Claude Code, Codex, Gemini, Copilot, Amp, Cursor, OpenCode, Droid and Qwen.
10. Price: free now (subscriptions were refunded).
11. Open source, Apache-2.0.
12. **Bloop shut down on 10 Apr 2026.** Co-founder Louis Knight-Webb: "the vast majority are free users and we couldn't find a business model that we could get excited about" (https://vibekanban.com/blog/shutdown, via search). The remote services were removed and the project is now community-maintained and fully local; the README says "Vibe Kanban is sunsetting."
- **Top-voted issues** (https://github.com/BloopAI/vibe-kanban/issues?q=sort:reactions):
  - "Bring back old UI" (#2687)
  - "Configurable automatic cleanup of worktrees" (#765)
  - multi-repo projects (#705)
  - self-hosted GitLab (#1697)
  - "Why do I need to log in…" (#2952)
- **Lesson:** a free, local orchestration UI alone did not make a business.

## Terragon (Terragon Labs)
1. "A remote background agent orchestrator for running Claude Code, Codex, and other coding CLIs in the cloud" (https://github.com/terragon-labs/terragon-oss).
2. Ran in their cloud; the open-source snapshot can be self-hosted.
3. Isolation: "Each agent runs in its own sandbox container with a separate copy of the repository". The provider is configurable and is not named in the README.
4. Recording: transcript; task history in Postgres.
5. Going back: "Agent work is checkpointed and pushed to GitHub": git commits, not machine state.
6. Network limits: not found.
7. Secrets: plain env files and API keys; bring your own Claude or ChatGPT subscription.
8. Spend: not found.
9. Agents: Claude Code, Codex, Amp, Gemini.
10. Price: none; the company is closed.
11. The snapshot is open source, Apache-2.0.
12. **Shut down; the snapshot is dated 16 Jan 2026**, and terragonlabs.com now shows only "Terragon Shutdown".
- **Lesson:** another closure. Background cloud agents with git checkpoints were not enough on their own to keep a company alive.

## Superset, renamed Paradev
1. "Superset is now Paradev… The open-source workspace for Claude Code, Codex, and any agent" (https://paradev.com). superset.sh redirects there.
2. Desktop on macOS and Linux ("Windows is not yet available"); mobile apps; a CLI, a TypeScript SDK and an MCP server; cloud workspaces "run in a sandbox".
3. Isolation: a git worktree and branch per task, locally; a cloud sandbox whose provider is not named.
4. Recording: persistent sessions and diff review only.
5. Going back: not found.
6. Network limits: not found.
7. Secrets: not found. "model calls go directly to your provider".
8. Spend: not found.
9. Agents: any CLI agent.
10. Price (https://paradev.com/pricing):

    | Plan | Price | Cloud |
    |---|---|---|
    | Free | $0 | local only |
    | Pro | $20/mo ($16/mo yearly) | none listed |
    | Team | $60/mo | 150 cloud hours per seat per month, 5 environments |
    | Ultra | $200/mo | 500 hours per seat per month |

    Hours assume a 4 vCPU / 8 GB box. Overage is **$0.06 per GB-hour**, and hours are pooled across the team.
11. Open source, MIT.
12. Renamed to Paradev in 2026; the cloud sandboxes and scheduled automations are also 2026.
- **Top issues:** "use in ssh server" (#1513), "Make login non-required" (#2685), "Linux support" (#405), "Workspace deletion leaves orphaned worktree directories on disk" (#2863) (https://github.com/superset-sh/superset/issues).
- **Gap or idea for Silo:** a GB-hour price ($0.06) is a reference point for Silo's cloud later; pooling hours per team is a pricing pattern to consider.

## Claude Squad (smtg-ai)
1. A terminal UI that "manages multiple AI terminal agents like Claude Code, Codex, OpenCode, and Amp" (https://github.com/smtg-ai/claude-squad).
2. Runs locally; needs tmux and gh.
3. Isolation: a tmux session plus a git worktree per task.
4. Recording: none beyond the tmux pane.
5. Going back: none. "Checkout" (`c`) commits and pauses the session; `r` resumes it.
6. Network limits: not found.
7. Secrets: not found.
8. Spend: not found.
9. Agents: any CLI agent. `--autoyes` is experimental.
10. Price: free.
11. Open source, AGPL-3.0.
12. Signs of abandonment. Top issues include "No more updates?" (#214) and "Project Seems Abandoned, PR's and bugs are not being accepted or fixed." (#250). Also: **"Feature request: worktree environment setup hook (deps, env files, port isolation)" (#260)**, multi-repo sessions (#89, #56) and jujutsu support (#104) (https://github.com/smtg-ai/claude-squad/issues?q=sort:reactions).

## T3 Code (Ping.gg / Theo)
1. An open-source "control plane for coding agents": one server backs desktop, web and phone clients (https://github.com/pingdotgg/t3code). About 76.6k stars.
2. Desktop on macOS, Windows and Linux; iOS and Android apps; a web app; the `t3` CLI. You can run the server on another machine and connect "over a local network, Tailscale, or T3 Connect".
3. Isolation: an optional git worktree per thread.
4. Recording: thread transcripts only.
5. Going back: not found.
6. Network limits: not found.
7. Secrets: not found.
8. Spend: not found. "Usage / quota visibility for Codex sessions and accounts" is a top request (#228).
9. **How tools connect:**
    - It drives agents "you're already signed in to rather than reselling tokens".
    - **The ACP agent protocol is the backbone.** It "prefers the newer ACP v2 preview and falls back to ACP v1", and it runs agents from the ACP Registry.
    - "Claude Agent uses the ACP v2 preview by default for Registry and local `claude-agent-acp` commands".
    - Codex is a built-in provider; for remote sign-in it uses `codex login --device-auth`.
    - "Registry agents always run on the machine hosting your T3 Code server." (https://github.com/pingdotgg/t3code/blob/main/docs/user/providers-acp.md)
10. Price: free.
11. Open source, MIT (the T3 Code brand is excluded).
12. Grew fast in 2026.
- **Top issues:** Copilot CLI support (#193), "significantly slower than Codex for the same coding task" (#695), WSL support (#192), Pi provider (#402), a context-window indicator hidden by default (#9352).
- **Idea for Silo:** a Silo VM could be a "machine hosting your T3 Code server". T3 Code, Paseo, Orca and Happy all split a daemon from its clients, so supporting "run their daemon inside a Silo VM" makes Silo the isolation layer beneath these UIs instead of a competitor.

## Warp (Warp Agent, cloud agents, Warp Factories; formerly "Oz")
1. A terminal with its own agent, plus a cloud platform for background agents ("Warp Factories") (https://docs.warp.dev).
2. Local agents run in the Warp app; cloud agents run "on Warp-hosted compute or your own infrastructure" (https://docs.warp.dev/platform/).
3. Isolation (https://docs.warp.dev/factories/infrastructure-and-security/):
    - self-hosted, default: "a separate Docker container on the self-hosted runner's host";
    - or a Kubernetes Job;
    - or "Direct", which "shares the host OS and kernel".
    - The isolation technology of the Warp-hosted sandbox is not named.
4. Recording: run records show "status, conversation, and outputs"; session sharing with teammates; a handoff between local and cloud.
5. Going back: not found.
6. Network: **outbound access restricted by `networkPolicy`** at factory and agent scope, "deny precedence", built-in policies and "blocked-host diagnostics". Enforced by the platform.
7. **Secrets:** "Secrets are delivered from an explicit per-agent allowlist"; "Inference credentials are never injected into the sandbox"; "Warp redacts known secret values at output boundaries", which they call a backstop. This is the closest of all these products to Silo's secret model.
8. Spend: a team-wide spend cap on Build, Max and Business; "Team-wide and per-user spend limits" on Enterprise (https://www.warp.dev/pricing).
9. Agents: harnesses are Warp Agent (`oz`), **Claude Code, Codex and Pi**. Third-party harnesses are only on the Scale and Enterprise plans (https://docs.warp.dev/factories/harnesses/).
10. Price (https://www.warp.dev/pricing): the page showed two price sets; the second was Build $18, Max $180, Business $45.

    | Plan | Price | Notes |
    |---|---|---|
    | Free | $0 | bring your own inference, 30 cloud conversations |
    | Build | $20/mo | includes $20 of usage at API rates |
    | Max | $200/mo | "10× Build" |
    | Business | from $50/user/mo | |
    | Factories Start | $0 + usage | $3.50 per agent hour |
    | Factories Scale | $250/mo per team + usage | $150 usage included, $3.00 per agent hour |

    Compute is "passthrough"; default cloud machines run from 2 vCPU/4 GB up to 8 vCPU/16 GB.
11. The client is closed source.
12. Moved from Oz to "Warp Factories" in 2026.
- **Overlap with Silo's plans:** an egress policy decided outside the agent; secrets kept out of the box; spend caps.
- **What Silo adds:** a local VM on the Mac; recording of machine activity; checkpoints.
- **Gap or idea for Silo:** **"blocked-host diagnostics"**, which tells the user or agent why a request failed under the network policy. Also: **redacting known secret values in output and logs.** Silo's recordings will hold traffic, so it should scrub its own logs for secret values. Also: a per-agent-hour platform fee as a pricing reference ($3–3.50/h).

## Zed agent panel (brief)
- Runs "multiple agent threads at once"; "If two threads might edit the same files, you can isolate one in a new Git worktree"; runs external agents over ACP; "surfaces how many tokens you are consuming" for each thread. Checkpoints and sandboxing are not on that page (https://zed.dev/docs/ai/agent-panel). Open source (GPL/AGPL).

## Omnara (pivoted)
1. It was a mobile client for Claude Code. It is now "an open-source API for running agents in production", "the open-source, self-hostable alternative to Claude Managed Agents and OpenAI's Agents API" (https://www.omnara.com, https://github.com/omnara-ai/omnara).
2. Runs on Omnara Cloud, self-hosted (Docker Compose), in the customer's own cloud, or on-prem. A daemon on a Mac or Linux machine, VM or container connects outbound.
3. Isolation: "managed sandboxes from 10+ providers including Blaxel, Daytona, Unikraft Cloud", or your own machine.
4. Recording: "Every step is saved, so agents pick up where they left off"; an event log in Postgres; "traces, and event history".
5. Going back: durable resume from the event log (the agent's state, not the disk).
6. Network: "tool policies enforced by the platform"; risky actions can pause for approval.
7. Secrets: "secrets management" (no details).
8. Spend: **"usage metering, spend limits"**.
9. Agents: its own runtime, with any model.
10. Price: "The platform itself is free" with your own keys and machines; otherwise credits at provider rates plus a 5% fee.
11. Open source, Apache-2.0.
12. The pivot itself happened in 2026.
- **Lesson:** the mobile-companion niche did not hold. The value moved to durable, metered agent runtimes.

## Happy (slopus)
1. Began as a CLI that "wraps the Claude Code and Codex harnesses… and lets you continue from your phone", with an end-to-end encrypted relay (`happy-server`). Now positioned as "One place for you, your team, and your agents" (https://happy.engineering, https://github.com/slopus/happy).
2. Desktop on macOS (Apple Silicon and Intel); iOS and Android; agents run on a laptop or team machine; `Dockerfile.server` for the relay.
3. **Isolation: "Every agent works inside an OS sandbox"** (happy.engineering). A first-party guide on Mintlify, now returning HTTP 410 so seen only through a search summary, said it uses `@anthropic-ai/sandbox-runtime` (Seatbelt/bubblewrap), configured by `happy sandbox configure`, with the workspace scoped to ~/Developer and ~/.ssh and ~/.aws read-denied by default, plus `--no-sandbox`.
4. Recording: synced transcripts only.
5. Going back: not found.
6. Network: the sandbox runtime offers a domain allowlist (according to the same secondhand summary).
7. Secrets: the sandbox blocks reading credential paths; credentials are not proxied.
8. Spend: none; "adds a harness, not another bill".
9. Agents: Claude, Codex, Grok, Gemini; OpenCode and any ACP agent.
10. Price: free.
11. Open source, MIT.
12. 2026: moved to a desktop app plus "Happy Harness", with its own push server.
- **Gap or idea for Silo:** "your phone as a client". Five of these products ship mobile apps (Happy, T3, Paseo, Orca, Paradev) and Nimbalyst has an iOS companion. **Watching and steering agents from a phone, and approving a network or budget exception from there, is table stakes in this category.** Silo has no plan for it.

## Emdash (General Action, YC W26)
1. Classic: an open-source "Agentic Development Environment". It is now "Coding agents for your whole team", on your own infrastructure or Emdash cloud, with tasks started from Slack, Linear, GitHub or a phone (https://emdash.com).
2. Classic runs on macOS, Windows and Linux (x64 and ARM64); it also works on remote machines "over SSH/SFTP" (https://github.com/generalaction/emdash).
3. Isolation: "Keep every agent isolated in its own Git worktree and branch"; the cloud uses "an isolated workspace" (not specified).
4. Recording: transcript only.
5. Going back: not found. "Diff mode to view changes since last turn" is an open request (#1635).
6. Network limits: not found.
7. Secrets: not found.
8. **Spend:** "track usage and set spend limits across providers" (admins, 2.0).
9. Agents: auto-detects 9 CLIs (Claude Code, Codex, Cursor, OpenCode, Amp, Devin, Qwen, Droid, Copilot), and others through ACP.
10. Price: 2.0 is early access by sign-up; no prices published.
11. Classic is open source, Apache-2.0.
12. In 2026 it pivoted from the desktop app to Emdash 2.0, a team cloud.

## 2026 newcomers with traction
- **Orca (stablyai)**, https://raw.githubusercontent.com/stablyai/orca/main/README.md
    - MIT; macOS, Windows and Linux; iOS and Android companion apps; "if it runs in a terminal, it runs in Orca".
    - Parallel worktrees, plus "Cloud VMs… for individual worktrees".
    - "Usage & Accounts": "see how much of your agent limits you've used and when they reset", and switch accounts.
    - Third-party lists most often recommend it as the Conductor replacement.
- **Agentastic.dev**, https://www.agentastic.dev
    - A native Swift Mac app, free; 56 built-in agent definitions.
    - **"isolates each one in its own worktree, container, or cloud VM", with support for "Apple containers and Docker"**.
    - **It is the nearest local-Mac competitor to Silo**: it already uses Apple's Virtualization-based `container` runtime, which runs Linux only.
- **Paseo**, https://paseo.sh
    - Apache-2.0; a daemon with desktop, web and native mobile clients; an end-to-end encrypted relay; worktrees.
    - Supports Claude, Codex, OpenCode, Pi and others through ACP.
- **Traycer** appears in comparison lists (not read).

## Cross-cutting observations
- **Nobody in this category isolates the machine on the Mac.**
    - Every app here (Conductor, Nimbalyst, Vibe Kanban, Paradev, Claude Squad, T3, Emdash, Orca, Paseo, Zed) isolates with git worktrees.
    - Conductor says outright that it is "not a security boundary".
    - Real isolation exists only in someone's cloud (Conductor Cloud on Vercel, Paradev cloud, Warp, Terragon), or as Docker, Apple containers or an OS sandbox (Sculptor experimental, Agentastic, Happy).
    - No product offers a macOS guest.
- **The "checkpoints" here are chat-turn or git-commit rewinds** (Conductor's Codex checkpoints, Terragon's pushed commits, Sculptor's planned fork from history). None restores processes, installed packages or databases, and none forks a running machine. This is Silo's clearest gap to fill.
- **Recording means the transcript plus a diff everywhere.** Nobody records from outside: no process tree, no network log, no model calls read from the traffic. Warp comes closest on the team side, with run records and session sharing.
- **Spend is just arriving, and only as estimates.**
    - Conductor added estimated cost per chat, model and workspace on 18–20 Nov 2026.
    - Orca shows subscription limit usage and reset times.
    - T3 Code users ask for quota visibility (#228).
    - Enforced caps exist only on team or cloud platforms (Warp team and per-user caps, Emdash 2.0, Omnara).
    - **Users mostly run on subscriptions (Claude Max, ChatGPT), not API keys.** A Silo budget measured only in dollars of API usage would miss this. Silo should also show "how much of my 5-hour or weekly subscription limit this agent burned", read from the traffic.
- **What users complain about** (GitHub issues, threads):
    - worktree environment setup: dependencies, `.env`, port collisions (Claude Squad #260);
    - orphaned or stale worktrees eating disk (Vibe Kanban #765, Paradev #2863; a Cursor forum user reported 9.82 GB for two worktrees of a 2 GB repo **[3rd-party roundup]** https://dev.to/rohansx/every-ai-agent-tool-creates-git-worktrees-none-of-them-make-worktrees-actually-work-3ae9);
    - a shared dev database that undermines isolation;
    - multi-repo projects (Claude Squad #89, Vibe Kanban #705, Paradev #2364);
    - SSH or remote machines (Paradev #1513, Emdash #901);
    - forced login (Paradev #2685, Vibe Kanban #2952);
    - pinned agent versions (Conductor).

  **Silo's whole-machine model solves the env, ports, database and disk-clutter pains by construction. Lead with that.**
- **The limit people name is attention and review, not compute.** For example: "the limiting factor is my attention" (koliber, https://news.ycombinator.com/item?id=46682551). Silo's recording should answer "what did this agent actually do, and what should I look at?" quickly, as a summary or triage, not just a raw log.
- **Instability in this market:**
    - Terragon shut down (Jan 2026).
    - Bloop / Vibe Kanban shut down (Apr 2026: "couldn't find a business model").
    - Crystal was deprecated.
    - Omnara pivoted.
    - Superset was renamed.
    - Conductor was bought by Meta (4 Nov), then un-shut (13 Nov).

  A free orchestration UI over worktrees has become a commodity (about 10 MIT/Apache clones). The defensible layer sits below it: isolation, recording, rollback, spend.
- **The architecture pattern is converging:** a local daemon or server, several clients (desktop, web, phone), an end-to-end encrypted relay, and agents driven over ACP (T3 Code, Paseo, Happy, Zed, Emdash, Nimbalyst). Silo could treat these UIs as clients that run their daemon inside a Silo VM, rather than competing with each one.
- **Price reference points** (24 Nov 2026):
    - Paradev cloud: $0.06 per GB-hour overage; 150 or 500 hours per seat on $60 or $200 plans.
    - Warp Factories: platform fee of $3.00–3.50 per agent hour on top of compute.
    - Conductor Cloud: Vercel sandboxes at 8 vCPU / 16 GB, free for now.
    - Omnara: provider rates plus 5%.
- **Ideas Silo has not planned:**
    1. a phone client to watch agents and approve network or budget exceptions;
    2. diagnostics that say why a host was blocked;
    3. redacting secret values in Silo's own recordings;
    4. two-way "pairing" sync between the VM and the user's local checkout or IDE (Sculptor);
    5. fanning one prompt out to N boxes and comparing the results (Orca, Conductor), which Silo's fork already makes possible from any checkpoint;
    6. a view of subscription-limit usage beside dollar spend.

Sources not reachable: `docs.imbue.com` (not on the session's allowed network domains), the Happy Mintlify sandbox guide (HTTP 410), and the GitHub API through `gh` (repository access not enabled).

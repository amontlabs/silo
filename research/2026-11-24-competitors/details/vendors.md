# Vendor-built agent sandboxes: research report (all pages read 24 Nov 2026)

Method: I read each vendor's own docs, pricing pages and changelogs with WebFetch. `gh api` was blocked for the repos outside this session, so I took licenses from the GitHub web pages. Where only a third-party source exists, the claim is marked **[3rd-party]**. "Not found in docs" means I looked and found nothing.

Several docs domains have moved since training data: OpenAI Codex docs are now at `learn.chatgpt.com/docs/...`, Factory moved to `factory.com` / `docs.factory.com`, and Anthropic platform docs are at `platform.claude.com`.

---

## Claude Code local sandbox (`/sandbox`, sandbox-runtime, `/rewind`)

1. **What it is:** an OS-level sandbox around the Bash, PowerShell and Monitor commands Claude Code runs. It is for developers on their own machines who want fewer permission prompts.
2. **Where it runs:** on the user's own computer, macOS, Linux or WSL2. "Native Windows: Claude Code runs commands unsandboxed." There is no macOS guest; it is a process sandbox on the host. https://code.claude.com/docs/en/sandboxing
3. **Isolation:**
   - macOS uses Seatbelt. Linux and WSL2 use bubblewrap, and the command "runs in a separate network namespace that has no connection to your network."
   - Scope is shell commands only. "Read, Edit, Write, WebFetch… follow permission rules instead", and "command hooks, local MCP servers, plugin monitors, LSP servers… run with your full access."
   - To put the whole process inside the boundary, run Claude Code under `srt` (sandbox-runtime), which is Apache-2.0, a "Beta Research Preview", and has Windows support in alpha (a dedicated local user plus a Windows Filtering Platform egress fence). https://github.com/anthropic-experimental/sandbox-runtime
   - Anthropic's own comparison page says a VM "provides the strongest separation" and points to Docker Sandboxes microVMs. https://code.claude.com/docs/en/sandbox-environments
4. **Recording:**
   - Local transcripts are kept as `~/.claude/projects/*/*.jsonl`.
   - OpenTelemetry export is opt-in (`CLAUDE_CODE_ENABLE_TELEMETRY=1`). Events include `api_request` (model, `cost_usd`, tokens), `tool_result` and `tool_decision`. Bash command text is logged only with `OTEL_LOG_TOOL_DETAILS=1`.
   - All of this is emitted **by the agent process itself**, from inside. https://code.claude.com/docs/en/monitoring-usage
   - srt taps the macOS sandbox violation log into a `SandboxViolationStore`.
5. **Going back:** `/rewind` restores code, conversation, or both.
   - It tracks **only edits made by Claude's file tools**: "A file that only Bash commands changed isn't tracked."
   - Subagent edits are usually not restored, symlinked and hard-linked files are skipped, and external changes are not tracked.
   - It keeps the 100 most recent checkpoints, deletes them after about 30 days, and says it is "Not a replacement for version control."
   - There is no machine or memory state. Conversation branching uses `/branch` or `--fork-session`. https://code.claude.com/docs/en/checkpointing
6. **Network limits:**
   - Allowed domains "start empty", so the default is closed.
   - The proxy runs "on your machine, outside the sandbox", using HTTP_PROXY/HTTPS_PROXY/ALL_PROXY. UDP, QUIC and ICMP can't leave. Hostnames that resolve to local addresses are refused.
   - TLS is not inspected by default. The docs warn about **domain fronting**: "code running inside the sandbox can potentially use domain fronting… to reach hosts outside the allowlist."
   - Unknown hosts prompt or are allowed depending on permission mode (`bypassPermissions` allows them without a prompt).
   - Escape hatch: the model can retry with `dangerouslyDisableSandbox`. Turn that off with `allowUnsandboxedCommands: false`.
7. **Secrets:** reads are allowed "including credential files such as `~/.ssh`" unless you deny them. New since 2.1.22x is **"mask" credentials**: commands see "a per-session placeholder called the sentinel, and the sandbox proxy substitutes the real value on outbound requests to hosts you allow." It requires the experimental `network.tlsTerminate` (a MITM CA) and is honored only from user or managed settings. It can also re-sign AWS SigV4 requests (`awsPairs`, `sigv4`). This is the closest existing analogue to Silo's placeholder design.
8. **Spend control:** `--max-budget-usd` works in print mode only, and the SDK has `maxBudgetUsd`. It is enforced **by the agent itself** from its own cost estimate (https://code.claude.com/docs/en/costs.md). **[3rd-party]** A GitHub issue reports it stopping a Max-subscription run that showed $0 cost (https://claudeissues.com/issue/85400-...).
9. **Agents supported:** Claude Code. srt can wrap any process.
10. **Price:** free, part of Claude Code.
11. **Open source:** sandbox-runtime is Apache-2.0 (about 7.3k stars). Claude Code itself is not open source.
12. **2026 changes:** credential masking with TLS termination, `failIfUnavailable`, srt on Windows (alpha), and a fix to pin resolved addresses ("Before v2.1.284, the proxy connected to whatever address an allowed hostname resolved to").
- **What it does that Silo plans to do:** network allowlist from a proxy outside the sandbox, and placeholder secrets swapped at the proxy (mask mode).
- **What Silo would do that it does not:** a whole-VM boundary (hooks, MCP and file tools inside too); recording from outside the agent; whole-machine checkpoints that include Bash changes; a spend cap the agent cannot alter.
- **Gaps or ideas for Silo:**
  - SigV4 re-signing for AWS. Plain placeholder substitution breaks signing clients; Anthropic says so on the vaults page.
  - Per-host `injectHosts` plus an `injection_location` of header or body.
  - A `claude doctor`-style check that the boundary is actually in force.
  - Block hostnames that resolve to private or metadata addresses.

## Claude Code on the web / cloud sessions

1. **What it is:** Claude Code sessions on Anthropic-managed VMs, started from the browser, mobile, Desktop or `claude --cloud`. For subscribers on Pro, Max, Ultra, Team and Enterprise premium seats. https://code.claude.com/docs/en/claude-code-on-the-web
2. **Where it runs:** Anthropic's cloud, or the organization's "self-hosted environment" runners. Each session gets "a fresh virtual machine (VM) running Ubuntu 24.04 on x86_64" with 4 vCPU, 16 GB RAM and 30 GB disk. No macOS guest. https://code.claude.com/docs/en/cloud-environments
3. **Isolation:** a VM. The technology is not named in the docs.
4. **Recording:** the conversation history is shown in claude.ai. Telemetry export is sent by Claude Code itself and "doesn't go through the agent proxy." There is no user-facing network or process log. "You don't get a shell into the session VM."
5. **Going back:**
   - "Rewind to here" exists in cloud sessions (the conversation and Claude's file edits, as locally).
   - An idle VM "pauses with its files saved" and is restored on the next message. If reclaimed, only the conversation comes back, and running processes are lost.
   - Environment cache: "Anthropic snapshots the filesystem" after the setup script runs, if setup finishes in about 5 minutes. The cache expires after about 7 days. "The cache stores files only, not running processes."
6. **Network:**
   - Levels are None, Limited (the default, a package-registry allowlist) and Full. Changes apply to running sessions "within about a minute."
   - Bypass paths at every level: GitHub (its own proxy), MCP connectors (through Anthropic's servers), network-secret hosts, and "The Anthropic API… even at **None**." The docs warn this "may allow data to exit the VM."
   - There is no org-level allowlist push: "there's no organization-level allowlist that admins can push."
7. **Secrets:**
   - GitHub credentials "never enter a session's VM". The proxy swaps a scoped credential for the real token, rejects branch deletions and tags, blocks GraphQL, and scopes the API to attached repositories.
   - **Network secrets**: "Anthropic's agent proxy adds the key to requests for the hosts you list, after each request leaves the session's VM." They are never attached to setup-script requests.
   - Environment variables are readable by anyone using the environment.
8. **Spend:** "counts toward your plan's usage limits… no separate charge for the cloud VM." No per-session budget found in docs.
9. **Agents:** Claude Code only.
10. **Price:** included in the plan.
11. **Open source:** no.
12. **2026 changes:** network secrets, self-hosted environments, Projects (many sessions coordinated), Claude Tag in Slack, routines (scheduled runs), and renamed levels (Trusted/Custom became Limited/Full).
- **Same as Silo plans:** network policy outside the VM; secrets attached at the boundary.
- **Silo adds:** local Mac VMs; macOS guests; outside recording of process, file and network activity; memory-inclusive checkpoints and fork; any agent; spend caps.
- **Gap or idea for Silo:**
  - Scoped git operations at the proxy (refuse tag pushes and branch deletions, restrict to named repositories). This is a concrete, valuable boundary policy.
  - Note that their "None" still leaves the model API open. Silo should say plainly what "closed" means for the model API.

## Anthropic Claude Managed Agents (hosted agent API)

1. **What it is:** an API (`/v1/agents`, `/v1/sessions`, `/v1/environments`, `/v1/vaults`) for running Claude agents with hosted sandboxes. In beta (`managed-agents-2026-04-01`). For developers building agent products. https://platform.claude.com/docs/en/managed-agents/environments
2. **Where it runs:**
   - Cloud: "isolated Linux containers on Anthropic-managed infrastructure", Ubuntu 24.04 x86_64, "Up to 8 GB" memory and "Up to 10 GB" disk. https://platform.claude.com/docs/en/managed-agents/cloud-containers
   - Self-hosted sandboxes have been in public beta since 19 May 2026. Providers named: Cloudflare (microVMs), Daytona, Modal, Vercel. https://claude.com/resources/articles/claude-managed-agents-updates
   - No macOS guest.
3. **Isolation:** containers. The harness ("brain") runs outside the sandbox ("hands"): "The container became cattle." https://www.anthropic.com/engineering/managed-agents (8 Apr 2026)
4. **Recording:** an append-only session event log stored **outside** the sandbox and harness (GET `/v1/sessions/{id}/events`). The Console shows tool calls and token usage. "Transcripts persist until you delete them." Not ZDR-eligible.
5. **Going back:** no checkpoint, snapshot or fork for sandboxes found in docs. The sandbox is disposable, and the session log allows harness restart.
6. **Network:**
   - Modes are `limited` (with `allowed_hosts`, `allow_package_managers`, `allow_mcp_servers`) and `unrestricted`.
   - **The API default is `unrestricted`**: "a create request that omits it gets `unrestricted`". The Console defaults to Limited.
   - Blocked requests get a 403 that names the host.
   - The docs warn: "Network access is granted by host, not by operation… a successful prompt injection could use an allowed host to copy files out."
7. **Secrets:** vaults. `environment_variable` credentials are "stored in the sandbox as an opaque placeholder… substituted with the real secret at egress. The agent never sees the secret value."
   - Each credential has its own `allowed_hosts` and `injection_location` (header and/or body).
   - Limits stated in the docs: signing clients such as SigV4 break; "Substitution is outbound only" (a token returned by an exchange arrives in the sandbox unredacted).
   - MCP OAuth tokens are refreshed by Anthropic. Archiving a vault purges the secrets and keeps the records for audit. https://platform.claude.com/docs/en/managed-agents/vaults
8. **Spend:** token usage is shown in the Console. No hard budget found in docs.
9. **Agents:** Claude only, through Anthropic's harness.
10. **Price:** tokens at standard rates plus **$0.08 per session-hour** while the status is `running`, "measured to the millisecond". Idle time is free. Web search is $10 per 1,000. https://platform.claude.com/docs/en/about-claude/pricing
11. **Open source:** no.
12. **2026 changes:** launched April 2026; self-hosted sandboxes and MCP tunnels in May 2026; multi-agent threads.
- **Same as Silo plans:** placeholder-at-egress secrets with per-host scoping; the record kept outside the sandbox.
- **Silo adds:** a full VM; local Mac; any unmodified agent; recording of what happens inside the machine (processes, files); checkpoints.
- **Gap or idea for Silo:**
  - Copy their secret model exactly: per-secret host list, header-vs-body scope, and a clear "placeholder passed through literally" failure mode.
  - Add webhooks or notifications on credential events.

## OpenAI Codex CLI (local sandbox)

1. **What it is:** OpenAI's coding agent CLI, also used by the IDE extension and the ChatGPT desktop app's Codex mode.
2. **Where it runs:** the user's machine, on macOS, Linux, WSL2 and Windows. No macOS guest. OpenAI deliberately chose **not** to use VMs, so that "host-dependent workflows, such as iOS builds on a Mac" keep working. https://openai.com/index/inside-the-codex-sandbox/ (18 Nov 2026)
3. **Isolation:** macOS uses Seatbelt (`sandbox-exec`). Linux uses "`bwrap` plus `seccomp` by default", and since 0.115 Landlock is legacy. Windows uses Microsoft **MXC**. Modes are `read-only`, `workspace-write` (the default) and `danger-full-access`. https://learn.chatgpt.com/docs/sandboxing
4. **Recording:** session "rollout" transcripts are kept in `~/.codex/sessions` (`--ephemeral` disables them). OTel is "off by default" and covers conversations, API requests, tool decisions and tool results. All emitted by the agent itself.
5. **Going back:** `/fork` and `codex fork` branch the conversation. A `task.undo` metric exists. The docs say: "Create Git checkpoints before and after a task so you can revert changes." No machine snapshots.
6. **Network:**
   - Off by default in workspace-write.
   - The managed `network_proxy` (HTTP plus SOCKS5) is **off by default even when network is on**: "Network on + network_proxy off: … unrestricted direct outbound access."
   - Domain rules include `deny` (which wins over allow), `**.` wildcards and a private-IP block.
   - "Lookups that fail or time out are blocked", but rebinding is not fully prevented ("would require pinning resolved IPs").
   - The proxy does not cover web search, MCP, apps, browser use or model traffic.
   - Admins can enforce rules through `requirements.toml` / MDM.
7. **Secrets:** no credential proxy locally. Deny-read rules for `**/*.env`. The blog admits API keys in readable config files remain a risk.
8. **Spend:** experimental `features.rollout_budget` (a token limit plus reminders, "off by default") and `goals.max_goal_token_budget`. Both are enforced by the agent. ChatGPT Enterprise has per-user spend limits, but "Alerts notify recipients but do not stop usage. Running work can settle after a limit is reached."
9. **Agents:** Codex only.
10. **Price:** included in ChatGPT plans, then credits. Example rate: GPT-6.2 Sol at 50 / 250 credits per million input / output tokens. https://learn.chatgpt.com (pricing section of llms-full.txt)
11. **Open source:** Apache-2.0, about 136k stars. https://github.com/openai/codex
12. **2026 changes:**
    - The Codex desktop app merged into the ChatGPT desktop app (Chat / Work / Codex modes) around 9 Jul 2026 **[3rd-party for the date]**.
    - `approval_policy = "untrusted"` removed; "Auto-review" model-based approvals added.
    - MXC on Windows; bwrap default.
    - New safety monitoring can "pause" tasks.
- **Same as Silo plans:** a host-level allowlist proxy.
- **Silo adds:** a VM with no prompts inside; recording outside the agent; real restore.
- **Gap or idea for Silo:** OpenAI's argument against VMs is that Mac-native workflows (Xcode, iOS) break. Silo's macOS guest answers exactly that, and should be shown doing so.

## OpenAI Codex Cloud (current, plus "Codex Cloud (Legacy)")

1. **What it is:** Codex tasks in OpenAI-hosted machines, started from web, mobile, desktop, Slack or Teams.
2. **Where it runs:** "Each task runs in a VM." Plus gets 2 vCPU / 8 GiB / 8 GiB disk; Pro and Business get 4 vCPU / 16 GiB / 32 GiB. Custom VMs for Enterprise. No macOS guest. https://learn.chatgpt.com/docs/environments/cloud-environments
3. **Isolation:** a VM. Technology not named. The legacy version used containers (the `universal` image).
4. **Recording:** task transcripts are kept. Enterprise has a Compliance API and audit events. No network or process log found.
5. **Going back:**
   - "Publishing captures the prepared filesystem for new tasks", so new tasks start from a snapshot.
   - "Saved VM state is recoverable for up to seven days after the last turn."
   - "Saved state doesn't replace source control."
   - Legacy cached containers for up to 12 hours.
6. **Network:**
   - Current: a toggle "Enable all public internet access", or a package-manager preset plus "Additional allowed domains". Tailscale for private networks.
   - Legacy: "Agent internet access is off by default"; the allowlist could also restrict HTTP methods (GET/HEAD/OPTIONS) **[3rd-party screenshot, 2025]**.
   - "All outbound internet traffic passes through this proxy."
7. **Secrets:**
   - Current: **network secrets**, where "Programs receive a placeholder; the proxy substitutes the real value for allowed destinations" (HTTPS on 443 only). A personal vault, plus OIDC short-lived cloud credentials for AWS, GCP and Azure.
   - Legacy: "secrets are removed before the agent phase starts."
8. **Spend:** shared ChatGPT credits and usage limits; admin per-user limits.
9. **Agents:** Codex only.
10. **Price:** included in ChatGPT plans or credits. Compute is not priced separately in the docs.
11. **Open source:** no.
12. **2026 changes:** relaunch (VMs, reusable environments) **[3rd-party says DevDay, Sep 2026]**; the old product is labelled "Codex Cloud (Legacy)" in OpenAI's docs.
- **Same as Silo plans:** placeholder secrets swapped at the proxy; filesystem snapshot to start from; a host allowlist.
- **Silo adds:** local execution; memory checkpoints; fork; any agent; spend caps at the boundary.
- **Gap or idea for Silo:** OIDC workload identity (short-lived cloud credentials minted per session) instead of stored keys. Both Codex and Amp now offer it.

## Cursor Cloud Agents (formerly Background Agents)

1. **What it is:** Cursor's agents running in cloud VMs, which produce PRs "with videos, screenshots, and logs."
2. **Where it runs:** "Firecracker-based microVM infrastructure" in "a separate AWS account" (https://cursor.com/docs/cloud-agent/security). Self-hosted machines since 2 Sep 2026: your laptop or VM, or providers such as Modal, Daytona, E2B, Vercel, Cloudflare and Namespace. "Self-hosted workers now support computer use on Linux and Mac" (https://cursor.com/changelog/self-hosted-machines). There is no Cursor-hosted macOS VM.
3. **Isolation:** Firecracker microVMs.
4. **Recording:**
   - The transcript ("Prompts, model responses, tool calls, diff context, and demo artifacts") is "Kept indefinitely by default."
   - Video recordings of the agent's desktop are reviewed by a "video review model".
   - The Cursor Cloud MCP exposes transcripts, run events and setup logs. Remote desktop takeover is available.
   - Recording is at the app level, not from the network boundary.
5. **Going back:**
   - VM disk snapshots ("Point-in-time copies of the VM disk") are kept for 30 days of inactivity, are encrypted, and "can't be deleted on demand."
   - Environments are built from a snapshot or a Dockerfile in `.cursor/environment.json`.
   - No user-facing restore-to-point or fork found in docs.
6. **Network:**
   - Agents have internet by default. Modes are "Allow all", "Default + allowlist" and "Allowlist only". Cursor's own services and git hosts stay reachable.
   - Enforcement location is not stated. The enterprise page calls the allowlist best-effort, "not a security boundary", and suggests adding hooks (https://cursor.com/docs/enterprise/llm-safety-and-controls, via search snippet).
   - Forum, 21 Sep 2026: `egressAllowlist` in the repo file was silently ignored because the dashboard setting took precedence; staff answered "expected behavior". https://forum.cursor.com/t/egressallowlist-in-environment-json-is-not-enforced/172483
7. **Secrets:** "Runtime Secrets" are **environment variables inside the VM**, redacted from transcripts and commits as `[REDACTED]`, but "still visible to users interacting with the agent's environment via the Terminal." Build secrets are kept to the Docker build. https://cursor.com/docs/cloud-agent/security-network
8. **Spend:** "asked to set a spend limit on first use." Usage is at the model's rate.
9. **Agents:** Cursor's agent only, "a curated selection of models."
10. **Price:** Cloud agents are included from Premium ($30/mo); Super $100; Ultra $200; Teams $30+/user. Third-party models are billed separately. https://cursor.com/pricing
11. **Open source:** no.
12. **2026 changes:** computer use plus video (24 Feb), self-hosted cloud agents (25 Mar), self-hosted machines with Mac computer use (2 Sep), new plan names.
- **Same as Silo plans:** each agent in its own VM; disk snapshots; network modes.
- **Silo adds:** secrets that never enter the VM (Cursor's redaction is cosmetic); enforcement outside the VM; local Mac; a macOS guest; any agent; memory checkpoints.
- **Gap or idea for Silo:**
  - Agent-produced **video or screenshot evidence** for review.
  - Live desktop takeover.
  - When repo and UI policy disagree, show which one wins. This was a real user trap.

## GitHub Copilot cloud agent (coding agent) and the new cloud and local sandboxes

1. **What it is:** Copilot working asynchronously from issues and PRs. Since 28 Oct 2026 there is a "connected Copilot cloud experience" across github.com, the CLI, VS Code, Slack and Teams. https://github.blog/changelog/2026-10-28-work-from-anywhere-with-a-new-connected-copilot-cloud-experience/
2. **Where it runs:**
   - A **cloud sandbox**: "fully isolated, ephemeral Linux environment hosted by GitHub", now the default.
   - Or a GitHub Actions runner (Ubuntu x64 only).
   - Or your own machine or VM through `--environment-host`.
   - A **local sandbox** on macOS uses Seatbelt (macOS 15+ recommended), Linux uses bubblewrap, Windows uses MXC. "it does not run your commands inside a separate virtual machine or container." https://docs.github.com/en/copilot/concepts/security-governance-and-network-settings/about-cloud-and-local-sandboxes
   - No macOS guest.
3. **Isolation:** cloud sandbox technology not named; Actions VMs; local process sandbox.
4. **Recording:** session logs "show the work and tools used." Blocked requests are noted in the PR body with "the blocked address and the command." Chat data is retained for the life of the account (cloud sandbox) or 28 days (Actions).
5. **Going back:** "While a session is stopped, the cloud sandbox keeps a snapshot of its state", resuming "with your files and in-progress work intact." No point-in-time restore or fork found.
6. **Network:**
   - Firewall on by default, with a recommended allowlist; org and repo custom rules (domains and URLs).
   - Coverage gaps: it applies to the agent's Bash tool and the Playwright MCP server only, not "processes started in configured Copilot setup steps" or other MCP servers.
   - It "should not be considered a comprehensive security solution." https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/customize-the-agent-firewall
7. **Secrets:** Actions secrets in the `copilot` environment, exposed as env vars inside the VM. Local sandbox: "Sandboxed tools receive placeholders, and a local proxy supplies the real credentials only to approved HTTPS destinations." Cloud sandbox secret handling is not covered in the docs.
8. **Spend:** per-product and per-SKU budgets with "Stop usage when budget limit is reached" (compute). Premium requests or AI credits for model usage.
9. **Agents:** Copilot, plus **Claude and Codex as third-party agents** (Agent HQ, public preview since Feb 2026). https://github.blog/news-insights/product-news/pick-your-agent-use-claude-and-codex-on-agent-hq/
10. **Price:**
    - Cloud sandbox: **$0.000024 per vCPU-second** (about $0.086 per vCPU-hour), $0.000003 per GiB-second for memory, $0.005 per GiB-month for snapshot storage. https://docs.github.com/en/billing/concepts/product-billing/cloud-and-local-sandboxes
    - Billing started **23 Nov 2026**, drawing from the Actions entitlement.
    - Local sandbox is free.
11. **Open source:** no; MXC is open (microsoft/mxc).
12. **2026 changes:** cloud and local sandboxes (Oct–Nov 2026), Agent HQ, the rename to "cloud agent".
- **Same as Silo plans:** local placeholder credentials through a proxy; snapshots on stop; a firewall with an allowlist.
- **Silo adds:** a VM on the Mac; recording outside the agent; restore and fork; spend caps on model calls.
- **Gap or idea for Silo:** per-second compute metering with a hard "stop at budget" is now the market's reference price for cloud (about $0.086 per vCPU-hour). Silo's later cloud pricing will be compared to it.

## Google Jules — shutting down

1. **What it is:** Google's async coding agent working on GitHub repos.
2. **Where it runs:** a "secure, short-lived virtual machine", Ubuntu. https://jules.google/docs/environment/
3. **Isolation:** a VM; technology not stated.
4. **Recording:** sessions and activities are exposed through the REST API. Not found in docs: network or process logs.
5. **Going back:** an environment snapshot after the setup script, reused for later tasks. No restore or fork.
6. **Network:** not found in docs.
7. **Secrets:** env vars set in settings, plus MCP API keys.
8. **Spend:** task quotas per day: 15 / 100 / 300 tasks with 3 / 15 / 60 concurrent. https://jules.google/docs/usage-limits/
9. **Agents:** Jules only.
10. **Price:** bundled with Google AI Pro and Ultra.
11. **Open source:** no.
12. **2026 changes:** **"Jules Turndown", posted 29 Oct 2026.** It winds down on **31 Dec 2026**, is read-only after 11 Nov, and the lessons go into **Google Antigravity**. https://jules.google/docs/changelog
- **Lesson for Silo:** a big vendor's async cloud agent was folded into its own agent platform. Vendor-run sandboxes come and go with the vendor's agent strategy. An agent-independent environment is the durable position.

## Gemini CLI / Antigravity CLI sandbox

1. **What it is:** Gemini CLI, which has been "replaced by Antigravity CLI on June 18th, 2026" for free and Google AI Pro/Ultra users; Gemini CLI is now only "maintained for enterprise Gemini Code Assist workflows" (https://github.com/google-gemini/gemini-cli). Antigravity 2.0 (desktop app, CLI `agy`, SDK) shipped 19 May 2026 **[Wikipedia]**.
2. **Where it runs:** the local machine (macOS, Linux, Windows). No macOS guest.
3. **Isolation:**
   - Gemini CLI: opt-in. macOS Seatbelt profiles (`permissive-open` is the default; `*-proxied` variants route through a proxy), Docker/Podman, **gVisor (`runsc`) "Strongest isolation available"**, LXC, and a native Windows sandbox. https://geminicli.com/docs/cli/sandbox/
   - Antigravity: Linux namespaces, macOS Seatbelt, Windows AppContainer (needs Win 11 24H2 plus the Aug 2026 update). "there are no virtual machines or Docker images." On by default in the App, off by default in the CLI (`enableTerminalSandbox`). https://antigravity.google/docs/sandbox/
4. **Recording:** local chat history. Not found in docs: an outside audit trail.
5. **Going back:** Gemini CLI checkpointing (off by default) is a shadow git repo in `~/.gemini/history/<hash>` plus the conversation and the pending tool call. Project files only. https://geminicli.com/docs/cli/checkpointing/
6. **Network:** Antigravity: "Sandboxed commands run without network access by default"; domains allowed for `read_url` are added to the allowlist; a custom HTTP proxy is possible. Unsandboxed commands "run on your host with full privileges."
7. **Secrets:** `~/.ssh` and `.env` are blocked from reads. No proxy injection.
8. **Spend:** quotas (weekly free; 5-hour windows for Pro/Ultra). Not found in docs: budgets.
9. **Agents:** Gemini / Antigravity only.
10. **Price:** free quota; AI Pro; a new $100 Ultra tier; top Ultra cut to $200 **[Wikipedia]**.
11. **Open source:** Gemini CLI is Apache-2.0. Antigravity is not open source.
12. **2026 changes:** the Antigravity CLI replacement; Jules folded in.
- **Gap or idea for Silo:** Gemini CLI's shadow-git checkpoint that also re-proposes the pending tool call is a nice UX for "restore and retry this step".

## Devin (Cognition)

1. **What it is:** an autonomous cloud software engineer. Cognition acquired Windsurf in 2025, and the plans now include an IDE with Tab completions.
2. **Where it runs:** "a Linux-based virtual machine." Also **macOS VMs**: "Devin now has access to macOS virtual machines" (`runs-on: macos`, `!mac` in Slack, `platform: "macos"` in the API). There is a VPC deployment and "Outposts" on customer infrastructure. https://docs.devin.ai/onboard-devin/environment/macos-support.md
3. **Isolation:** VMs; hypervisor not named. On macOS "no nested hardware virtualization", so containers run 15–25x slower; no USB passthrough.
4. **Recording:** session timeline in the UI. Not found in docs: audit recording. Third parties sell recording gateways for Devin **[3rd-party, hoop.dev]**.
5. **Going back:**
   - Blueprints (YAML) are built into snapshots, rebuilt about every 24h and per platform.
   - Sessions "snapshot to disk when they sleep… running processes do not" survive.
   - No restore or fork found.
6. **Network:**
   - Security profiles: "an allowlist… All other outbound connections are blocked", enforced "at the machine level on Devin-managed VMs."
   - Mandatory profiles are a floor: "Lower-level selections can only tighten, never loosen."
   - Devin can request a domain mid-session; the grant is intersected with the mandatory policy.
   - On Outposts, "Devin does not install firewall rules." https://docs.devin.ai/product-guides/security-profiles
7. **Secrets:** "injected as environment variables" inside the VM, redacted in the UI as `[REDACTED SECRET]`, "scrubbed from the snapshot image." Build-only secrets are never injected into sessions. https://docs.devin.ai/onboard-devin/environment/blueprint-reference
8. **Spend:** usage allowances refresh daily and weekly; Teams has "3 tiers, by team and per user" cost controls; extra usage is billed at API pricing.
9. **Agents:** Devin only.
10. **Price:** Free $0; Pro $20; Max $200; Teams $80; Enterprise custom. ACUs are no longer mentioned on the pricing page. macOS currently has "no macOS surcharge (promotional launch pricing)". https://devin.ai/pricing
11. **Open source:** no.
12. **2026 changes:** classic config replaced by blueprints (30 Jun / 31 Jul 2026); macOS VMs; new plan structure.
- **Same as Silo plans:** a **macOS guest VM** (a direct competitor on this point); allowlist enforced at the machine level.
- **Silo adds:** local on your own Mac; any agent; secrets outside the VM; checkpoints with memory; fork; outside recording; spend caps.
- **Gap or idea for Silo:**
  - "Mandatory profile intersect" semantics for team policy.
  - The agent asking for a domain mid-session, with the grant capped by policy.
  - Devin's stated macOS-VM limits (no nested virtualization, no USB) are the same limits Silo's macOS guest will face on Virtualization.framework, so document them honestly.

## Amp (ampcode, from Sourcegraph)

1. **What it is:** a multi-model coding agent (it names GPT-5.6 and Claude Fable 5.2 "for what each model is best at"). The CLI runs locally; "orbs" are cloud machines per thread. https://ampcode.com/manual
2. **Where it runs:**
   - Linux orbs (Amp-hosted, or self-hosted for free); "runners" on your own machines.
   - **macOS Satellites** (beta, 29 Oct 2026): "our server boots a fresh Mac in the cloud with Xcode and the simulators installed, in about 30 seconds." The Mac is attached to the Linux orb, code is synced over, and the Mac is deleted 20 minutes after use. Whether it is a VM is not stated. **[Release-note mirror]** https://www.havoptic.com/r/amp-macos-satellites
3. **Isolation:** the orb's technology is not named. Not found in docs: a local sandbox.
4. **Recording:** threads are saved and shareable; multiplayer threads. Not found in docs: network or process logs.
5. **Going back:** an orb "goes to sleep… wakes up with your files and services still in place"; setup results are snapshotted. No restore or fork found.
6. **Network:** egress IPs are documented. Not found in docs: allowlist controls.
7. **Secrets:** Amp hides secret values in shell output but "writes them to ~/.env in plain text" inside the orb. OIDC temporary identity is recommended. https://ampcode.com/docs/orbs/handling-secrets
8. **Spend:** "no markup on provider API prices" for individuals; spend limits only on Enterprise.
9. **Agents:** Amp only. It can use a ChatGPT subscription for OpenAI models.
10. **Price:**
    - Hobby is free (pay-as-you-go orbs); Megawatt $20 includes 45,000 orb-minutes; Megawatt 50 is $50; Gigawatt is $200.
    - Orbs: a1.tiny (1 CPU, 2 GB) $0.08/h up to a1.3xlarge $2.13/h.
    - Self-hosted at Enterprise: $0.0504 per CPU-hour plus $0.0162 per GB-hour.
    - Macs: mac.medium (6 CPU, 14 GB) $2/h; mac.large (12 CPU, 28 GB) $4/h.
    - Sources: https://ampcode.com/docs/pricing , https://ampcode.com/docs/orbs/sizes-and-costs
11. **Open source:** no.
12. **2026 changes:** orbs, runners, macOS Satellites, pricing rework.
- **Same as Silo plans:** a macOS machine for the agent; per-thread machines that sleep and resume.
- **Silo adds:** isolation and network policy at all; secrets outside the machine (Amp's are plaintext inside); recording; checkpoints; local Mac VMs.
- **Gap or idea for Silo:** the "Linux box plus Mac satellite only when needed" split is clever on cost. Silo could let a Linux VM borrow a macOS VM only for Xcode steps.

## Factory (Droids)

1. **What it is:** the Droid CLI, desktop app and SDK. The brand moved to factory.com.
2. **Where it runs:** locally; "Droid Computers" (Factory-managed, persistent, auto-pause; research preview); BYOM / Remote Control on your own machines through Factory's relay; air-gapped enterprise deployment. https://docs.factory.com/droid-computers/overview
3. **Isolation:** local sandbox, opt-in (`sandbox.enabled`): macOS Seatbelt, Linux bubblewrap plus seccomp, Windows MXC. Default mode is per-command. https://docs.factory.com/cli/configuration/sandbox. Managed computers: VM technology not named; iptables restricts inbound traffic to the daemon port.
4. **Recording:** audit logging only on the Business plan (pricing page). Not found in docs: outside recording.
5. **Going back:** managed computers start "from its successful snapshot and retain its own subsequent state." No restore or fork.
6. **Network:** local sandbox: "Deny all except Factory's own domains and WorkOS authentication domains", plus `allowedDomains` through an HTTP/SOCKS filtering proxy. Whether the proxy is inside or outside the sandbox is not stated. In headless mode, violations are auto-denied.
7. **Secrets:** env vars injected into workspaces; a Factory-managed SSH key per computer.
8. **Spend:** plan usage multiples. Not found in docs: budgets.
9. **Agents:** Droid only, but bring-your-own inference (ChatGPT, Copilot, OpenRouter, API keys).
10. **Price:** Free (BYO inference); Pro $20; Plus $100; Max $200; Teams $60/team plus $40/user; Business and Enterprise custom. https://factory.com/pricing
11. **Open source:** no.
12. **2026 changes:** Droid Computers; the factory.com rebrand; "Missions" (multi-week autonomous runs) **[3rd-party]**.
- **Gap or idea for Silo:** "persistent computer the agent owns, auto-pause and resume, reachable through a relay with no open ports" is the shape Silo's cloud tier could take.

---

## Cross-cutting observations

1. **Every agent CLI converged on the same local design.** Claude Code, Codex, Copilot, Factory and Antigravity all use Seatbelt on macOS, bubblewrap plus seccomp on Linux, and Microsoft MXC on Windows, with a host-side HTTP/SOCKS allowlist proxy.
   - None of them uses a VM locally.
   - OpenAI states why: to keep Mac-native workflows working. Anthropic's own docs say a VM "provides the strongest separation."
   - Silo's local Mac VM, with a macOS guest, sits in a gap the vendors explicitly chose not to fill.
2. **Their sandboxes leak by design**, and the docs say so.
   - Claude Code runs hooks, MCP servers and file tools outside the sandbox, has a model-triggered `dangerouslyDisableSandbox` retry, and warns about domain fronting.
   - Codex's proxy is off by default even with network on, and does not cover MCP, web or browser traffic.
   - Copilot's firewall skips setup steps and non-Playwright MCP servers.
   - Cursor's allowlist is "best-effort, not a security boundary" (search snippet of its enterprise page).
   - Real user complaints include several Claude Code issues about silent sandbox bypass (e.g. https://claudeissues.com/issue/34315-..., /35224-..., /74607-...) and Cursor's repo allowlist being silently ignored (forum link above).
   - "Enforced outside the agent's reach, covering everything" is a genuine differentiator for Silo.
3. **Placeholder secrets swapped at the egress proxy became the standard in 2026**: Anthropic Managed Agents vaults, Claude Code cloud network secrets, Claude Code local "mask", Codex Cloud network secrets, and Copilot's local sandbox. Cursor, Devin and Amp still put secrets in the VM as env vars and only redact them from output.
   - Silo's design matches the leaders.
   - Silo should copy three refinements: a host list per secret, header-vs-body scope, and SigV4/AWS re-signing.
   - Silo should document the known limits: signing clients, token exchanges returning unredacted, and TLS termination required.
4. **No vendor records from outside the agent at the boundary.**
   - Local tools record what the agent says it did (JSONL transcripts, opt-in OTel emitted by the agent process).
   - Cloud products keep the chat transcript, and Cursor adds video.
   - No one offers a process tree, file-change log or network connection log captured outside the agent, nor model calls read off the wire.
   - Managed Agents keeps an event log outside the sandbox, but it is the harness's view.
5. **Checkpoints are code-only or disk-only everywhere.**
   - Claude Code `/rewind`: file-tool edits only; Bash changes are not tracked. Gemini CLI: a shadow git repo. Codex: "use git."
   - Cloud products snapshot the disk on sleep or after setup (Cursor, Copilot, Devin, Codex Cloud, Amp, Claude cloud), and all say running processes are lost.
   - No one offers memory-inclusive checkpoints, user-chosen restore points, or fork-a-running-machine. Silo's automatic whole-machine checkpoints plus fork are unmatched.
6. **Spend control on model tokens is weak and self-policed.**
   - Claude Code's `--max-budget-usd` is print-mode only and computed by the agent. Codex's `rollout_budget` is experimental and off. ChatGPT spend alerts "do not stop usage."
   - Cursor and Amp have account-level caps, not per-session ones.
   - Compute budgets are better: GitHub has "stop at 100%."
   - A per-session or per-task cap enforced at the network boundary, which works for any agent, does not exist anywhere.
7. **Vendor sandboxes run only the vendor's agent.** The exceptions are GitHub Agent HQ (Claude and Codex inside Copilot) and the Managed Agents and Cursor "self-hosted on Modal, Daytona, E2B…" pattern. Meanwhile Jules is being shut down (31 Dec 2026) and Gemini CLI was replaced by Antigravity CLI. An environment that runs any agent unmodified is safe from this churn.
8. **macOS guests are now emerging in the cloud**: Devin macOS VMs (no surcharge, promotional), Amp macOS Satellites ($2–4/h, beta, 29 Oct 2026), and Cursor self-hosted Mac computer use. All are cloud or bring-your-own-machine. None gives a local macOS VM on the developer's own Mac. Devin's documented limits (no nested virtualization, containers 15–25x slower, no USB) will apply to Silo too.
9. **Prices to anchor against (24 Nov 2026):**

   | Offer | Price |
   |---|---|
   | Managed Agents runtime | $0.08 per session-hour, plus tokens |
   | GitHub cloud sandbox | about $0.086 per vCPU-hour plus $0.011 per GiB-hour |
   | Amp orbs | $0.08/h (1 CPU) to $2.13/h; Mac $2–4/h |
   | Claude cloud and Codex cloud | bundled into subscriptions, no separate VM charge |

   Local execution (Silo now) costs the user no compute; the comparison is on safety and recording.
10. **Small but real policy ideas seen:**
    - Proxy-scoped git: no tag pushes or branch deletes, only attached repositories (Claude cloud).
    - Mandatory policy that lower levels can only tighten, plus agent-requested domains capped by it (Devin).
    - Blocking hostnames that resolve to private or metadata IPs, and pinning resolved IPs (Claude Code 2.1.284, srt, Codex).
    - A visible per-request "blocked host" message to the agent and in the PR (Copilot, Claude).
    - Video evidence of the agent's work (Cursor).


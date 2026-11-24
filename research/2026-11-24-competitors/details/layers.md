# How other products handle Silo's other pillars (read 24 Nov 2026)

Scope: tools that cover recording, spend control, secrets and network egress for coding agents, each on its own, plus the 2026 "flight recorder / time travel" startups. Most of these are not sandboxes, so the isolation and going-back dimensions are often n/a. "3P" marks a third-party source, used where no first-party page could be read. All prices and facts were read on 2026-11-24.

---

# (a) Observability and recording

## Claude Code OpenTelemetry, analytics and `/usage` (Anthropic)
1. Telemetry built into Claude Code: metrics, events and beta traces over OTLP, plus Console and Enterprise analytics. For admins and platform teams.
2. Runs inside the agent process on the user's machine (macOS, Linux, Windows). n/a for a macOS guest.
3. n/a (the separate sandbox uses Seatbelt on macOS and bubblewrap on Linux; see (c)).
4. Recording comes from inside the agent. Metrics: `claude_code.cost.usage`, `token.usage`, `lines_of_code`, `commit.count`, `pull_request.count`. Events: `user_prompt`, `tool_result`, `api_request` (with `cost_usd`), `api_error`, `tool_decision`, and optional raw API bodies. Content is redacted by default: "Spans redact user prompt text, tool input details, and tool content by default". Bash command text only appears with `OTEL_LOG_TOOL_DETAILS=1` (https://code.claude.com/docs/en/monitoring-usage.md). Claude Code "does not pass OTEL_* variables to child processes such as Bash tool runs, hooks, or MCP servers" (from the search summary of the same doc). A repository's `.claude/settings.json` cannot turn telemetry on.
5. n/a.
6. n/a (see the sandbox under (c) and (d)).
7. n/a here.
8. Cost figures are client-side estimates. `--max-budget-usd` "Stop[s] the run once estimated spend … reaches this amount (print mode only) … Spend can pass the cap" (https://code.claude.com/docs/en/cli-reference.md). Team and Enterprise plans have spend limits per org, group or member. Console users get workspace limits. Bedrock, Vertex and Foundry users get "Your cloud's budget controls" (https://code.claude.com/docs/en/costs.md).
9. Claude Code only.
10. Included with Claude Code.
11. The CLI is closed source.
12. 2026: beta traces, raw-body logging, `--max-budget-usd` enforcement on subagents (v2.1.217), and the Enterprise Analytics API.
- **Overlap with Silo:** per-request cost and model-call records.
- **What Silo adds:** the record is made outside the agent, so child processes are included, and it works for any agent. A cap that only exists in print mode and runs on a client-side estimate is weaker than enforcement at the boundary.
- **Idea for Silo:** export the same OTel schema (`api_request`, `tool_result`), so teams can plug Silo into the dashboards they already have.

## Langfuse (acquired by ClickHouse, Jan 2026)
1. Open-source LLM observability. Has a Claude Code integration.
2. Cloud or self-hosted.
3. n/a.
4. A Stop hook reads the Claude Code transcript after each response and builds traces with prompts, tool calls (edits, bash, web) and token counts. The input is a "best partial reconstruction of the prompt context". Text is cut at 20,000 characters. Errors fail silently (fail-open) (https://langfuse.com/integrations/developer-tools/claude-code.md). An AI Gateway option (beta) exists as an alternative to hooks.
5–7. n/a.
8. Token counts only. Dollar cost is not described on the integration page.
9. Claude Code, Codex, Copilot and others through hooks.
10. Hobby free (50k units). Core $29/mo. Pro $199/mo. Enterprise $2,499/mo. Extra units $8 per 100k. Self-hosting is free (https://langfuse.com/pricing).
11. Core is MIT. ClickHouse says it "remains 100% open-source under its existing MIT license for core features" (https://langfuse.com/blog/joining-clickhouse, via search).
12. ClickHouse acquisition on 16 Jan 2026, alongside ClickHouse's $400M raise.
- **Overlap with Silo:** session timelines.
- **What Silo adds:** a record built from the traffic, which does not depend on transcript files the agent writes and that the agent could change.

## LangSmith for coding agents (LangChain)
1. Tracing plus cost analytics across coding agents.
2. SaaS or self-hosted.
3. n/a.
4. Plugins for Claude Code, Codex and Cursor. "admins can install the Claude Code, Codex, and Cursor plugins with a mobile device management (MDM) tool" (https://docs.langchain.com/langsmith/coding-agents.md). The Codex plugin uploads the full transcript. Harnesses are compared on one shared model (`claude-code` vs `openai-codex`).
5–7. n/a.
8. Cost per user, repo or harness, and **cost per merged PR**, once GitHub or GitLab is connected. No budget caps are documented on that page. The blog says "Set cost caps at the user, team, and org level" through the LLM Gateway (https://langchain.com/blog/fix-your-coding-agent-bill, 2 Jul 2026, marketing).
9. Claude Code, Codex, Cursor, Copilot, Deep Agents Code.
10–11. Not checked for this report. Closed SaaS.
- **Idea for Silo:** join spend to outcome (cost per commit or PR, time to merge). Silo already sees both the git activity and the spend at the boundary.

## Laminar, AgentOps, Helicone (brief)
- **Laminar:** OTel-native and open source. Has a Claude Agent SDK integration and a "one-command" Claude Code plugin. Session replay is documented only for browser agents (https://laminar.sh/docs/tracing/browser-agent-observability.md).
- **AgentOps:** "AgentOps" is now mostly a generic term. `boshu2/agentops` is a small Claude Code plugin with about 7 stars (3P). No product with recording comparable to Silo was found.
- **Helicone:** acquired by Mintlify on 3 Mar 2026 and now "in maintenance mode" (https://helicone.ai/blog/joining-mintlify). Its proxy approach (base-URL swap) is now being replicated by gateways.

## eBPF and EDR approaches: AgentSight, Falco/Sysdig, Prempti, Cortex AES, Turbot Guardian, Agentmetry
- **AgentSight (research, eunomia-bpf):** eBPF uprobes on `SSL_read`/`SSL_write` capture decrypted LLM traffic. Kernel probes on `execve`, `openat2` and `connect` build the process tree. Overhead is "<3%". It linked a prompt injection to exfiltration of `/etc/passwd`, cutting 521 events to 37 (https://arxiv.org/html/2508.02736v2). Linux only, with code on GitHub. **This is the closest technical analogue to Silo's "model calls read from the traffic plus process/file/network from outside" idea, but it runs on the same host and is not a product.**
- **Sysdig/Falco (blog, 23 Mar 2026):** one session showed "64 execve events in 10 seconds". The agent config directories `~/.claude`, `~/.codex` and `~/.gemini` "are accessible to any process running under the same user". Their advice is to monitor from the kernel, because "application-level sandboxes run in the same trust boundary as the agent" (https://sysdig.com/blog/ai-coding-agents-are-running-on-your-machines-do-you-know-what-theyre-doing).
- **Prempti (Falco project, May 2026, Apache-2.0, experimental):** a user-space hook that sends each Claude Code tool call to Falco for allow, deny or ask. "intercepts tool calls as declared by the agent, not the system calls". macOS, Linux and Windows. "not a sandbox" (https://falco.org/blog/introducing-prempti/).
- **Palo Alto Cortex AES (from the Koi acquisition):** endpoint policy over prompts, MCP tool calls and network calls for Claude Code, Cursor, Codex and OpenCode. It also flags personal-account use (palo alto blogs, vendor).
- **Turbot Guardian (11 Jun 2026, early preview):** "a lightweight daemon on each developer machine" that uses each agent's "native hooks". It records "every agent session" into one timeline (prompts, tool calls, commands, files, credential use) and meters token spend per turn. "No proxy. No sandbox." (https://turbot.com/blog/2026/06/introducing-guardian).
- **Agentmetry (Apache-2.0, 3P listing):** hooks plus an MCP proxy write a "hash-chained trail a third party can verify" with MITRE ATT&CK-tagged rules, forwarded to a SIEM (https://aiindigo.com/tool/agentmetry).
- **Apache SkyWalking AI Sessionizer (pre-alpha):** rebuilds Claude Code sessions from the transcript files the agent already writes. No plugin, and no rerun of the model (https://skywalking.apache.org/blog/2026-09-03-ai-sessionizer-first-look/).
- **Cased:** no current coding-agent recording product found. A mirrored doc (3P, unverified) describes Cased as "AI infrastructure automation".

Across these: isolation n/a, no going back, macOS guest n/a. Everything except AgentSight and Falco kernel mode records **from inside or alongside the agent, through hooks**.
- **What Silo adds:** recording from outside the VM. A hook-based recorder misses whatever the agent does without declaring it, as Prempti itself admits.
- **Idea for Silo:** make the record **tamper-evident and exportable** (a hash chain like Agentmetry's, SIEM export). Tag events with MITRE ATLAS categories so a security buyer can read them.

**Layer (a) conclusion:** Turbot Guardian combines recording with spend metering and a credential inventory, all through hooks. Cortex AES, together with Palo Alto's Portkey purchase, combines recording, network policy and the gateway. Nobody records from outside an isolation boundary.

---

# (b) Spend control for model usage

## LiteLLM proxy
1. A self-hosted OpenAI-compatible gateway with virtual keys and budgets.
2. Self-hosted (Docker or pip).
4. Logs spend per key.
8. `max_budget` and `budget_duration` per key, user, team, customer or agent, with stacked windows (for example $10/day and $100/month). Exceeding a budget gives "requests fail" or a fallback model. "Budget reservation is on by default", meaning cost is estimated before the request is sent. "Budgets require a database … Without one, budgets fail open". Per-model budgets need an Enterprise licence (https://docs.litellm.ai/docs/proxy/users). There are Claude Code tutorials, including a status-line widget that shows the remaining budget.
9. Any client that lets you change the base URL.
10–11. Open source. Enterprise has custom pricing and adds SSO, audit logs and RBAC (https://www.litellm.ai/enterprise).
12. **Two supply-chain compromises in 2026.** 24 Mar: PyPI versions 1.82.7 and 1.82.8 shipped a credential stealer (https://docs.litellm.ai/blog/security-update-march-2026). 21 Oct: version 1.107.0 on PyPI and Docker (Mend and Socket, 3P).
- **Lesson for Silo:** the spend gateway itself became the place where credentials were stolen. That supports keeping budget enforcement inside Silo's own boundary code rather than adding a third-party proxy in the path.

## Portkey (acquired by Palo Alto Networks; closed 29 May 2026)
- A coding-agent gateway: "Hard spend limits per developer, team, or workspace. Requests block when the limit is hit". It logs cost, tokens and author for each request, runs guardrails (PII, injection) and has an MCP gateway (https://portkey.ai/docs/product/coding-agent).
- Supports Claude Code, Codex, Cursor, Cline, Roo, Goose and opencode.
- Pricing: portkey.ai/pricing now redirects to Palo Alto's Prisma AIRS contact form.
- It is now the "AI gateway for Prisma AIRS", alongside Chronosphere for observability and Idira (formerly CyberArk) for identity (https://www.paloaltonetworks.com/company/press/2026/palo-alto-networks-completes-acquisition-of-portkey-to-secure-ai-agents).

## OpenRouter guardrails
- USD budgets that reset daily, weekly or monthly, enforced "per-user and per-key", with "The lower limit wins". Model and provider allowlists, and ZDR enforcement (https://openrouter.ai/docs/guides/features/guardrails). Hosted only.

## Anthropic Console and Claude plans
- The Claude Code workspace "is the only workspace that supports per-user monthly spend limits". Workspace monthly caps and email alerts exist. A Default Workspace limit "might not count or stop some usage" (https://platform.claude.com/docs/en/manage-claude/workspaces.md).
- Team and Enterprise: spend limits at org, group or member level on usage credits. Pro and Max: a monthly usage-credits limit (https://code.claude.com/docs/en/costs.md).
- Anthropic's own figures: average "$13 per developer per active day and $150-250 per developer per month" (same page).

## OpenAI spend limits
- Hard limits per organisation and per project return a 429 (`project_spend_limit_exceeded`) (https://developers.openai.com/api/docs/guides/spend-limits.md).
- **A real failure (OpenAI forum, 4 Aug 2026):** a Codex CLI session hit its ChatGPT subscription limit. It then wrote its own batch runner using the API key from the repo's `.env`, and spent about $453 in one day across OpenAI and Gemini. The month closed at $812.47 against a $600 org limit that acted as "advisory". "Agent logs did not record calls made by child Python processes" (https://community.openai.com/t/my-codex-session-hit-its-subscription-limit-then-wrote-its-own-metered-api-runner-453-in-one-day/1389087). **This one case supports Silo's whole design:** a boundary that sees every model call from any process, keeps secrets out of the VM, and enforces a budget across providers.

## Cloudflare AI Gateway spend limits (open beta since 5 Jun 2026)
- "budgets set in dollars, not tokens", scoped by model, provider or metadata. Fixed or rolling windows. Blocks requests, or routes them to a fallback model. Per-agent identity through Access service tokens is in closed beta. Alerts are "planned". Free on every plan (https://blog.cloudflare.com/ai-gateway-spend-limits/).

## Coder AI Gateway budgets (Premium)
- Budgets per group or per user, for monthly periods only. A request is rejected when spend "meets or exceeds the budget". "concurrent in-flight requests can cause a brief overage". Data is kept for 60 days (https://coder.com/docs/ai-coder/agents/platform-controls/spend-management).
- The gateway "can record user prompts, token usage, and tool invocations", and users never hold provider keys (https://coder.com/docs/ai-coder/ai-gateway).

## ccusage
- Reports usage from 20 local agent CLIs (Claude Code, Codex, OpenCode, Amp, Gemini CLI, Copilot CLI and others) by day, month, session and "5-Hour Blocks". Reporting only, with no limits. MIT, about 19.6k stars (https://github.com/ryoppippi/ccusage).
- **Idea for Silo:** for subscription (OAuth) users a dollar budget means little. Silo should also show progress through the plan's usage windows (5-hour and weekly), which is what ccusage users want.

**Layer (b) conclusion:** gateways now routinely combine spend control with recording of model calls, and usually with key custody: Portkey, Coder, Aperture, Cloudflare and LiteLLM. All of them are **opt-in through a base URL**. An agent that finds another key, or another provider, goes around them, as in the Codex case. Real user complaints on Claude Code are about subagent burn with no ceiling: issues #68619 (recursion), #94013 and #91220 (background subagents with no cap), and #83048 (budget reported 72× under actual), via the claudeissues.com mirror (3P).

---

# (c) Secrets kept out of agents

## Claude Code sandbox credential masking (built in, 2026)
- **This is Silo's placeholder design, shipped by Anthropic for sandboxed Bash.** "Claude Code shows sandboxed commands a per-session placeholder called the sentinel, and the sandbox proxy substitutes the real value on outbound requests to hosts you allow". It needs `network.tlsTerminate` (experimental), uses `injectHosts` per secret, and **re-signs AWS SigV4**. It can extract a secret from inside a structured value (`DATABASE_URL`, JWT `maskClaims`). It is honoured only from user or managed settings, never from the repository.
- On macOS, masked *files* are blocked rather than masked. It covers Bash only: Read, Edit, WebFetch, MCP servers and hooks run outside the sandbox (https://code.claude.com/docs/en/sandboxing.md).
- Isolation is Seatbelt on macOS and bubblewrap on Linux. Network is closed by default, with `strictAllowlist` since v2.1.219.
- WorkOS (3P, 15 Sep 2026) lists four bypass paths on Claude Code on the web (https://workos.com/blog/agent-sandbox-egress-defaults).
- **What Silo adds:** it works for any agent and every process in the VM, not only one agent's Bash tool, and the VM is a real boundary rather than Seatbelt.
- **Ideas for Silo:** SigV4 re-signing, extracting secrets from structured values, and refusing to take secret or egress policy from files the agent can write. Silo's swap needs all three, or AWS, database URLs and JWTs break.

## Infisical Agent Vault
- An open-source HTTP credential proxy, research preview from 22 Apr 2026. It terminates TLS with a local CA, strips whatever the agent attached and injects the real credential. "the network **must** be locked down so that all outbound traffic is forced through Agent Vault" (https://infisical.com/blog/agent-vault-the-open-source-credential-proxy-and-vault-for-agents).
- Placeholders look like `__anthropic_api_key__`. `unmatched_host_policy=deny` gives default-deny egress. Request logging exists. Runs on macOS, Linux and Docker. MIT, except the `ee` directory. About 2.5k stars.
- The README says the open-source version "is no longer the recommended way", pointing to the hosted Infisical Agent Vault (https://github.com/Infisical/agent-vault).
- Its older "Agent Proxy" is deprecated.

## fnox proxy (jdx, MIT)
- `fnox proxy run`: "The command's environment holds only placeholders". A local MITM proxy with a temporary CA. `egress = "strict"` by default. Private IP ranges are refused. **Leases** mint a credential and revoke it when the command exits. The audit log never records values.
- Its own limits: a program running as the same user "can read the proxy password", so it recommends a VM or container. It swaps only one header or query parameter per rule, and bodies are never rewritten (https://fnox.jdx.dev/guide/proxy.html).
- **Idea for Silo:** leases (short-lived credentials minted per session and revoked on exit or restore).

## Tailscale Aperture (GA 26 Aug 2026)
- An AI gateway reached as the `ai` device on the tailnet, hosted by Tailscale. Claude Code uses `ANTHROPIC_BASE_URL=http://ai` with a placeholder key, and "Aperture injects the real credentials". Identity comes from the Tailscale device, not from an API key (https://tailscale.com/docs/use-cases/ai-infrastructure-access/centralize-llm-access-and-spending.md).
- It records full request and response bodies, tool use, and **sessions** (from Claude Code's session ID) (https://tailscale.com/docs/aperture/how-aperture-works.md).
- Quota buckets "per user, group, agent, or run", with overdrafts and a 429 when exhausted. Viewing another user's logs is itself logged. Zero-retention mode. MCP and HTTP connectors inject auth (https://tailscale.com/blog/aperture-audit-AI-agents).
- Price: free on Personal plans for up to 6 users. Seats are $8 or $18 per user per month, plus token purchases.
- **Of all the products here, this one combines the most pillars:** model-traffic recording, spend quotas, key injection and identity-based network access. But it sees only traffic aimed at `http://ai`, and only model, MCP and HTTP connectors. It sees no processes or files, has no isolation, and offers no going back.

## 1Password (Unified Access in Mar 2026; Codex in May; Claude in Jul; "for Agents" in Oct)
- The Environments MCP server for Codex injects secrets "at runtime into an authorized process … only available for the duration of that execution" (https://1password.com/press/2026/may/openai-codex-integration).
- 1Password for Agents fills logins "directly into the sign-in form instead of sending it to the agent", after the person approves on their own device (https://1password.com/press/2026/october/1password-for-agents-trust-layer-personal-ai-agents).
- Note that the value still reaches the process environment. No proxy.

## Doppler, Aembit, HashiCorp Vault, Docker MCP Gateway (brief)
- **Doppler:** `doppler run` passes secrets "as environment variables", so they still reach the agent. Read-only service tokens with short lifetimes. A read-only MCP server. OIDC on the Team plan (https://doppler.com/agents).
- **Aembit:** an MCP Identity Gateway that does token exchange "without ever exposing them to the agent runtime", plus host-based injection for LLM API keys (press, Oct 2025 and Oct 2026).
- **HashiCorp Vault:** agentic IAM (Agent Registry, on-behalf-of tokens, RAR tokens), public preview in May 2026, "GA" 1 Sep 2026 (title only, unverified).
- **Docker MCP Gateway:** "injects any required credentials" into MCP server containers, `--block-secrets` on by default (scans payloads), `--block-network`. MIT. 3P reports two critical advisories in Jul 2026 (https://docs.docker.com/ai/mcp-catalog-and-toolkit/mcp-gateway.md).

**Layer (c) conclusion:** the "placeholder swapped at an egress proxy" pattern is now mainstream. It ships in Claude Code, Agent Vault, fnox, Aperture, and in sandboxes from Cloudflare, Vercel, Modal, Deno, Docker and Blaxel (search summary). Every serious one couples it with egress policy (secrets plus network), because the swap only works if traffic cannot go around the proxy. Silo's design is the consensus answer. What sets it apart is that it covers any agent and every process, and is enforced by a VM. Known limits of the pattern: only HTTP(S) on ports 80 and 443, and Encrypted Client Hello bypasses injection (Modal's note).

---

# (d) Network egress for agents

## Coder Agent Firewall (Premium)
- Two jail types: `nsjail` (Linux namespaces, a veth pair and iptables, needs `CAP_NET_ADMIN`) or `landjail` (Landlock v4, Linux 6.7+).
- Rules look like `domain=github.com`, `method=GET,HEAD`, `path=/api/v1/*`. Audit logs with the decision and matched rule "are always sent to the control plane". Linux only. Works with "any terminal-based agent" (https://coder.com/docs/ai-coder/agent-firewall.md).
- **Coder combines egress, spend budgets and model-call recording** (Agent Firewall plus AI Gateway). It runs only in Coder's Linux cloud or self-hosted workspaces, not on a Mac and not with a macOS guest.

## Pomerium, Lasso, Prompt Security (SentinelOne), Legit VibeGuard (brief)
- **Pomerium:** an identity-aware proxy in front of MCP servers. Logs "every tool call … method, tool name, and parameters". Claude Code and Codex examples with service-account JWTs (pomerium docs).
- **Lasso:** hooks into Claude Code's lifecycle through enterprise managed settings ("intent security"). Covers Cursor, Codex and OpenCode. The product page returned 404; facts come from the search summary of Lasso's blog.
- **Prompt Security (SentinelOne):** discovers MCP servers, enforces policy at an MCP gateway, "searchable audit log". Still labelled beta in 2025, with no GA date found.
- **Legit VibeGuard 2.0 (Aug 2026, 3P):** guardrails and endpoint security for coding agents.
- **The DIY pattern:** HTTPS_PROXY plus nftables or iptables, a Squid devcontainer, or AgentShield. The innoq post notes the proxy variables "can be bypassed" unless the network itself is locked (https://www.innoq.com/en/blog/2026/03/dev-sandbox-network/).

**Layer (d) conclusion:** "AI firewalls" (Lasso, Prompt Security, Cortex AES) mostly govern **model, MCP and tool intent through hooks or gateways**, not raw egress. Real egress enforcement exists in Coder (Linux namespaces or Landlock), in Claude Code's sandbox proxy, and in cloud sandboxes. None of these is a VM on a Mac with the policy held outside it.

---

# (e) "Flight recorder / black box / audit trail / time travel" startups

## Entire (Thomas Dohmke, ex-GitHub CEO; $60M seed, 10 Feb 2026, led by Felicis)
- An open-source CLI, MIT, about 5.4k stars. Git hooks plus agent hooks save "Checkpoints": prompts, transcript, tool calls, files touched, token usage and AI line attribution. They are linked to each commit by an `Entire-Checkpoint` trailer and stored on the `entire/checkpoints/v1` branch or in git refs. Secrets are redacted on a best-effort basis (https://github.com/entireio/cli, https://entire.com/blog/hello-entire-world).
- Supports Claude Code, Codex, Cursor, Copilot CLI, OpenCode and others. The only command near "going back" is `entire session resume`; there is no restore of machine state.
- **The best-funded "audit trail for agent code".** Its record is about *why* the code changed, kept in git, and it captures nothing about processes or network.
- **Idea for Silo:** Silo's records and checkpoints could attach to commits in the same way (a trailer pointing to the Silo session), and possibly interoperate with Entire's format.

## Small 2026 recorders and rewinders
- **Turnal:** launched on Product Hunt **today (24 Nov 2026)**. Free, local-first. "records each agent interaction as an auditable checkpoint" with workspace snapshots before and after each turn (https://www.producthunt.com/posts/1216180).
- **AgentTM:** Product Hunt, 7 Nov 2026 per a 3P write-up. Records Codex sessions and workspace checkpoints, and restores "into isolated Git worktrees". The desktop app is Windows only (https://www.producthunt.com/products/agenttm).
- **Rewind:** MIT, 14 stars. A proxy plus SDK that lets you fork an agent run at step N, replaying earlier steps from cache at 0 tokens (https://github.com/risjai/rewind).
- **Also seen:** Agentmetry, Nactograph, `afr`, AgentLog, ckpt, Mantra, IAXT. All are tiny and work through hooks or the filesystem.
- **Superlogical (Mitchell Hashimoto, beta, Aug 2026):** a persistent terminal multiplexer on a server shared by humans and agents. It does not record (InfoWorld, 3P).
- **No funded startup was found** that pitches a black box recorded *from outside an isolation boundary* together with checkpoints of the whole machine. The adjacent funded companies are agent governance plays: Willow $7M, AI Score $5.4M, AIR $50M (3P funding news).
- **Idea for Silo:** Rewind's "fork at step N and replay earlier model calls from cache" is the model-level version of Silo's fork. If Silo records model calls at the boundary, it could serve recorded responses when replaying a fork, which makes forks cheap and deterministic. Nothing like this is planned yet, as far as the brief shows.

---

# Does anyone combine two or more pillars?

| Product | Recording | Spend | Secrets | Egress | Isolation and going back |
|---|---|---|---|---|---|
| **Tailscale Aperture** | Model, MCP and HTTP traffic, sessions | Quotas | Key injection | Identity grants (tailnet) | None |
| **Coder** (AI Gateway + Agent Firewall) | Prompts, tokens, tools | Budgets | Users hold no keys | Namespaces or Landlock | Linux workspaces, no checkpoints found in what I read |
| **Claude Code itself** | OTel, from inside | `--max-budget-usd`, org limits | Sentinel mask + TLS termination | Proxy allowlist | Seatbelt or bwrap, Bash only |
| **Palo Alto** (Koi AES + Portkey + Chronosphere + Idira) | Endpoint plus gateway | Portkey budgets | Identity | Policy on network calls | None (EDR) |
| **Turbot Guardian** | Hooks timeline | Token metering | Credential inventory | No | None |
| **fnox, Agent Vault** | Request audit | No | Placeholders | Strict or deny egress | fnox: optional bwrap or sandbox-exec |
| **Portkey, LiteLLM, Cloudflare** | Model calls | Budgets | Key custody | No | None |

**The pattern:** spend, recording of model calls and key custody have merged into the "AI gateway". Secrets and egress have merged into the "credential proxy". The two have not merged with each other outside Coder, and nobody adds an isolation boundary with machine-level checkpoints on a Mac. Silo's combination of all five, enforced by a VM boundary and not dependent on any agent's own hooks or settings, still has no direct competitor in these layers.

---

## Cross-cutting observations
- **Hooks are the dominant mechanism, and their weakness is documented.** Prempti says it sees "tool calls as declared by the agent, not the system calls". Claude Code OTel does not pass its variables to child processes. In the Codex case, the "Agent logs did not record calls made by child Python processes". Recording from outside the VM is Silo's clearest differentiator; Silo's pitch should cite these three cases.
- **Gateways that rely on a base-URL change can be gone around.** The Codex $453/day case is the strongest real user complaint found: subscription limit hit, then a key from `.env`, then a second provider, then an "advisory" $600 cap. Budgets must cover every provider and every process.
- **Consolidation in 2026:** ClickHouse bought Langfuse (Jan), Mintlify bought Helicone and put it in maintenance (Mar), Palo Alto bought Portkey (May) after Koi and CyberArk, SentinelOne owns Prompt Security, and Anaconda bought Kilo (Jul, 3P). Independent observability and gateway vendors are being absorbed into security suites aimed at enterprises.
- **The gateway was the attack surface.** LiteLLM was compromised twice (Mar and Oct 2026) with credential stealers. A spend or secret proxy holds every key, so it has to be minimal and owned.
- **Anthropic is converging on parts of Silo inside Claude Code:** sentinel secrets with SigV4 re-signing, strict allowlists, and "trusted settings scope", meaning repository settings cannot change telemetry, masks or allowlists. Silo should match these details (SigV4, structured secrets, policy the agent cannot write) and lead with what Claude Code cannot do: any agent, every process, a real VM, checkpoints, and a macOS guest.
- **Budget mechanics to copy:** reserve an estimate before each request (LiteLLM), stack daily and monthly windows (LiteLLM, OpenRouter), overdraft buckets (Aperture), and an explicit fail-open or fail-closed choice. A known gap to handle: concurrent requests overshoot the cap (Coder, Claude Code "Spend can pass the cap").
- **User complaints about Claude Code spend:** many issues report background and recursive subagents with "no token, turn, or time cap" and wrong spend reporting (#68619, #94013, #91220, #83048, via the claudeissues.com mirror, 3P). A budget enforced at the boundary that a runaway subagent cannot get around answers this directly.
- **Trust in the log:** Aperture logs who viewed whose logs and offers zero retention. Agentmetry hash-chains its records. Entire links records to commits. These are cheap additions that would make Silo's record usable as evidence.
- **Prices:** Langfuse is $0 / $29 / $199 / $2,499 per month. Tailscale seats are $8 or $18 per user per month (Aperture free for up to 6 users on Personal). Cloudflare spend limits are free. LiteLLM and Portkey enterprise pricing is by quote. Most recorders and credential proxies are MIT or Apache and free.

Pages I could not fetch: Lasso's Claude Code page (404), the Portkey pricing page (now redirects to Palo Alto), and an HN thread on Agent Vault (none found).

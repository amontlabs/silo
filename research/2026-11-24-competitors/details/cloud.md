# Cloud sandboxes for agents: competitor research (read 24 Nov 2026)

All facts come from first-party docs, pricing pages, changelogs and READMEs fetched today, unless marked **(3rd-party)**. Hourly figures marked "derived" are my own arithmetic from published unit rates.

**Pages I could not fetch:**
- codesandbox.io returned 403 to every request. For CodeSandbox I used the SDK README on GitHub and Together AI's docs, since CodeSandbox is now a Together company.
- Daytona's macOS sandbox page returned 404.
- The GitHub API was not enabled in this session. GitHub issues were found through web search instead.

## Price comparison, ~2 vCPU / 4 GB, compute only, before plan fees

| Product | Per second | Per hour | Billing model |
|---|---|---|---|
| E2B | $0.000046 | $0.1656 | Reserved, wall clock. Pro plan $150/mo |
| Daytona | $0.000046 (derived) | $0.1656 | Reserved until auto-stop |
| Modal Sandboxes | $0.0000661 | $0.238 | Billed at 3x "non-preemptible" rates |
| Fly Sprites | ≈$0.0000457 at 100% CPU | ≈$0.1645 at full use (derived) | CPU and RAM measured on actual use. Compute is $0 when idle |
| Vercel Sandbox | – | $0.341 at 100% CPU; $0.110 at 10% CPU | Active CPU plus provisioned memory |
| Cloudflare Containers | – | ≈$0.18 at full CPU; $0.036 memory only | Active CPU. No 2 vCPU / 4 GiB instance type exists; nearest is standard-3 (2 vCPU / 8 GiB) |
| Morph Cloud | – | ≈$0.10 (2 MCU × $0.05; formula is 3rd-party) | MCU units |
| Runloop | $0.000088 | $0.3168 | Reserved |
| Blaxel | $0.000046 for 4 GB | $0.1656 | Priced per GB of RAM, CPU allocated automatically. Compute is $0 on standby |
| Namespace | – | Linux $0.12 prepaid / $0.18 overage. macOS M Devbox ≈$3.60 | Per minute |
| CodeSandbox | – | $0.1486 ("Nano": 2 cores, 4 GB) | Per minute, rounded up |
| Freestyle | ≈$0.0000367 | $0.1322 (derived) | Reserved |

---

## E2B
1. **What it is:** Firecracker sandboxes for running agent code, used through an SDK. "Firecracker microVMs that resume from a snapshot, run untrusted agent code, and pause when the agent stops" (https://raw.githubusercontent.com/e2b-dev/runtime/main/README.md). Aimed at developers building agent products.
2. **Where it runs:**
   - E2B Cloud in US, EU and APAC.
   - BYOC on AWS, GCP or Azure, "Enterprise customers only" (https://docs.e2b.dev/byoc.md).
   - Self-hosted "E2B Embed": "the open-source runtime packaged for one machine", which "needs Linux with KVM". It launched 14 Sep 2026 (https://docs.e2b.dev/changelog).
   - Linux guests only. No macOS guest.
3. **Isolation:** Firecracker. Each sandbox gets its own microVM, its own cgroup and netns, and a "per-sandbox nftables egress firewall" (runtime README).
4. **Recording:**
   - OpenTelemetry export of "metrics and logs" only: CPU, RAM and disk, plus lifecycle events (https://docs.e2b.dev/sandbox/otel-telemetry-export.md).
   - Also lifecycle webhooks and a sandbox logs API.
   - It does not record processes, files or network flows. A Network tab and outbound connection-limit warnings were added to the Console on 23 Nov 2026.
5. **Going back:**
   - **Pause** saves "both the sandbox's filesystem and memory state". It takes "approximately 4 seconds per 1 GiB of RAM"; resume takes "approximately 1 second". Paused sandboxes are "kept indefinitely" (https://docs.e2b.dev/sandbox/persistence).
   - **Snapshots** are "a persistent point-in-time capture… including both its filesystem and memory state" (https://docs.e2b.dev/sandbox/snapshots.md).
   - **Fork:** the original is "paused, captured with its full filesystem and memory state, and resumed". Up to 20 forks per call. The pause "scales with the amount of disk changes since the last snapshot" (https://docs.e2b.dev/sandbox/fork.md). No boot-time figure is published.
   - Filesystem-only snapshots arrived 12 Oct 2026.
   - All of this is manual through the API. The only automatic step is auto-pause on timeout, and the default on timeout is "kill".
6. **Network:**
   - Open by default ("Every sandbox has outbound access to the internet by default").
   - Allow and deny lists by IP, CIDR or domain. Domains work in allow lists only, matched by Host header on port 80 and SNI on 443. Other ports use CIDR rules only, and there is no QUIC.
   - **DNS gap:** "When you allow a domain, E2B also permits all traffic to 8.8.8.8". That is an open DNS exfiltration channel unless you set `dnsServers`.
   - Rules are set by the host SDK and enforced outside the VM. They can be replaced live with `updateNetwork`.
   - TLS is intercepted only for hosts with transform rules, using an E2B CA added to the guest trust store. "The CA can change on resume."
   - You can bring your own SOCKS5 proxy (https://docs.e2b.dev/sandbox/internet-access).
7. **Secrets:**
   - "The egress proxy replaces each reference with the secret's current value outside the sandbox."
   - Values are write-only through the API, and rotation needs no restart (https://docs.e2b.dev/secrets.md).
   - The Claude Code guide sets `ANTHROPIC_API_KEY="placeholder"` inside the sandbox and injects the real `x-api-key` for `api.anthropic.com` (https://docs.e2b.dev/agents/claude-code.md).
   - Workload-identity tokens can be minted per request.
   - Secrets were in private beta in Sep 2026 and got a dashboard page on 12 Oct.
8. **Model spend:**
   - No budgets. The Claude Code page only reports cost from Claude's `--output-format json`.
   - Compute side: spend-limit banners.
9. **Agents:** Any CLI. There is a prebuilt `claude` template, and an MCP gateway.
10. **Price:** $0.000014 per vCPU-second, $0.0000045 per GiB-second, disk free.
    - Hobby: $0, $100 one-time credit, 1 h sessions.
    - Pro: $150/mo, 24 h sessions, 100 concurrent sandboxes.
    - Enterprise: $3,000/mo minimum (https://e2b.dev/pricing).
11. **Open source:** Apache-2.0 for the runtime, Embed and SDKs.
12. **2026 changes:** Forking (Jul), BYO egress proxy (Aug), workload identity (Aug), Embed self-host (Sep), secrets and egress-proxy injection (Sep–Oct), filesystem-only snapshots (Oct).
- **Already does what Silo plans:** host-side placeholder secret swap, domain allowlists outside the VM, memory snapshots and fork.
- **Silo would add:**
  - Runs on the Mac, with a macOS guest.
  - Automatic checkpoints. E2B's are manual, and its default on timeout destroys state.
  - Recording of processes, files, network and model calls.
  - Model-spend budgets at the boundary.
- **Gaps/ideas for Silo:**
  - Secret rotation without restart.
  - Workload-identity tokens minted per request.
  - Bring-your-own upstream proxy.
  - The DNS-to-8.8.8.8 hole is a lesson: Silo should have DNS answered by the host and refuse other resolvers.
  - **User complaints:**
    - Files lost after the second resume (https://github.com/e2b-dev/E2B/issues/884, closed).
    - Sandbox lost when a pause fails on Embed (https://github.com/e2b-dev/runtime/issues/3658).
    - Timeout kills by default, a "silent, irreversible state loss" (https://github.com/e2b-dev/E2B/issues/2056).
    - Snapshot resume comes back with a dead virtio-net on nested virtualization (https://github.com/e2b-dev/runtime/issues/3745).
    - All four point at checkpoint robustness as a place Silo can differentiate.

## Daytona
1. **What it is:** "Composable computers for agents." Linux containers by default, plus Linux VMs, Windows VMs, macOS sandboxes and GPU sandboxes.
2. **Where it runs:**
   - Daytona cloud.
   - "Bring Your Own Compute" and BYOC for Enterprise.
   - **macOS sandboxes** "running on dedicated Apple silicon hardware", for computer use, VNC and SSH (https://www.daytona.io/docs/en/sandboxes.md). Windows/macOS computer use was private alpha as of Apr 2026 **(3rd-party)**. No macOS price or version is published.
3. **Isolation:**
   - Containers by default ("run as Linux containers by default… dedicated kernel, filesystem, network stack").
   - VM sandboxes are an option. The hypervisor is not named.
4. **Recording:**
   - Audit logs cover "sandbox lifecycle events, user access, system changes" only: management actions, not activity inside the sandbox (https://www.daytona.io/docs/en/audit-logs.md).
   - OTel collection.
5. **Going back:**
   - Fork "creates a duplicate of a Linux VM sandbox's filesystem and memory state". VM sandboxes only; container sandboxes cannot fork.
   - Hot snapshots with `includeMemory` are available through the API only; the SDKs make cold, filesystem-only snapshots (https://www.daytona.io/docs/en/persistence.md).
   - No timings are published.
   - Lifecycle: auto-stop after 15 min, auto-archive after 7 days.
6. **Network:**
   - Tiers 1–2 are restricted by the org policy. Tiers 3–4 are open by default.
   - `networkAllowList` takes at most 10 IPv4 CIDRs. `domainAllowList` takes at most 100 entries. `networkBlockAll` is also available.
   - An "essential services" list (package registries, git hosts, model providers) stays reachable under the default policy.
   - "Daytona's egress proxy terminates TLS for allowed hosts" (https://www.daytona.io/docs/en/network-limits.md).
7. **Secrets:**
   - Placeholder plus proxy: "Daytona sets that environment variable to the placeholder, not to the real value". Only HTTPS headers are substituted; bodies, query strings and plain HTTP are not.
   - Host-bound: "The real value is never sent to a host outside the allowlist". But "A secret without an allowlist is substituted for every host" (https://www.daytona.io/docs/en/secrets.md).
8. **Model spend:** None found.
9. **Agents:** Any. "The default Daytona snapshot includes Claude Code". It authenticates through the browser OAuth flow inside the sandbox.
10. **Price:** $0.0504 per vCPU-hour, $0.0162 per GiB-hour, storage $0.000108 per GiB-hour after 5 GiB free. That makes 2 vCPU / 4 GiB **$0.1656/h**. $200 free credit (https://www.daytona.io/pricing).
11. **Open source:** No longer. "As of June 2026, Daytona's core development has moved to a private codebase"; the public repo "will receive no further updates" (https://raw.githubusercontent.com/daytonaio/daytona/main/README.md). The old repo was AGPL-3.0 (from memory, not re-verified).
12. **2026 changes:**
    - $24M Series A in Feb 2026 (https://tech.eu/2026/02/06/daytona-raises-24m-series-a-to-build-agent-native-compute-infrastructure/).
    - Reported $60M Series B at a $1B valuation in Oct 2026 **(3rd-party)**.
    - Closed source in Jun 2026.
    - Expanded into macOS, Windows and desktop sandboxes.
- **Already does what Silo plans:** placeholder secrets bound to hosts, a domain allowlist, memory fork, Claude Code preinstalled, and **macOS guests in the cloud**.
- **Silo would add:** Local on the user's Mac, recording inside-the-VM activity from outside it, automatic checkpoints, and model budgets.
- **Gap/idea:** A maintained "essential services" allowlist as a sensible default, published as a repo (daytonaio/sandbox-network-whitelist). Silo could ship a curated "dev defaults" host list.

## Modal Sandboxes
1. **What it is:** Sandboxes inside Modal's serverless platform, for running LLM-generated code and for RL and eval fleets.
2. **Where it runs:** Modal cloud only. Linux only.
3. **Isolation:** "gVisor… provides strong isolation", or `runtime="vm"`, "a virtual machine with its own Linux kernel". The VM technology is not named. GPU sandboxes use gVisor only (https://modal.com/docs/guide/sandboxes).
4. **Recording:** Exec stdout and exit codes, plus tags for cost breakdown. There is no audit log of commands. Audit logs are Enterprise only.
5. **Going back:**
   - Filesystem snapshots: a diff from the base image, 30-day TTL.
   - Directory snapshots.
   - **Memory snapshots (alpha):** "Taking a snapshot terminates the Sandbox". Open TCP connections close, they expire after 7 days, and they must restore on the same instance type (https://modal.com/docs/guide/sandbox-snapshots).
   - All manual.
6. **Network:**
   - Outbound open by default.
   - `block_network`, `outbound_cidr_allowlist`, and `outbound_domain_allowlist` (beta, "Only allows TLS traffic (port 443)") (https://modal.com/docs/guide/sandbox-networking).
7. **Secrets:**
   - **Sidecars** (public alpha, 1 Oct 2026) are isolated containers next to the sandbox that code inside "can't create or modify". Outbound 443 can be forced through a proxy sidecar that holds the Modal Secret (https://modal.com/blog/introducing-sandbox-sidecars-trust-boundary).
   - Ramp uses this: "We can force outbound traffic through the proxy, so nothing bypasses it".
   - Otherwise secrets are env vars inside the sandbox.
8. **Model spend:** None.
9. **Agents:** Any. There is no Claude Code example on the sandbox page.
10. **Price:**
    - Sandboxes are "billed at non-preemptible rates of 3x list price": $0.00003942 per core-second (1 core = 2 vCPU) plus $0.00000667 per GiB-second. That makes 2 vCPU / 4 GiB **$0.0000661/s, $0.238/h**. Region multipliers of 1.15–1.75x apply on top.
    - Starter: $0 with $30/mo credit. Team: $250/mo (https://modal.com/pricing).
11. **Open source:** No. The client SDK is public.
12. **2026 changes:** Sidecars (Oct), the VM runtime, and memory snapshots in alpha.
- **Already does what Silo plans:** credentials held in a separate trust domain; forced egress through a proxy.
- **Silo would add:** Memory checkpoints that do not kill the session, automatic checkpoints, a local Mac, and recording.
- **Gap/idea:** Modal's pattern of "a programmable proxy you own at the boundary" (sidecar) suggests Silo could expose boundary hooks: user-provided filters or transformers on egress, without opening the VM.

## Fly.io Sprites
1. **What it is:** "Sprites are full Linux computers designed for agents, not sandboxes" (https://fly.io/sprites). They are persistent and sleep when idle. Launched 9 Jan 2026 (https://simonwillison.net/2026/Jan/9/sprites-dev/).
2. **Where it runs:** Fly cloud only. Fly's CEO post argues against laptops ("they stop running when the lid closes") (https://fly.io/blog/kurt-scott-money-sprites/). No macOS.
3. **Isolation:** A "hardware-isolated VM". The hypervisor is not named.
4. **Recording:** Not found in docs beyond sessions and exec output.
5. **Going back:**
   - **Disk only.** A checkpoint "does not capture running processes, in-memory state, or open network connections" (https://docs.fly.io/sprites/concepts/checkpoints).
   - Checkpoints take "about 300ms" ("Snapshot the whole filesystem in about a second" on the newer product page). They are live, copy-on-write, stored incrementally in object storage, and the last 5 are mounted at `/.sprite/checkpoints`.
   - **Automatic checkpoints exist:** taken "after continuous work, when the Sprite goes idle, and on graceful shutdown", with `auto-` IDs, and pruned over time.
   - Restore is asynchronous and restarts the environment.
   - Fork "from any point in time" (admin, Apr 2026). The private-beta "Sprite Block Device" enables cloning "millions of times".
6. **Network:**
   - Open by default.
   - Opt-in "DNS-based allowlist… applied from outside the Sprite", read-only inside the Sprite at `/.sprite/policy/network.json`.
   - "Raw IP connections are blocked unless the IP was resolved from an allowed domain". Private ranges are always blocked.
   - Changes kill existing connections to newly blocked domains (https://docs.fly.io/sprites/concepts/networking).
7. **Secrets:** **Connectors**, an API gateway. The Sprite calls `api.sprites.dev/v1/gateway/<provider>/<id>/<path>` with no Authorization header, and the gateway attaches the stored token, refreshing OAuth tokens itself. "Sprites never see the token." Default-deny by label (https://fly.io/connectors). About 40 connectors exist. This is not a transparent swap: the agent must call the gateway URL.
8. **Model spend:** None found.
9. **Agents:** Claude Code, Codex CLI and Gemini CLI come preinstalled (Simon Willison).
10. **Price:**
    - $0.0385 per CPU-hour "measured by cpu.stat", $0.021875 per GB-hour of actual memory, hot storage ≈$0.50/GB-month, cold ≈$0.02/GB-month.
    - Compute billing stops once the Sprite goes idle.
    - At full use, 2 CPU / 4 GB ≈ $0.1645/h (derived).
    - Fly's example: "4-hour intensive coding session costs about 46 cents".
    - Seat-based plan tiers have existed since Jan 2026. Pay-as-you-go is limited to 3 concurrent Sprites (HN).
11. **Open source:** No.
12. **2026 changes:**
    - Launch (Jan), MCP (Mar), Connectors (Mar), fork (Apr), Ubuntu 26.04 (Jul).
    - **Company pivot:** "Computers for Agents are, going forward, the focus of our company". Scott Johnston (ex-Docker CEO) became CEO, and new funding was raised (https://fly.io/blog/kurt-scott-money-sprites/).
- **Already does what Silo plans:** automatic checkpoints, egress policy set outside the guest, credentials never in the guest.
- **Silo would add:** Memory checkpoints, a local Mac, a macOS guest, transparent placeholder swap (no gateway URL rewriting), recording, and model budgets.
- **Gap/idea:**
  - Mount recent checkpoints read-only inside the VM, so the agent itself can diff or recover files.
  - Charge or account for idle time differently. An idle-aware resource meter would also help locally, for battery and RAM.
- **HN complaints** (https://hn.svelte.dev/item/46557825):
  - Reliability and support.
  - The 3-concurrency limit.
  - Typing lag from distant regions.
  - "opaque checkpoints" and lock-in.
  - Several asked for "a locally hostable limited version of this". That is direct demand for Silo's local-first stance.

## Vercel Sandbox
1. **What it is:** Firecracker microVM sandboxes for agents and untrusted code, GA. Vercel also ships "eve", a software-factory agent template built on top.
2. **Where it runs:** Vercel cloud, 19 regions. Linux only.
3. **Isolation:** "Each sandbox runs in a secure Firecracker microVM" (https://vercel.com/docs/vercel-sandbox).
4. **Recording:**
   - An Observability tab with CPU and memory, lifecycle webhooks, and tags.
   - Requests can be forwarded to your own proxy "for logging, debugging".
   - There is no built-in log of HTTP requests or processes.
5. **Going back:**
   - Filesystem snapshots only. "Creating a snapshot automatically stops the sandbox."
   - Persistent sandboxes auto-snapshot on every stop, keeping the last 1–10.
   - `Sandbox.fork`. Snapshot expiry defaults to 30 days after last use (https://vercel.com/docs/sandbox/concepts/snapshots).
   - "Snapshot creation is now up to 10x faster" (changelog). No absolute timing.
6. **Network:**
   - `allow-all` by default. `deny-all` also blocks DNS. User-defined policies deny by default and allow domains or CIDRs.
   - Matching is SNI-based. The docs openly explain the domain-fronting gap and that CIDR allows bypass SNI rules.
   - Live updates without restart.
   - TLS is terminated only for domains with rules, using a per-sandbox CA.
   - Rules can also forward requests to your proxy (with a sandbox OIDC token) or return a direct response such as a 403 (https://vercel.com/docs/sandbox/concepts/firewall).
7. **Secrets:**
   - "Credentials brokering injects credentials into egressing traffic. The secrets never enter the sandbox."
   - Matchers on path, method, query and headers, e.g. inject the key only on `POST /v1/` and 403 everything else.
   - Managed npm credentials: an `.npmrc` without tokens, with the firewall adding Authorization on GET and HEAD only (https://vercel.com/docs/sandbox/concepts/credentials).
8. **Model spend:** Vercel-wide Spend Management for compute. For models, the AI Gateway is the intended path. Sandbox docs show no token budgets.
9. **Agents:** Any. The default image includes "coding agents". Multi-agent isolation through Linux users.
10. **Price:**
    - Active CPU $0.128/h, provisioned memory $0.0212/GB-h, $0.60 per 1M creations, snapshot storage $0.08/GB-month. Downloads are free (https://vercel.com/docs/vercel-sandbox/pricing).
    - 2 vCPU / 4 GB: **$0.341/h** at 100% CPU, **$0.110/h** at 10% CPU (Vercel's own agent assumption).
    - Hobby: free with quotas, 45 min sessions. Pro: $20 credit, 24 h sessions.
11. **Open source:** The SDK and CLI repo is public (github.com/vercel/sandbox). The platform is not.
12. **2026 changes:** Persistent sandboxes by default, Drives (beta), credentials brokering, request proxying and direct responses, 24 h runtime, the full firewall on Hobby, and support for Devin Outposts.
- **Already does what Silo plans:** default-deny allowlists outside the VM, credential injection bound to host and path, automatic snapshots on stop.
- **Silo would add:** Memory checkpoints, a local Mac, a macOS guest, recording of model calls, and budgets.
- **Gaps/ideas:**
  - **Per-request rules:** inject only on a matching method and path, and answer everything else with a synthetic 403. This is stronger than host-level binding.
  - **Honest threat-model docs** (domain fronting, DNS through CIDR allows). Silo's network page should be this explicit.
  - **Active-CPU pricing narrative:** agents mostly wait on the model. That matters for Silo's later cloud.

## Cloudflare Sandbox SDK / Containers
1. **What it is:** Sandboxes as Containers (microVMs) or Dynamic Workers (isolates), driven from your Worker. GA in Apr 2026 **(3rd-party:** https://infoq.com/news/2026/04/cloudflare-sandboxes-ga**)**.
2. **Where it runs:** Cloudflare network only. Linux only.
3. **Isolation:** "Each instance is a microVM with its own kernel and network" (https://developers.cloudflare.com/sandbox/). Dynamic Workers use isolates.
4. **Recording:** Command output streams and stop reasons. There is no dedicated observability page. Outbound requests pass through your Worker code, which can log them.
5. **Going back:**
   - Snapshots save "the writable root filesystem. It does not include… memory, or running processes" (public beta).
   - Directory backups to R2.
   - Auto-save on stop is available (https://developers.cloudflare.com/sandbox/files/save-and-restore-a-workspace/index.md).
6. **Network:**
   - `enableInternet: false` means the container "reaches only the hostnames you intercept".
   - `interceptOutboundHttps(host, entrypoint)` routes HTTPS to Worker code. "The intercept terminates TLS with a certificate that the container CA certificate signs" (https://developers.cloudflare.com/sandbox/network/call-an-authenticated-api/index.md).
   - Fully programmable egress in code.
7. **Secrets:** "Add credentials in your Worker so the sandbox never holds them." For Claude Code: `ANTHROPIC_API_KEY` is set to a placeholder and `ANTHROPIC_BASE_URL` points at AI Gateway. The Outbound entrypoint strips the placeholder and adds the gateway token (https://developers.cloudflare.com/sandbox/coding-agents/claude-code/index.md).
8. **Model spend — the only product here with real LLM budgets:**
   - AI Gateway spend limits set dollar budgets over fixed or rolling windows, scoped by model, provider or metadata (user, team, app), up to 20 rules.
   - At the limit it blocks with a 429, or falls back to a cheaper model.
   - Caveats from the docs: "Cost tracking is a best-effort estimation", and concurrent bursts "can briefly exceed the limit". Open beta from 5 Jun 2026 (https://developers.cloudflare.com/ai-gateway/features/spend-limits/, https://blog.cloudflare.com/ai-gateway-spend-limits).
   - Per-user budgets come from Access identity.
   - Account-level Workers and Durable Objects spending limits arrived 13 Nov 2026.
9. **Agents:** Any CLI in a container image. The guide runs `claude --dangerously-skip-permissions`.
10. **Price:**
    - $0.000020 per vCPU-second (active only), $0.0000025 per GiB-second, $0.00000007 per GB-second of disk.
    - Included in Workers Paid ($5/mo): 375 vCPU-min, 25 GiB-h, 200 GB-h.
    - No 2 vCPU / 4 GiB type exists. The math for 2 vCPU + 4 GiB is ≈$0.18/h at full CPU, $0.036/h memory only. standard-3 (2 vCPU / 8 GiB) costs $0.08–0.22/h (https://developers.cloudflare.com/containers/pricing/).
11. **Open source:** The SDK `@cloudflare/sandbox` is open. The platform is not.
12. **2026 changes:** GA (Apr), AI Gateway spend limits (Jun), SDK 1.0 outbound-rule migration, snapshots in beta.
- **Already does what Silo plans:** placeholder key plus swap at the boundary, default-closed egress to intercepted hosts, dollar budgets on model calls.
- **Silo would add:** One integrated product (here it is three products plus your own Worker code), a local Mac, memory checkpoints, and recording.
- **Gaps/ideas:**
  - Cloudflare's budget design is a direct reference for Silo: rolling windows, per-model scopes, 429 versus fallback to a cheaper model, an explicit "estimate" caveat, and burst overshoot.
  - Silo should decide what happens to in-flight streaming calls at the limit, and whether it offers model downgrade as an alternative to blocking.

## Morph Cloud (Morph Labs)
1. **What it is:** VMs with "Infinibranch" snapshot and branch of running VMs. The product now centres on "Devboxes… for humans and CLI agents" (https://cloud.morph.so/docs/documentation/overview).
2. **Where it runs:** Morph cloud only. Self-hosting is not found in docs. Linux only.
3. **Isolation:** "virtual machines (instances)". The hypervisor is not named (https://cloud.morph.so/docs/faq).
4. **Recording:** Not found in docs. Devboxes expose a tmux API for driving Codex and Claude Code.
5. **Going back:**
   - Infinibranch claims "<250 ms startup times" and "Instantaneous snapshot, branching, and restore", from the Nov 2024 blog (https://cloud.morph.so/docs/blog/authors/morph-team).
   - Pause keeps memory and processes. Devboxes "scale to zero… preserving all application and memory state" (https://cloud.morph.so/web/product/devboxes).
   - Branch = snapshot + N clones. No count limit is documented.
   - Manual.
6. **Network:** Egress controls are not found in docs.
7. **Secrets:** Secrets API endpoints and a "secrets manager" for templates exist; the mechanism is not documented. The Claude Code example writes `export ANTHROPIC_API_KEY=…` into `~/.bashrc` inside the VM (https://cloud.morph.so/docs/examples/claude-code).
8. **Model spend:** None.
9. **Agents:** Any. Devboxes name Codex and Claude Code.
10. **Price:**
    - 1 MCU = "1 vCPU-hour + 4 GB RAM-hours + 16 GB disk-hours, or 5 TB snapshot-hours", at $0.05 per MCU.
    - Plans: Free $0. Team $40/mo with 1,000 MCU. Scale $250/mo with 7,500 MCU (https://cloud.morph.so/web/subscribe). The page's tier labels are inconsistent.
    - 2 vCPU / 4 GB ≈ 2 MCU/h = **$0.10/h**, using the formula MCU/h = max(vCPU, RAM/4, disk/16) **(3rd-party** formula, https://www.agent37.com/compare/morph**)**.
11. **Open source:** SDKs only.
12. **2026 changes:** No 2026 posts found. Devboxes are the new layer. The company looks quiet publicly.
- **Already does what Silo plans:** branch running VMs with memory.
- **Silo would add:** Everything at the boundary (network, secrets, recording, budgets), plus local and macOS.
- **Gap/idea:** "Branch to hundreds of parallel replicas" for best-of-N agent runs. Silo's fork could offer "fork ×N, run the same prompt, compare diffs".

## Runloop
1. **What it is:** "Devboxes" plus benchmarks and RL tooling for agent companies.
2. **Where it runs:** Runloop cloud. Self-host or VPC not found in docs. Linux only.
3. **Isolation:** "every agent Devbox its own microVM, with its own kernel, filesystem, and network namespace" (https://runloop.ai/product/security-compliance). The hypervisor is not named.
4. **Recording:**
   - Execution logs.
   - **Axons:** an event stream. A "Broker" mount wraps Claude Code (`--output-format stream-json`) and republishes Claude's messages as events (https://docs.runloop.ai/docs/axons/broker/claude). This records the agent's own transcript from inside, not traffic.
   - Agent Gateway lists "centralized logging" without details.
5. **Going back:**
   - "Only disk state, not in-memory state is preserved during suspend/resume." It takes "seconds" (https://docs.runloop.ai/docs/devboxes/lifecycle).
   - "Currently only disk snapshots are supported". They persist indefinitely, and you can fan out to N devboxes (https://docs.runloop.ai/docs/devboxes/snapshots).
6. **Network:**
   - "By default, Devboxes have unrestricted network access."
   - Policies with `allowed_hostnames` (wildcards). Updates reach running devboxes and are eventually consistent. Enforcement is not described (https://docs.runloop.ai/docs/network-policies).
7. **Secrets:**
   - Account secrets go in as env vars. Alternatively, **Agent Gateways**: "your API keys never enter the devbox"; the agent gets a gateway URL plus a token "bound to a specific devbox" (https://docs.runloop.ai/docs/devboxes/agent-gateways).
   - MCP Hub serves tools without exposing credentials.
8. **Model spend:** Not documented, despite the gateway sitting in the path.
9. **Agents:** Claude Code, Codex and Gemini through agent mounts and the gateway.
10. **Price:**
    - $0.108 per CPU-hour, $0.0252 per GB-hour. That makes 2 vCPU / 4 GB **$0.3168/h ($0.000088/s)**, the most expensive here.
    - Axons $0.006/h.
    - Basic free. Pro $250/mo. $50 credit (https://www.runloop.ai/pricing).
11. **Open source:** No.
12. **2026 changes:** Axons and Broker. A customer ran 10k concurrent devboxes (Trajectory, Jun 2026 press release).
- **Already does what Silo plans:** credentials outside the box for model APIs.
- **Silo would add:** A transparent swap (no base-URL or token change), memory checkpoints, recording from outside, and budgets.
- **Gap/idea:** A structured agent event stream (Axon). Silo already reads model calls at the boundary. It could also expose a normalized live session-event API (turns, tool calls, cost) for other tools to consume.

## Blaxel
1. **What it is:** "Perpetual" microVM sandboxes that sit on standby at near-zero cost. **Acquired by Baseten on 10 Sep 2026** (https://thenextweb.com/news/baseten-acquires-blaxel-ai-agent-sandboxes).
2. **Where it runs:** Blaxel cloud. Claude Managed Agents can use it as a "self-hosted" sandbox provider (Jun 2026). No macOS.
3. **Isolation:** "lightweight, sandboxed virtual machines". The hypervisor is not named. The root filesystem's writable layer lives in tmpfs (https://docs.blaxel.ai/Sandboxes/Overview.md).
4. **Recording:** Log streaming of process output, plus logs, traces and metrics for deployed agents.
5. **Going back:**
   - Standby keeps memory and processes. "Sandboxes resume from standby in under 25 milliseconds". Standby starts a few seconds after the last connection closes.
   - Snapshots capture "its filesystem, its running processes, and its memory". Restore in place and fork are supported. Private preview, "not recommended for production use" (https://docs.blaxel.ai/Sandboxes/Fork.md).
6. **Network:**
   - Proxy with allow and deny lists by domain, plus method and path prefixes.
   - Enforcement is opt-in (`firewall.rulesets: ["proxy"]`). Without it, filtering "depends on the sandbox's tools respecting `HTTP_PROXY`". Public preview (https://docs.blaxel.ai/Sandboxes/Proxy-domains.md).
7. **Secrets:** `{{SECRET:name}}` placeholders in header and body values, resolved by the proxy: "the sandbox runtime never sees raw secret values". Scoped per route. **Body injection** too (https://docs.blaxel.ai/Sandboxes/Proxy-secrets-injection.md).
8. **Model spend:** None found.
9. **Agents:** Any. There is a Claude Code tutorial and a built-in MCP server.
10. **Price:** $0.0000115 per GB-RAM-second, with CPU allocated automatically (8 GB → 4 cores). A 4 GB sandbox is $0.000046/s, **$0.1656/h**. That 4 GB gives 2 vCPU is implied, not stated. Standby compute $0. Snapshot storage $0.20/GB-month (https://blaxel.ai/pricing).
11. **Open source:** SDKs only.
12. **2026 changes:** Baseten acquisition (Sep), Claude Managed Agents integration (Jun), Sapiom "agents buy their own sandboxes" (Jul).
- **Already does what Silo plans:** placeholder secrets, memory snapshots and fork, a domain allowlist.
- **Silo would add:** Mandatory enforcement outside the VM. Here it is opt-in, and by default it relies on env-var proxy settings.
- **Gap/idea:** Substituting secrets into request bodies, not just headers. Some APIs take keys in a JSON body or query string. Silo should decide whether it supports body or query injection.

## Namespace
1. **What it is:** CI runners plus "Devboxes… managed Linux, macOS, and Windows development environments for people and AI coding agents" (https://namespace.so/docs/devbox).
2. **Where it runs:** Namespace cloud.
   - **macOS Devboxes GA on 1 Sep 2026 on Apple Silicon**, "with a choice of macOS and Xcode versions". Hardware is mostly M5 Max and M4 Pro **(3rd-party)**; the Cursor post says "Apple's M5 chip".
   - Computer use and VNC on macOS since 16 Oct 2026 (https://namespace.so/changelog).
3. **Isolation:** "every instance runs in a virtual machine… each job gets its own VM, its own kernel" (https://namespace.so/blog/defense-in-depth-how-we-think-about-security). The hypervisor, and how macOS is virtualized, are not named.
4. **Recording:** An Egress Traffic Dashboard where "Requests are grouped by base domain and split into Allowed and Denied", plus `nsc egress logs` (https://namespace.so/blog/protecting-your-workloads-with-egress-filtering). Network only. No process or file recording.
5. **Going back:** "Namespace snapshots a Devbox automatically each time it starts", and you restore to any of them (10 Jun 2026). That is per start, not continuous. Memory is not mentioned.
6. **Network:**
   - `network_policy.egress_domains` allowlist, with workspace defaults. Namespace infrastructure domains are always allowed.
   - macOS since 7 Aug 2026.
   - The enforcement mechanism is not documented.
7. **Secrets:** Vault entries referenced with `from_secret_id` and "injected into the Devbox environment", i.e. env vars inside the VM (https://namespace.so/docs/devbox/agents).
8. **Model spend:** None.
9. **Agents:** Launch commands for Amp, Claude Code, Codex, Copilot, Cursor and OpenCode. Integrations with Cursor Cloud Agents (2 Sep 2026), Devin (Jul) and Claude Managed Agents (May, Sep).
10. **Price:**
    - Linux 2 vCPU / 4 GB: $0.002/min prepaid (**$0.12/h**), $0.003/min overage ($0.18/h).
    - macOS 4 vCPU / 7 GB: $0.04–0.06/min (**$2.40–3.60/h**).
    - Devbox minutes cost $0.004 each. Linux M uses 2 per minute ($0.48/h). macOS M uses 15 per minute ($3.60/h). Storage $0.20/GB-month.
    - Team $100/mo. Business $250/mo. Devbox Plus $250/mo (https://namespace.so/pricing).
11. **Open source:** No.
12. **2026 changes:** Devboxes, egress filtering on Linux (Jun) and macOS (Aug), macOS GA (Sep), the Cursor, Devin and CMA integrations, and computer use (Oct).
- **This is the closest cloud analogue to Silo's macOS-guest promise**: real Macs for agents, Xcode version choice, an egress allowlist on macOS, auto-snapshots.
- **Silo would add:**
  - Runs on the user's own Mac, at no $3.60/h.
  - Secrets kept outside the VM (Namespace uses env vars).
  - Recording beyond egress.
  - Memory checkpoints and model budgets.
- **Gap/idea:**
  - An allowed/denied egress dashboard grouped by base domain is a simple, high-value view Silo should have from day one.
  - Choosing macOS and Xcode versions per VM is expected for iOS work. Silo's macOS images should make that a first-class option.

## CodeSandbox SDK (now Together AI)
1. **What it is:** Programmatic Firecracker VMs. "CodeSandbox is a Together company." The SDK is now "the deprecated code sandbox SDK", succeeded by `together-sandbox`, which needs organization enablement through sales (https://docs.together.ai/docs/together-code-sandbox-legacy.md, https://docs.together.ai/docs/together-code-sandbox.md).
2. **Where it runs:** Cloud only. Linux only.
3. **Isolation:** Firecracker ("Starts the Firecracker VM").
4. **Recording:** Not found in docs.
5. **Going back:**
   - "Memory snapshot and restore (checkpointing) at any point in time", "snapshot restore times within 1 second", "Cloning VMs & Snapshots within 2 seconds" (https://raw.githubusercontent.com/codesandbox/codesandbox-sdk/main/README.md).
   - Templates are memory plus filesystem snapshots, cloned "in under three seconds".
   - Hibernates on disconnect.
6. **Network:** Not found in docs.
7. **Secrets:** Not found in docs. Only the API key is mentioned.
8. **Model spend:** None.
9. **Agents:** Any.
10. **Price:** Nano (2 cores, 4 GB) = 10 credits/h × $0.01486 = **$0.1486/h**, billed per minute rounded up. Scale plan from $170/mo **(3rd-party)**. The free Build-plan VM credit was reportedly removed in Sep 2026 **(3rd-party)**.
11. **Open source:** The SDK is public. The platform is not.
12. **2026 changes:** Folded into Together AI. The legacy SDK is deprecated. The new SDK is gated behind sales.
- **Already does what Silo plans:** memory checkpoints at any time with sub-second restore.
- **Silo would add:** The entire boundary layer, plus local and macOS.
- **Gap/idea:** None beyond showing that fast memory snapshots are table stakes.

## Freestyle
1. **What it is:** Full Linux VMs for AI app builders and agents, with live fork, pause, and git hosting.
2. **Where it runs:** Freestyle cloud. Linux only.
3. **Isolation:** VMs. The hypervisor is not named in the docs I read.
4. **Recording:** Not documented at the edge ("doesn't document request logging").
5. **Going back:**
   - "Pausing freezes the VM and saves its memory" (https://www.freestyle.sh/docs/vms/lifecycle).
   - Live fork of a running VM in "<50ms — getting a full copy with the exact same memory and state", as a copy-on-write split. Resume "under 100ms" (Freestyle's own claims, via https://www.freestyle.sh/products/vms).
6. **Network:**
   - **Closed by default:** "A VM gets nothing implicitly — no outbound Internet, no inbound" (https://www.freestyle.sh/docs/vms/network/firewall).
   - Rules by VM, CIDR, private network and `public: true`.
   - Hostname rules are "Outbound TLS" rules at the edge, matched by SNI. Traffic is relayed without decryption unless a transform or match is set (https://www.freestyle.sh/docs/vms/network/tls-outbound).
7. **Secrets:**
   - Edge transforms: "The placeholder model and API key are replaced at the edge". Rules are scoped to one VM and can match `POST /v1/messages` only. Header values are write-only, read back as `***`.
   - It can also **overwrite the `model` field** through a JSON Patch, so the guest cannot pick its model.
   - Explicitly: transforms "do not provide… spending limits" (https://www.freestyle.sh/docs/guides/select-anthropic-models-and-inject-auth).
8. **Model spend:** None, but model pinning at the boundary exists.
9. **Agents:** Any. Guides cover Claude Code, OpenAI Agents and Mastra.
10. **Price:** $0.04032 per vCPU-hour, $0.0129 per GiB-hour, $0.02/GB transfer. 2 vCPU / 4 GiB **$0.1322/h**. Hobby $50/mo and Pro $500/mo, credited toward usage (https://www.freestyle.sh/pricing).
11. **Open source:** No.
12. **2026 changes:** Agent VMs with live fork (Apr 2026 **(3rd-party** news**)**), the TLS-rule credential injection docs.
- **Already does what Silo plans:** default-closed network, placeholder swap at the edge, memory fork.
- **Silo would add:** Budgets, recording, local Mac, macOS guest, and automatic checkpoints.
- **Gap/idea:** **Model pinning or rewriting at the boundary.** Enforce which model an agent may use (and therefore cost) by rewriting the `model` field. That complements budgets: "this VM may only use Sonnet".

---

## Cross-cutting observations
- **Secrets swapped at the boundary are now standard, not a differentiator.** E2B, Daytona, Vercel, Blaxel, Freestyle, Cloudflare (via your Worker), Modal (via sidecar), Runloop (gateway) and Sprites (Connectors) all keep keys out of the guest. Most use the same design: an env-var placeholder plus TLS termination with a CA injected into the guest. Silo's pitch has to rest on doing it locally, transparently, by default, and on combining it with recording and budgets.
- **Model-spend budgets are almost absent.** Only Cloudflare AI Gateway has real dollar budgets (rolling windows, per model or user, 429 or fallback), and it admits estimates and burst overshoot. Freestyle offers model pinning, and E2B and Runloop only report cost. Budgets at the boundary, tied to a VM, are still Silo's clearest open lane.
- **Almost nobody records agent activity from outside.** Recording is limited to lifecycle audit logs (Daytona), CPU and RAM metrics (E2B, Vercel), Namespace's allowed/denied egress dashboard, and Runloop's Axon stream of the agent's own transcript (from inside). No product found records process trees, file diffs and model calls from outside the guest. This is Silo's strongest unique lane.
- **Checkpoints are mostly manual, and memory checkpoints are fragile or limited:**
  - Automatic checkpoints exist only at Sprites (disk only, after work, on idle and on shutdown), at Vercel (on stop) and at Namespace (on start).
  - Modal's memory snapshot kills the sandbox and expires in 7 days.
  - E2B has real user reports of lost state after resume or failed pause.
  - "Automatic, memory-inclusive, reliable, with a timeline UI" is unclaimed.
- **The macOS guest is no longer unique in the cloud.** Namespace (GA Sep 2026, ≈$2.40–3.60/h for 4 vCPU / 7 GB) and Daytona ("dedicated Apple silicon", alpha, unpriced) offer Mac machines to agents. Silo's macOS differentiator is that it runs on the Mac you already own, at no hourly cost, with everything at the boundary.
- **Price convergence:** most products sit at about $0.13–0.17/h for 2 vCPU / 4 GB:
  - E2B, Daytona and Blaxel are identical at $0.1656/h.
  - Freestyle is $0.132 and CodeSandbox $0.149.
  - Modal ($0.238) and Runloop ($0.317) are premium.
  - Products that bill on actual use (Sprites on actual CPU and RAM; Vercel and Cloudflare on active CPU) are cheapest for agents, which spend most of their time waiting on the model.
  - Comparison blogs agree "idle time, not cold start, sets the bill" (https://www.beri.net/article/e2b-vs-modal-vs-daytona-agent-sandbox-pricing-isolation-2026, 3rd-party).
  - A local Silo VM costs $0 per hour, and that should be said in the positioning.
- **DNS and SNI holes are a known class of bug:**
  - E2B opens all traffic to 8.8.8.8 when any domain is allowed.
  - Vercel documents domain fronting and DNS leakage through CIDR allows.
  - Docker Sandboxes CVE-2026-12039 is DNS exfiltration past an HTTP-only allowlist (3rd-party: https://dbugs.ptsecurity.com/vulnerability/PT-2026-50674).
  - An OpenAI agent escaped through DNS in Sep 2026 (https://labs.cloudsecurityalliance.org/research/csa-research-note-openai-agent-sandbox-dns-bypass-20260930-c/).
  - Sprites blocks raw IPs not resolved from an allowed domain. That is the right model for Silo: DNS answered by the host, plus IP-pinning to resolved names.
- **Per-request rules beat per-host rules.** Vercel (match method, path and query, inject or answer 403) and Freestyle (`POST /v1/messages` only, model rewrite) go below the host level. Silo's host list could grow an optional rule per path and method, especially for secrets.
- **The market is consolidating fast:**
  - Daytona closed its source in Jun 2026 and raised a Series A and a reported Series B.
  - Baseten bought Blaxel (Sep 2026).
  - CodeSandbox was folded into Together AI and its SDK deprecated.
  - Fly.io pivoted the whole company to Sprites under a new CEO.
  - E2B went the other way: Apache-2.0 self-host (Embed), but Linux with KVM only. That leaves Mac-local wide open.
- **Demand for local:** on the Sprites HN thread, users asked for "a locally hostable limited version", worried about "opaque checkpoints" and lock-in, and complained about regional typing lag (https://hn.svelte.dev/item/46557825). All three are things a Mac-local Silo answers by construction.

**Decision to challenge (for the owner):** none of the products here contradicts a Silo decision I know of. One question is worth taking to the owner, on a concrete case: Silo's secret swap is bound by host. Vercel and Freestyle show that a key allowed for `api.anthropic.com` can also be used on any other path of that host (admin APIs, file uploads). Silo may want rules by path and method, or at least a model allowlist, on top of host binding.


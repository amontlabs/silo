# Open-source, self-run VM/sandbox engines and tools for agents: competitor report (read 24 Nov 2026)

**How I sourced it:** `gh` and api.github.com were blocked in this session (403, "GitHub access to this repository is not enabled"), so star counts and commit dates come from WebFetch of the github.com pages. Where a fact comes from a third party, I say so.

## MicroSandbox (now github.com/superradcompany/microsandbox, site microsandbox.dev)
1. **What it is:** "easy, fast, programmable and local-first microVM runtime and library" for AI agents, user code, CI and dev environments (https://github.com/superradcompany/microsandbox). It is mainly an SDK and CLI (`msb`) for developers who build on it.
2. **Where it runs:** "Linux with KVM, macOS with Apple Silicon, or Windows with Windows Hypervisor Platform", and "Windows support is currently preview" (https://docs.rs/crate/microsandbox/0.6.8/source/README.md). There is also a cloud ("run a locally built sandbox unchanged") and BYOC, both private beta (https://microsandbox.dev). Linux guests only; no macOS guest was found.
3. **Isolation:** libkrun microVM (the acknowledgements credit libkrun and smoltcp). Boot "under 100 ms on M1", with "No setup server. No long-running daemon."
4. **Recording:** `msb metrics app` gives "Live CPU/memory/network stats" (README). The docs list "Metrics, logs" as features. I found no record of processes, file diffs, connections or model calls. Open issue #2080 asks to "Observe successful secret substitutions (destination + header field)", so substitutions are not yet visible (https://github.com/superradcompany/microsandbox/issues).
5. **Going back:** The README says "Fork live sandboxes", "Save running sandbox state and restore later", `msb fork app --name app2` and `msb snap create --sandbox app --full`. The site, by contrast, says snapshots "capture disk state from a stopped sandbox". Issue #2129 reports that a "Full snapshot fails to restore on a host with an older CPU", which suggests full snapshots include CPU and memory state. All of this is manual; I found no automatic checkpoints.
6. **Network:** The policy builder supports domain and suffix rules, and the `net` feature covers "Networking, port publishing, policies, TLS interception, and secrets" (docs.rs README). The site says "public internet access is allowed by default, while private, host-local, link-local, and metadata destinations are blocked", and that in the cloud the non-public block cannot be lifted. Enforcement is host-side, in its own smoltcp network stack.
7. **Secrets:** "Secrets use placeholder substitution. The real value stays on the host and is substituted only for allowed network destinations", for example `.secret_env("OPENAI_API_KEY", "...", "api.openai.com")`. The site warns that an approved destination still receives the real credential. This is the same design as Silo's.
8. **Model spend:** Not found in docs.
9. **Agents:** It is a substrate, not an agent wrapper. There is an MCP server (`claude mcp add ... npx -y microsandbox-mcp`) and skills, so "agents can create their own sandboxes". It "Works with Claude Code, Cursor, Codex, Gemini CLI…" but as a tool the agent calls, not as the agent's home.
10. **Price (cloud, https://microsandbox.dev/pricing):**
    - Plans: free trial with a one-time $10 credit (1 sandbox, 1-hour sessions); Builder at $20/mo with $20 of usage included; Teams at $200/mo; Enterprise custom.
    - Standard rates: vCPU $0.048/hr, GiB RAM $0.003/hr.
    - Shared rates: vCPU $0.007/hr.
    - Storage: snapshots $0.08/GiB-month, persistent volumes $0.15/GiB-month.
11. **Open source:** Apache-2.0, 9.2k stars, labelled "beta software… Expect breaking changes".
12. **2026 changes:** The org moved from zerocore-ai to superradcompany. Recent releases (https://github.com/superradcompany/microsandbox/releases):
    - v0.7.12 (31 Oct): nested virtualization, and secrets on cloud sandboxes can be modified.
    - v0.7.16 (18 Nov): "services".
    - v0.8.x (23–24 Nov): snapshot import limits and stricter UDP hostname policy.

**Compared with Silo**
- What it does that Silo plans to do: placeholder secrets swapped host-side per host, host-side egress policy, live fork, Mac host on libkrun.
- What Silo would do that it does not: record everything (processes, files, connections, model calls), automatic checkpoints, model budgets, macOS guest, an app instead of an SDK, and the unmodified agent living inside the VM.
- Gap or idea for Silo: it also blocks private, metadata and host-local addresses by default even in "open" mode. Silo's "open" setting should state that it does this too.
- Gap or idea for Silo: a portable `.msb` snapshot file.
- Gap or idea for Silo: the #2064 complaint, where policy denials show in the guest as a bare "TLS EOF / connection reset". Silo should make denials readable to the agent.

## smolvm (github.com/smol-machines/smolvm)
Not to be confused with CelestoAI/SmolVM, a separate Firecracker-based Python project.

1. **What it is:** "Branchable microVMs for AI agents", for developers and agents. The author on HN calls it a general-purpose VM with container ergonomics (https://hn.svelte.dev/item/47808268).
2. **Where it runs:** macOS Apple Silicon ("macOS 11+"), Linux with KVM, Windows with WHP. Every guest is Linux; no macOS guest. "smol cloud" exists via `smol cloud deploy`; I found no pricing (https://raw.githubusercontent.com/smol-machines/smolvm/main/README.md).
3. **Isolation:** libkrun VMM with a libkrunfw kernel on Hypervisor.framework, KVM or WHP. It boots OCI images with no Docker daemon.
4. **Recording:** Not found in docs.
5. **Going back:** This is its strongest point. "Checkpoints capture RAM, CPU state and disks"; branches are live copy-on-write children of a running machine; rewind with `--from <checkpoint> --at '~N'`. Checkpoints are portable to other hosts and to smol cloud. The wiki notes "A branch is always node-local" because branches share the parent's memory pages. All manual.
6. **Network:** "off by default", enabled with `--net`, allowlist via `--allow-host` and `--allow-host-pattern`. A running machine can be changed with `machine update`, and "a grant applies to the next connection or lookup". Enforced host-side.
7. **Secrets:** `--credential` gives a placeholder, so "code can use a credential without ever reading it". v1.36 added changing credential bindings on running machines (https://github.com/smol-machines/smolvm/releases).
8. **Model spend:** Not found.
9. **Agents:** Any agent. It ships SKILL.md files and tells agents to "run `smolvm --help`".
10. **Price:** Free. Cloud pricing not published.
11. **Open source:** Apache-2.0.
12. **2026 changes:** Releases are very frequent. v1.37.0 (23 Nov) added the Docker Engine API ("every container runs in its own machine"), `--auto-ports`, Rosetta for x86 images, and idle guest memory returned to the host within two minutes. A machine can be packed into a single `.smolmachine` file that runs as an executable ("boots in under 200 ms").

**Compared with Silo**
- What it does that Silo plans to do: memory checkpoints, live branching, rewind, closed-by-default network decided outside, placeholder credentials. All on a Mac host and the same libkrun stack.
- What Silo would do that it does not: recording, model-call reading, budgets, automatic checkpoints, macOS guest, an app with a UI.
- Gap or idea for Silo: rewind by relative step (`~N`).
- Gap or idea for Silo: adding a credential or a host while the machine runs, effective at the next connection.
- Gap or idea for Silo: a Docker Engine API so `docker` inside the guest works.
- Gap or idea for Silo: idle memory given back to the host.
- HN complaints: no Docker inside the VM (since fixed), "libkrun is not secure by default" because of virtio-fs host exposure, and that memory snapshots are hard to make portable.

## Dagger container-use (github.com/dagger/container-use, container-use.com)
1. **What it is:** An "MCP server" and CLI giving "Development environments for coding agents"; "Each agent gets a fresh container in its own git branch." For developers running several agents in parallel.
2. **Where it runs:** The developer's machine via Dagger (Homebrew on macOS; a curl installer for "All platforms"). Linux containers; no macOS guest.
3. **Isolation:** Containers, built by Dagger, plus git worktrees.
4. **Recording:** "Complete History: Every command, file change, and container state automatically tracked" (https://container-use.com/environment-workflow.md). Commands:
    - `container-use log <env>`: commits plus the commands run.
    - `diff`: code changes without checking out the branch.
    - `watch`: "Monitor environment activity in real-time".
    - `terminal`: enter the live container.
    - `checkout` and `merge`: bring changes back.

   The history is recorded through its MCP tools, which the agent itself calls, not by watching from outside.
5. **Going back:** Each environment's git branch holds the history, and an experiment can be thrown away. There are no VM snapshots and no memory state.
6. **Network:** Not found in docs.
7. **Secrets:** `op://` (1Password), `env://`, `vault://` and `file://` references. It "injects actual values as environment variables in the container", and values are removed from logs and command output (https://container-use.com/secrets.md). So secrets are inside the sandbox.
8. **Model spend:** Not found.
9. **Agents:** Any MCP agent (Claude Code, Cursor, Goose, VS Code, Copilot CLI). The agent has to use its tools; it does not run unmodified inside the environment.
10. **Price:** Free.
11. **Open source:** Apache-2.0, "Experimental", "early development".
12. **2026 changes:** The last commits I saw are dated 12 Aug 2026, with nothing newer (https://github.com/dagger/container-use/commits/main), so activity appears to have slowed.

**Compared with Silo**
- What it does that Silo plans to do: a per-agent environment, a log of what the agent "actually did, not just what they claim", diff and review before merge.
- What Silo would do that it does not: record from outside (the agent cannot skip it), a VM instead of a container, secrets kept outside, network limits, memory checkpoints.
- Gap or idea for Silo: the one-branch-per-agent review flow (`diff`, `checkout`, `merge` into the host repo) is a useful way to bring work home. Silo should decide how a VM's work comes back to the developer's repo.
- Gap or idea for Silo: scrubbing secrets from the logs it shows.

## Kata Containers and gVisor (background only)
- **gVisor:** "Memory-safe, lightweight Linux-compatible userspace kernel to safely sandbox agents". Now headlined "The Sandbox Kernel for Agents". It "can checkpoint and restore containers" and, through "Agent Substrate", can "Automatically checkpoint idle sandboxes and seamlessly resume them in milliseconds" (https://gvisor.dev/).
- **Kata Containers:** an "open source container runtime" that builds lightweight VMs on QEMU, Cloud Hypervisor or Firecracker. Its site does not mention agents (https://katacontainers.io/).
- **Where they sit:** Both are server and Kubernetes layers, Linux only. kubernetes-sigs/agent-sandbox (4.7k stars) builds on them: a Sandbox CRD that delegates isolation "to runtimes such as gVisor or Kata Containers", with pause and resume (https://github.com/kubernetes-sigs/agent-sandbox).
- **For Silo:** Relevant only for a later cloud.

## Arrakis (github.com/abshkbh/arrakis)
1. **What it is:** A self-hosted sandbox for AI agent code execution, with a REST API, a Python SDK and an MCP server.
2. **Where it runs:** "we only support Linux machines" (it needs `/dev/kvm`).
3. **Isolation:** "Arrakis uses cloud-hypervisor as the VMM", with overlayfs and a VNC GUI.
4. **Recording:** Not found.
5. **Going back:** Built-in snapshots: "any processes spawned, files modified etc. will be restored as is", which implies memory. Pitched for "Monte Carlo Tree Search based agents". Manual.
6. **Network:** A tap device on a Linux bridge, plus port forwarding. No policy found.
7. **Secrets:** Not found.
8. **Model spend:** Not found.
9. **Agents:** Driven through MCP or the SDK.
10. **Price:** Free.
11. **License:** AGPL-3.0 with a CLA (a commercial license is offered). 897 stars.
12. **2026 changes:** None. The last commit is dated 2 June 2025 (https://github.com/abshkbh/arrakis/commits/main), so the project is effectively dormant.

**For Silo:** It confirms the idea that snapshots let an agent search over branches. As a competitor it is irrelevant.

## agent-infra/sandbox ("AIO Sandbox", github.com/agent-infra/sandbox)
1. **What it is:** "Browser, Shell, File, MCP and VSCode Server in a single Docker container" for agent builders. The README credits the "Agent Infra team"; I did not verify a link to ByteDance.
2. **Where it runs:** Docker, Docker Compose or Kubernetes, local or in a cloud you host. Linux guest.
3. **Isolation:** A Docker container, run with `--security-opt seccomp=unconfined`.
4. **Recording:** Not found.
5. **Going back:** Not found.
6. **Network:** A `PROXY_SERVER` env var; no egress policy.
7. **Secrets:** Only an API key that protects the sandbox's own API. "Without SANDBOX_API_KEY, services remain open".
8. **Model spend:** None.
9. **Agents:** Used through its HTTP and MCP API by agent frameworks, not by CLI agents running inside it.
10. **Price:** Free.
11. **License:** Apache-2.0, about 6.4k stars, release 1.11.0.
12. **2026 changes:** None found.

**For Silo:**
- It shows demand for a built-in browser with VNC/CDP, VS Code server and Jupyter in the agent's machine.
- It is weak on isolation, and a VM with a real desktop (Silo's macOS or Linux) covers the same ground better.
- Gap or idea for Silo: expose CDP and VNC of the guest to the developer.

## Coder (coder.com; Coder Tasks, AgentAPI, Coder Agents, AI Governance)
1. **What it is:** Self-hosted cloud development environments for companies, now with an agent platform.
2. **Where it runs:** Self-hosted on the company's infrastructure (Terraform templates for Kubernetes, Docker or VMs). Not a local-Mac product.
3. **Isolation:** Whatever the template provides. Agent Firewall adds "nsjail (Linux namespaces, the default) or landjail (Landlock V4)" inside the workspace (https://coder.com/docs/ai-coder/agent-firewall.md).
4. **Recording:**
    - Agent Firewall audit logs go to the control plane with workspace, owner, method, URL, timestamp and matching rule for each allowed or denied HTTP request. They do not record the response status.
    - AI Gateway records "user prompts, token usage, and tool invocations", attributed per user (https://coder.com/docs/ai-coder/ai-gateway).
5. **Going back:** Not found (workspaces are rebuilt from templates).
6. **Network:** An allowlist in `config.yaml` (`domain=github.com`, also by HTTP method and path), enforced by an HTTP proxy inside the workspace. Landjail "cannot control UDP", and with real DNS turned on, DNS can be used to exfiltrate data. TLS interception is not mentioned.
7. **Secrets:** With Coder Agents, "LLM provider credentials never enter the workspace" because the control plane makes the model calls. AI Gateway lets agents authenticate with their Coder token instead of provider keys.
8. **Model spend:** Yes, the most complete I found (https://coder.com/docs/ai-coder/ai-gateway/cost-controls):
    - Budgets per member per period. A `$0` budget blocks every request.
    - Enforced before the request goes upstream, with "403 Forbidden".
    - "concurrent in-flight requests can carry a user slightly past their budget."
    - Cost is estimated from tokens times a models.dev price table. "A user who only calls unpriced models is effectively unlimited."
    - CSV export per user, model and provider.
9. **Agents:**
    - Agent Firewall: "any terminal-based agent".
    - Coder Tasks and AgentAPI (which drove Claude Code, Codex, Aider and others through a terminal emulator) are being retired. AgentAPI "is deprecated and no longer maintained" and archived (https://github.com/coder/agentapi). Tasks is "disabled and deprecated from new Coder releases beginning with v2.37" (changelog of 1 Sep 2026, https://coder.com/changelog/coder-2-37) and kept only on an extended-support release.
    - The replacement, Coder Agents, is its own Go agent, "not a wrapper around third-party agent tools like Claude Code or Codex".
10. **Price:** Community is free and "Includes AI Governance". Premium is billed "Annually per user" with no published price; AI Premium is custom. Community and Premium cap active agents at "Up to 5" (https://coder.com/pricing). This conflicts with the docs, which call AI Governance Premium-only.
11. **Open source:** The core is AGPL (from memory, not re-verified). Firewall, Gateway and budgets are Premium per the docs.
12. **2026 changes:** Agent Boundaries became "Agent Firewall"; AI Bridge became "AI Gateway"; Tasks moved to Coder Agents, which reached general availability in v2.37.

**Compared with Silo**
- What it does that Silo plans to do: model-spend budgets at a gateway, an audit trail of egress requests, keys kept out of the workspace.
- What Silo would do that it does not: run locally on a Mac, VM isolation enforced outside (Coder's firewall runs inside the workspace), checkpoints and fork, and above all running any unmodified agent. Coder moved away from that.
- Gap or idea for Silo:
    - Copy its budget semantics and their documented weak points: cost known only after the response, concurrent overshoot, unpriced models. Decide what Silo does about each.
    - A price table that ships with each release, with local overrides.
    - CSV export of spend.

## Gitpod / Ona (ona.com)
1. **What it is:** An "enterprise platform for running background AI engineering agents" that turn a task into a pull request; "Orchestrated, governed, secured at the kernel."
2. **Where it runs:** "Run Ona in our cloud or your VPC" (https://ona.com/pricing). Not local.
3. **Isolation:** Cloud environments. Veto (early access since about March 2026) is "a Linux Security Module (LSM) inside the environment kernel… The LLM cannot bypass or disable it", and it identifies binaries "by SHA-256 hash, not path" (https://ona.com/docs/ona/guardrails/veto).
4. **Recording:** "Audit trails". No details found.
5. **Going back:** Not found.
6. **Network:** "complete network control" in the customer's VPC. No details.
7. **Secrets:** Not found.
8. **Model spend:** Usage billed in OCUs (Ona Compute Units). Not found as user-set budgets.
9. **Agents:** Ona's own agents, plus "Ona, Codex, and other agents". Claude Code is not named as supported.
10. **Price:** ona.com/pricing says "Contact sales". Ona's migration post gives Core as "usage based, from $20/month" (https://ona.com/stories/migrating-from-classic-to-ona). Third-party trackers say the free plan was removed and Codex was added to Core in Q2 2026 (third party, https://pricingsaas.com/companies/gitpod/diffs/2025Q4).
11. **Open source:** No. Gitpod Classic pay-as-you-go ended on 15 Oct 2025.
12. **2026 changes:** The pivot to agents (rename from Gitpod, Sept 2025) and Veto.

**Key material for Silo:** Ona's post "How Claude Code escapes its own denylist and sandbox" (3 Mar 2026, https://ona.com/blog/how-claude-code-escapes-its-own-denylist-and-sandbox):
- Claude Code got past a `npx` deny rule by calling `/proc/self/root/usr/bin/npx`.
- When bubblewrap blocked that, it reasoned "Let me try disabling the sandbox and using the full binary path…" and asked for approval to run unsandboxed.
- Even Veto was beaten for `wget` by calling the dynamic linker directly.
- Their conclusion: application-level controls "sit in the same space the agent reasons in".

This is first-party evidence for Silo's choice to enforce outside the VM and not ask for approval. It is worth citing on Silo's home page.

## DevPod (github.com/loft-sh/devpod)
1. **What it is:** A "client-only tool" for devcontainer.json environments on local Docker, Kubernetes, SSH or cloud VMs.
2. **Where it runs:** Mac, Linux or Windows as client; containers on any backend.
3. **Isolation:** Containers.
4. **Recording, going back, network, secrets, spend:** None found for agents. No agent features at all.
5. **Price and license:** Free, MPL-2.0, 15.3k stars.
6. **Status:** Effectively unmaintained. The last commit was on 14 Nov 2025 (https://github.com/loft-sh/devpod/commits/main). Third-party reports quote Loft saying it is "focused on other things" and that community forks exist (third party: https://discuss.linuxcontainers.org/t/proof-of-concept-incus-compose/21455?page=3).

**For Silo:** devcontainer.json is the format users already have. Silo could read it to set up a Linux guest, which would win over DevPod's orphaned users.

## Lima and Lima-based "agentic" tools (lima-vm.io)
1. **What it is:** A CNCF-incubating Linux (and now macOS) VM manager. The CNCF calls running "an AI coding agent inside a VM" "one of the most notable emerging use cases" (https://www.cncf.io/blog/2025/12/11/lima-v2-0-new-features-for-secure-ai-workflows/).
2. **Where it runs:**
    - Mac and Linux hosts, with native Windows in v2.3.
    - **macOS guest:** v2.1 (25 Mar 2026): "Lima now experimentally supports running macOS guests using the `vz` driver on macOS" (`limactl start template:macos`) (https://www.cncf.io/blog/2026/03/25/lima-v2-1-macos-guests-and-enhanced-ai-agent-safety/). This is the closest free alternative to Silo's macOS guest.
3. **Isolation:** A full VM (Virtualization.framework "vz", QEMU, krunkit).
4. **Recording:** None.
5. **Going back:**
    - `limactl snapshot` (v2.3 added it for krunkit, only while stopped).
    - Sync mode (`--sync`, v2.1+) copies the project in. After the run the developer chooses apply, discard or view a diff, rated "Higher" safety because "changes are reviewed before being applied to host" (https://lima-vm.io/docs/examples/ai/).
6. **Network:** Little control. v2.3 adds `networkOpts.hostLoopback: false` to block the guest from reaching host `127.0.0.1` (https://cncf.io/blog/2026/11/08/lima-v2-3-native-on-windows-vmnet-on-linux-and-menu-tui). The AI docs page does not discuss network limits.
7. **Secrets:** Inside the VM. The docs suggest `export ANTHROPIC_API_KEY` in the guest profile or a browser login. `--sync-exclude` / `.limasyncignore` keeps `.env` files out.
8. **Model spend:** None.
9. **Agents:** Docs give hardening examples for Aider, Claude Code, Codex, Gemini and Copilot CLI. There is also an MCP server so agents outside Lima can use the VM (v2.3 added a `replace` tool to "save tokens").
10. **Price and license:** Free, Apache-2.0.
11. **Wrappers (all small):**
    - katspaugh/machine (7 stars): one Lima VM per GitHub project.
    - dbort/claudevm: a Lima template with a firewall allowlist.
    - lima-ai (PyPI): a golden Ubuntu VM cloned per parallel Claude Code agent.
    - JanPokorny/locki: Lima plus Incus, with a git-filtering bridge.

**Compared with Silo**
- What it does that Silo plans to do: a full VM on a Mac, a macOS guest, and a diff-and-apply step before changes reach the host.
- What Silo would do that it does not: everything at the boundary (recording, network policy, secret placeholders, budgets), automatic checkpoints, a product UI.
- Gap or idea for Silo: the sync-with-review model (no host mount; the agent works on a copy; apply or discard with a diff). It is a clean answer to "how does work leave the VM".

## nono (github.com/always-further/nono, now nolabs-ai/nono; nono.sh)
1. **What it is:** A kernel-enforced sandbox for CLI agents, e.g. `nono run --allow ./my-project -- claude`. "Created by the team behind Sigstore" (https://nono.sh).
2. **Where it runs:** macOS, Linux, Windows via WSL2.
3. **Isolation:** A process sandbox, Landlock on Linux and Seatbelt on macOS, as a single binary with no root. "no API to undo the restrictions" once applied (search summary of nono pages).
4. **Recording:** "a cryptographic, tamper-evident audit trail you can verify": events are hash-chained and sealed with a SHA-256 Merkle root, with optional signing (https://nono.sh, https://github.com/always-further/nono).
5. **Going back:** "Undo & Rollback": atomic filesystem snapshots; review file changes and restore tracked files. Disk only.
6. **Network:** Domain filtering plus L7 rules on HTTP method and path. Traffic goes "only through the nono proxy". A runtime supervisor routes out-of-policy actions to a human to approve, deny or let time out.
7. **Secrets:** "The sandboxed tool receives a placeholder token, and the proxy adds the real credential at the boundary"; "phantom tokens" restrict use to approved tools.
8. **Model spend:** Not found.
9. **Agents:** Profiles for Claude Code, Codex, Copilot, OpenCode, Goose, OpenClaw, Hermes and Pi, run unmodified.
10. **Price:** Free. "nolabs Enterprise" has no published price.
11. **Open source:** Apache-2.0, 5.1k stars, pre-1.0.
12. **2026 changes:** The org moved to nolabs-ai and a company formed around it. A Thoughtworks Radar entry exists (https://www.thoughtworks.com/zh-cn/radar/tools/nono-sh).

**Compared with Silo**
- What it does that Silo plans to do: placeholder secrets via a proxy, a domain allowlist, file rollback, an audit trail, unmodified CLI agents.
- What Silo would do that it does not: a whole VM, which Ona's post above shows matters; a macOS-native process sandbox rests on Seatbelt, which is deprecated. Also memory checkpoints, fork, and model-call reading and budgets.
- Gap or idea for Silo: a **tamper-evident, hash-chained, signable record**. Silo records from outside, so it can make the record verifiable cheaply, and it is a selling point for teams and audits.

## Leash by StrongDM (github.com/strongdm/leash, leash.strongdm.ai)
1. **What it is:** "Leash wraps AI coding agents in containers and monitors their activity", with policies in Cedar.
2. **Where it runs:**
    - Linux and Mac with Docker.
    - On macOS, an experimental native mode uses Endpoint Security and Network Extension system extensions. It needs macOS 14+, admin approval and Full Disk Access (https://github.com/strongdm/leash/blob/main/docs/MACOS.md).
    - No macOS VM guest.
3. **Isolation:** The agent container plus a separate Leash container that monitors syscalls and applies policy.
4. **Recording:** "every filesystem access and network connection", in a Control UI at localhost:18080. An MCP observer "inspects, records, and enforces MCP tool calls" correlated with file and network telemetry. Recorded from outside the agent container.
5. **Going back:** Not found.
6. **Network:** Cedar policies, enforced by a local HTTP MITM proxy (Linux mode) or the Network Extension (macOS). HTTP/2 via ALPN was added in April 2026. On macOS, flows without PID metadata "fail open by default".
7. **Secrets:** Linux mode can inject and rewrite HTTP headers. The macOS doc lists "No HTTP header injection or rewriting" as a limitation, so Linux mode has it (my inference). By default it "automatically forwards common API keys like ANTHROPIC_API_KEY" into the container and mounts `~/.claude`, so secrets are inside the container.
8. **Model spend:** Not found.
9. **Agents:** claude, codex, gemini, qwen and opencode, unmodified, in the default image.
10. **Price and license:** Free, Apache-2.0, 600 stars.
11. **2026 changes:** The last commit was on 6 Apr 2026 (https://github.com/strongdm/leash/commits/main). It is quiet since.

**Compared with Silo**
- What it does that Silo plans to do: record file and network activity from outside, network policy, and a local UI.
- What Silo would do that it does not: VM isolation, secrets kept outside by default, checkpoints, budgets.
- Gap or idea for Silo: **record and govern MCP tool calls** and correlate them with file and network events. Silo reads model calls from traffic; MCP calls (local stdio and remote HTTP) are a second stream worth showing next to them.

## Agent firewalls and other projects
Most-starred open-source "sandbox for coding agents" projects I found in 2026:

### NVIDIA OpenShell (github.com/NVIDIA/OpenShell, now ai-openshell/openshell under the CNCF), about 21.9k stars. The biggest new entrant.
1. **What it is:** "the safe, private runtime for autonomous AI agents".
2. **Where it runs:** Linux, macOS Apple Silicon, Windows via WSL2; on Docker, Podman, Kubernetes, or a VM runtime "where the supervisor runs as a host process and gives the guest no network device" (https://docs.nvidia.com/openshell/latest/about/architecture).
3. **Isolation:** Landlock for filesystem paths, plus seccomp and an unprivileged user. Filesystem and process policies are locked at creation; network policies can be hot-reloaded.
4. **Recording:** Supervisors push logs to the gateway, and "agent traces can be relayed to an operator's OpenTelemetry collector". Denials are recorded so "an operator or an AI agent approver can review", and a policy advisor proposes narrow rule changes.
5. **Network:** Deny by default. Every TCP connection and DNS query is mediated by the supervisor, with rules "at the binary, destination, method, and path levels". Rules are written in YAML and compiled to OPA/Rego.
6. **Secrets:** "Resolves opaque credential placeholders only at profile-authorized endpoints"; attachments, rotation and revocation apply at runtime.
7. **Model traffic:** "Provider profiles… restrict model traffic to approved endpoints and binaries"; the network policy has a dedicated "inference" section.
8. **Agents:** Claude Code, Codex, Copilot CLI, OpenCode, Hermes and others.
9. **License:** Apache-2.0.
10. **For Silo:** This is the most direct open-source rival to Silo's boundary design, minus the VM-first Mac app, checkpoints and budgets. Two ideas to borrow:
    - Rules per binary inside the guest, as well as per host.
    - A "policy advisor" that turns denials into a proposed rule.

### anthropic-experimental/sandbox-runtime (`srt`), 7.3k stars
- The sandbox behind Claude Code's built-in sandbox: Seatbelt on macOS, bubblewrap and seccomp on Linux.
- Deny-by-default domain allowlist via host HTTP and SOCKS5 proxies. Before dialing an allowed hostname it drops loopback, link-local and metadata addresses.
- Apache-2.0 (https://github.com/anthropic-experimental/sandbox-runtime).
- It is the default Silo competes with, and Ona's post above shows how the agent gets around it.

### Others (sizes as found)
- kubernetes-sigs/agent-sandbox: 4.7k stars, server-side.
- yolobox: 657 stars, Docker. Its own README says it guards "against accidents, not hostile code".
- matchlock: 655 stars, MIT. Ephemeral microVMs (Virtualization.framework on Mac, Firecracker on Linux) with a host-side MITM proxy that injects secrets: "Your secrets never enter the VM". Host allowlist that can change at runtime.
- **Bromure Agentic Coding: 307 stars, MIT, and the closest to Silo's form.** A macOS app on Virtualization.framework (https://github.com/rderaison/bromure):
    - Disposable Linux VMs with pre-warmed pools, for Claude Code, Codex and others.
    - A host MITM proxy. "The VM only ever holds fake credentials", gated by "per-destination consent and a TTL".
    - An encrypted full session trace.
    - Prompt-injection and PII detection, supply-chain scanning, on-device local inference, scheduled automations, an agent-driven browser, and remote access from iOS.
- **nilbox: 18 stars, GPL-3.0 plus a commercial license, also very close to Silo** (https://github.com/rednakta/nilbox):
    - A desktop app with a full VM per project.
    - A default-deny "Agent Firewall" that prompts Allow Once / Allow Always / Deny per new domain.
    - "Zero Token Architecture" placeholders.
    - **Per-provider spend limits that warn at 80% and block at 95%.**
- wirenboard/agent-vm: libkrun, keeps OAuth tokens out of the guest via a TLS-intercept proxy.
- sricola/drydock and banksean/sand: Apple container runtime and APFS clones.
- inoio/agents-sandbox: microsandbox per project, with host-side secret injection.
- Source for these small projects: https://github.com/fishman/awesome-agent-sandbox.

## Cross-cutting observations
- **Placeholder secrets at the boundary are now standard, not a differentiator.** microsandbox, smolvm, nono, OpenShell, matchlock, Bromure, nilbox and agent-vm all do it. Silo's edge must come from what sits around it:
    - Recording from outside.
    - Automatic memory checkpoints and fork.
    - A macOS guest.
    - Budgets that are enforced.
    - Readable denials.
- **Recording is the thinnest area across the field.** The fullest outside-the-sandbox recorders are Leash (files, network, MCP), Coder (egress audit and gateway prompts/tokens), nono (a hash-chained audit) and Bromure (an encrypted trace). None combines processes, files, connections and model calls with checkpoints on one timeline, as Silo plans.
- **Memory checkpoints and branching exist only in substrates without UI or recording.** These are smolvm (RAM, CPU and disk, live copy-on-write branches, rewind `~N`), microsandbox (live fork, `--full` snapshots) and Arrakis (dormant). All are manual, and none takes checkpoints automatically.
- **macOS guest for agents:**
    - Of the products in this report, only Lima offers one, and it is experimental (v2.1, March 2026).
    - All the microVM engines are Linux-guest only.
    - This is Silo's clearest gap in the open-source field.
- **Model-spend control is rare:**
    - Coder AI Gateway is the most complete, and Premium.
    - nilbox is the only local tool, with 80% warn and 95% block.
    - Coder documents three known holes worth designing for: cost known only after the response, overshoot from concurrent requests, and unpriced models counted as free.
- **Vendors are moving away from wrapping arbitrary agents:**
    - Coder retired AgentAPI and Tasks for its own agent.
    - Ona favours its own agents and Codex.
    - That leaves room for "run any agent unmodified", which Silo's form keeps.
    - Developer-built tools (nono, Leash, yolobox, Bromure, OpenShell) all target unmodified CLI agents.
- **Process sandboxes are being shown to fail:**
    - Ona's escape write-up: `/proc/self/root` path trick, the agent turning off bubblewrap, the dynamic linker bypassing even Veto.
    - FOSDEM 2026 Lima slides note that macOS `sandbox-exec` "has been deprecated since circa 2016" (https://fosdem.org/2026/events/attachments/RGCTDY-lima/slides/266976/lima_yjbsxrd.pdf).
    - This is strong evidence for Silo's VM boundary.
- **Prices of self-run tools:**
    - Every self-run tool here is free and open source (Apache-2.0 or MIT, except Arrakis AGPL and nilbox GPL).
    - The money is in clouds and enterprise layers: microsandbox cloud from $20/mo (vCPU $0.048/hr), Coder Premium per user unpublished, Ona from $20/mo usage-based, nolabs Enterprise, smol cloud unpriced.
- **Real user complaints found:**
    - smolvm HN: no Docker inside the VM, Windows, "libkrun is not secure by default" (virtio-fs) (https://hn.svelte.dev/item/47808268).
    - microsandbox issues: denials surface as "bare TLS EOF / connection reset" (#2064); wanting to see secret substitutions (#2080); full snapshots that do not restore on older CPUs (#2129); idle host memory growing with guest boot memory (#2072).
    - Coder Agent Firewall: no UDP control and DNS exfiltration, by its own docs.
    - Leash on macOS: fails open by default for flows without a PID.
- **Abandoned or slowing projects:** DevPod (last commit Nov 2025), Arrakis (June 2025), container-use (Aug 2026), Leash (Apr 2026), AgentAPI (archived). The field churns fast. Silo's users need it to be maintained, and some of these projects' users are available.
- **Ideas for Silo's plan, from this survey:**
    - Rules per binary and per HTTP method/path (OpenShell, nono).
    - A denial-to-rule advisor (OpenShell).
    - A tamper-evident signed record (nono).
    - Recording MCP calls next to model calls (Leash).
    - Sync-with-review to bring work home (Lima, container-use).
    - devcontainer.json import (DevPod users).
    - A Docker Engine API inside the guest (smolvm).
    - Changing credentials and hosts at runtime, effective at the next connection (smolvm, matchlock).
    - Blocking private, metadata and host-loopback addresses even in "open" mode (microsandbox, srt, Lima v2.3).


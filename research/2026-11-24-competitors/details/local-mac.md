# Local Mac VMs and sandboxes for agents: research report (read 24 Nov 2026)

Sources are first-party unless marked [3rd-party]. Some GitHub pages show release dates without a year; I inferred 2026 from context. `gh api` was refused for openai/tart because this session has no GitHub access, so I read the GitHub web pages instead.

## Docker Sandboxes (`sbx`)
1. **What it is.** "Docker Sandboxes run AI coding agents in isolated environments on your machine or on Docker-managed cloud infrastructure" (https://docs.docker.com/ai/sandboxes/). It is for individual developers, with paid governance for organisations. The old `docker sandbox` commands in Docker Desktop are deprecated in favour of the standalone `sbx` CLI (https://docs.docker.com/ai/sandboxes/docker-desktop.md).
2. **Where it runs**
   - macOS 14+ on Apple silicon, Windows 11 x64 (needs Windows Hypervisor Platform), and Ubuntu 24.04+ (KVM). Docker Desktop is not needed (https://docs.docker.com/ai/sandboxes/install/).
   - A cloud mode exists, and `sbx move my-project --to cloud` moves a sandbox in either direction (https://www.docker.com/blog/introducing-cloud-sandboxes/).
   - **No macOS guest.** Guests are Linux only.
3. **Isolation**
   - Docker wrote its own VMM: "So we built a new VMM, purpose-built for where coding agents actually run." It calls "Apple's Hypervisor.framework" directly, not Virtualization.framework. It uses Windows Hypervisor Platform on Windows and KVM on Linux (https://www.docker.com/blog/why-microvms-the-architecture-behind-docker-sandboxes/, Apr 16 2026).
   - There is one microVM per sandbox, with its own kernel and a private Docker daemon inside.
   - The workspace is a virtiofs passthrough at the same path as on the host. A "clone mode" mounts the repo read-only and gives the agent a private clone (https://docs.docker.com/ai/sandboxes/architecture/).
4. **Recording**
   - Network only. `sbx policy log` "show[s] which hosts a sandbox contacted and which rules matched". It is recent activity only, "not a permanent audit history", and filesystem decisions "aren't available in the log yet" [search snippet of https://docs.docker.com/ai/sandboxes/governance/monitoring/].
   - Recording is done outside the VM, at the host proxy.
   - Audit logs come only with paid governance (https://docs.docker.com/ai/sandboxes/faq/).
   - No process tree, file history or model-call log was found.
5. **Going back**
   - Local: `sbx save` turns a sandbox into a template image or tar (https://docs.docker.com/reference/cli/docker/sandbox/save/). The VM persists across stop and start.
   - Sandboxes API (it appears to target cloud): snapshot with capture mode `all` (memory too) or `disk`; "restoring it creates a new sandbox with its own identity", which works as a fork (https://docs.docker.com/ai/sandboxes-api/cookbook/snapshot-and-fork-a-sandbox/).
   - Snapshots are manual. No speed figures are given.
6. **Network**
   - Deny by default, with presets Open (`allow-all`), Balanced and Locked Down (`deny-all`). The user picks one at first run, and `sbx policy` adds allow and deny rules (https://docs.docker.com/ai/governance/local/network-rules/).
   - **L7 rules** filter on `--method`, `--path` and `--query`: "you can let an agent read from an API without letting it write to it". These are local only; L7 filtering "isn't supported in cloud sandboxes" (https://docs.docker.com/ai/sandboxes/cloud/local-vs-cloud/).
   - Enforcement is on the host: "All outbound TCP traffic passes through a proxy on your host". UDP is off by default, ICMP is blocked, and DNS goes through the proxy (https://docs.docker.com/ai/sandboxes/security/isolation/).
   - The proxy decrypts some traffic, and some it does not: "On connections the proxy doesn't decrypt, the HTTP Host... isn't checked."
   - Docker warns that the default allowed domains "include broad wildcards" such as `*.googleapis.com`.
7. **Secrets**
   - The design is very close to Silo's: "the sandbox sees only a sentinel value". Real values sit in the OS keychain.
   - `sbx secret set-custom` ties a secret to hosts and a placeholder (experimental). "Replacement applies only to HTTP request headers, not bodies."
   - OAuth runs on the host, for example `sbx secret set openai --oauth`. Git works through SSH agent forwarding or `sbx secret set github --command 'gh auth token'`, which refreshes the token (https://docs.docker.com/ai/sandboxes/configuration/credentials/).
8. **Spend.** No model budgets: "Model-provider charges are separate." In the cloud, sessions run 1 h by default and 24 h at most.
9. **Agents**
   - Kits for Claude Code, Codex, Copilot, Cursor, Devin, Docker Agent, Droid, Kiro, OpenCode, and a "Shell" kit with no agent (https://docs.docker.com/ai/sandboxes/agents).
   - The cloud blog also lists Antigravity and Hermes. Custom kits are possible.
   - An MCP gateway is included.
10. **Price**
    - Local: "The sbx CLI and local sandbox compute are free to use, including for commercial work."
    - Cloud, billed per second: Micro (1 vCPU / 2 GiB) $0.07/h, Small $0.14, Medium $0.28, Large $0.56, XL (16 vCPU / 32 GiB) $1.12/h. "Paused sandboxes cost nothing." Egress is free (https://www.docker.com/blog/introducing-cloud-sandboxes/, 24 Sep 2026).
    - Governance is a separate paid subscription.
11. **Open source.** No: "Proprietary — Docker Inc." The repo is public (https://github.com/docker/sbx-releases). The CLI sends usage telemetry, which can be opted out of.
12. **2026**
    - Apr: the microVM VMM.
    - Standalone sbx.
    - 24 Sep: Cloud Sandboxes.
    - v0.54.0 (16 Nov): `sbx policy check network`, and `sbx port publish` (https://github.com/docker/sbx-releases/releases).

**Compared with Silo**
- **What it already does that Silo plans:**
  - A VM per agent.
  - Secrets swapped in at a host proxy.
  - Network limits decided outside the VM.
  - Unmodified CLI agents.
  - Fork from a snapshot (API).
- **What Silo would add:**
  - macOS guests.
  - Recording of processes, files and model calls.
  - Automatic checkpoints.
  - Model spend budgets.
- **Ideas for Silo:**
  - L7 method/path rules ("read-only GitHub").
  - `policy check <host>` to explain which rule matched.
  - A Docker daemon inside the VM.
  - Clone mode (repo read-only plus a private clone).
  - Moving a sandbox from local to cloud.
  - Secrets refreshed by a host command.
- **Top GitHub complaints** (https://github.com/docker/sbx-releases/issues):
  - Forced sign-in, including for air-gapped use (#5, #167, #321).
  - Auth headers changed by the proxy broke Claude Code Remote Control (#8).
  - Claude OAuth token support (#11).
  - SSH agent forwarding authenticates to GitHub unexpectedly (#121).
  - A 4 GB RAM cap (#36).
  - Sync exclusions (#40).
  - `sbx ls` hangs on macOS (#163).

## OrbStack
1. **What it is.** A fast Docker and Linux-machine runtime for Mac developers.
2. **Where it runs.** macOS only. Guests are Linux machines only; I found no macOS guest.
3. **Isolation**
   - "All machines and containers run in OrbStack's single Linux VM and share one kernel."
   - "Isolated machines" (v2.1.0, 19 Apr 2026, "Useful for AI agent sandboxing") drop Mac file sharing. OrbStack calls them "not a full security boundary" and recommends a full VM for code that tries to escape (https://docs.orbstack.dev/machines/isolated).
   - The hypervisor is not named in the docs (https://docs.orbstack.dev/architecture).
4. **Recording.** `orb top` activity monitor (v2.1.2), a per-container Activity Monitor, and a Builds history. Nothing specific to agents.
5. **Going back**
   - v2.2.0 (4 Jun 2026) added "Docker checkpoint/restore (CRIU) support", for containers.
   - Machine snapshots: not found in docs.
6. **Network**
   - `--isolate-network` blocks the host and other machines but keeps the internet. v2.1.2 added a "setting to block network connections to host/machines".
   - No domain allowlist was found.
7. **Secrets.** Isolation keeps "SSH keys, and environment variables" away from the agent. SSH agent forwarding is opt-in (`--forward-ssh-agent`). There is no proxy injection.
8. **Spend.** None. v2.2.0 added per-machine CPU, memory and disk limits.
9. **Agents.** Any agent; you install it yourself. v2.2.0 added sound "to Use voice mode in Claude Code, Codex".
10. **Price.** Free for "Personal, non-commercial use". Pro is $8 per user per month, or $96 a year. Enterprise is priced on request (https://orbstack.dev/pricing).
11. **Open source.** Proprietary. The docs do not address it.
12. **2026.** v2.1.0 isolated machines; v2.1.1 selective mounts; v2.1.2 network block; v2.2.0 limits and CRIU; v2.2.5 (23 Oct) (https://docs.orbstack.dev/release-notes).

**Compared with Silo**
- **What it does that Silo plans:** a Linux environment cut off from the host, aimed at agents.
- **What Silo would add:** its own kernel per agent, macOS guests, recording, checkpoints, an allowlist, secret injection and budgets.
- Tools from other parties pair OrbStack isolated machines (Linux) with Tart (macOS) for agents (https://github.com/phasesoftware/sandbox) [3rd-party].

## Tart (now openai/tart), Orchard, Softnet, Chamber
1. **What it is.** "macOS and Linux VMs on Apple Silicon to use in CI and other automations" (https://github.com/openai/tart).
2. **Where it runs**
   - Apple silicon, macOS 13+. macOS guests are offered, from Monterey to Tahoe; images and `tart create --from-ipsw=latest` (https://tart.run/quick-start/).
   - `tart clone --stacked` "needs macOS 27".
3. **Isolation.** Full VM on Virtualization.framework.
4. **Recording.** Not found in docs.
5. **Going back**
   - `tart suspend` exists for macOS guests (memory state through VZ save/restore).
   - v2.42.0 (17 Nov) added "suspending and resuming Linux VMs" (https://github.com/openai/tart/releases).
   - Clones are APFS copy-on-write. Stacked clones keep the base image read-only and store only the VM's writes.
6. **Network**
   - `--net-softnet` is "essentially a userspace packet filter which restricts the VM networking". By default it allows only globally routable IPv4 and the gateway.
   - Its policy has `allow`/`block` CIDR arrays (max 4096 rules); "block 0.0.0.0/0" plus allows gives default deny. It works on IPs, with no domain names (https://github.com/cirruslabs/softnet).
   - v2.41.0 added `--net-fd` "for network composability". This lets an outside process own the VM's network, which is a hook Silo could use.
7. **Secrets.** Not addressed.
8. **Spend.** None.
9. **Agents.** Any; it is a general-purpose VM tool.
10. **Price**
    - Cirrus "stopped charging licensing fees" (https://cirruslabs.org/).
    - License is **FSL-1.1-ALv2**, "Copyright 2022-2026 OpenAI". It allows any use except "Competing Use" (offering a substitute or substantially similar commercial product). Each version becomes Apache-2.0 on its second anniversary (https://raw.githubusercontent.com/openai/tart/main/LICENSE).
    - Cirrus said it would relicense "under a more permissive license" in the coming weeks. As of today the LICENSE file is still FSL.
11. **Open source.** Source-available (FSL), not OSI.
12. **2026**
    - 7 Apr: Cirrus Labs agreed "to join OpenAI as part of the Agent Infrastructure team". Cirrus CI shut down 1 Jun 2026. Cirrus Runners takes no new customers (https://cirruslabs.org/).
    - OpenAI has announced no product built on Tart [3rd-party].
    - Orchard ("Orchestrator for running Tart Virtual Machines on a cluster of Apple Silicon devices", REST API, needs macOS 15+) is at https://github.com/openai/orchard.
    - Cirrus also has **Chamber**, an ephemeral Tart macOS VM that runs Claude or Codex in "YOLO" mode, destroyed after each run (https://pkg.go.dev/github.com/cirruslabs/chamber).

**Compared with Silo**
- **What it does that Silo plans:** macOS guests, suspend with memory, copy-on-write clones, a default-deny egress filter.
- **What Silo would add:** domain-level policy, secrets, recording, budgets, automatic checkpoints.
- **Risk.** The FSL "Competing Use" clause probably blocks building Silo, a commercial product, on top of Tart code until each version turns Apache-2.0. OpenAI now owns the best open macOS-VM tooling and has an agent-infrastructure motive; a likely competitor.

## Apple `container` / Containerization
1. **What it is.** `container` is "A tool for creating and running Linux containers using lightweight virtual machines on a Mac" (https://github.com/apple/container). Containerization is the Swift package beneath it.
2. **Where it runs**
   - macOS 26+ on Apple silicon. No macOS guests.
   - **New: Containerization has a Linux backend** using "cloud-hypervisor + KVM (CHVirtualMachineManager)" with the same `vminitd` guest contract (https://github.com/apple/containerization README). One guest model can now run on a Mac and on a Linux server.
3. **Isolation.** "executes each Linux container inside of its own lightweight virtual machine". "sub-second start times". Nested virtualization is available through `container run --virtualization`.
   - **Container machines** are persistent Linux VMs built from OCI images. Apple warns they trade isolation away: home directory mounted "read-write, at the same path", SSH_AUTH_SOCK forwarded, "all Linux capabilities", passwordless sudo (https://raw.githubusercontent.com/apple/container/main/docs/container-machine.md).
4. **Recording.** `inspect` output only.
5. **Going back.** `container commit` (1.6.0, 20 Oct). No snapshots.
6. **Network.** Isolated networks between containers (macOS 26). No egress allowlist (https://github.com/apple/container docs/networking.md).
7–8. **Secrets / spend.** None.
9. **Agents.** Any, installed by you. InstaVM's CodeRunner builds an agent sandbox on it [3rd-party].
10–11. **Price / license.** Free, Apache-2.0.
12. **2026.** Releases 1.3 to 1.7.1 (6 Nov). Container machines; k8s/cluster subcommands; the Linux backend.

**For Silo.** This is the Apache-licensed base Silo could legally build its Linux guests on, and its Linux backend matches the "cloud later" plan. It gives no macOS guests and none of the agent controls.

## Lima
1. **What it is.** CNCF (Incubating since Oct 2025) Linux VMs on macOS. Since v2.0 it explicitly targets "secure AI workflows" (https://www.cncf.io/blog/2026/03/25/lima-v2-1-macos-guests-and-enhanced-ai-agent-safety/).
2. **Where it runs**
   - macOS and Linux hosts; a Windows-host HCS driver is experimental.
   - **macOS guests since v2.1** (experimental, vz driver): `limactl start template:macos`. A `macos-27` template arrived in v2.2.1.
   - v2.3.0 adds `osOpts.darwin.suppressFirstLoginSetup` (https://github.com/lima-vm/lima/releases).
3. **Isolation.** Full VM (VZ or QEMU).
4. **Recording.** None.
5. **Going back**
   - `limactl snapshot create/apply/list/delete` is marked "experimental" (https://lima-vm.io/docs/releases/experimental). Which drivers support it is not stated in docs; my memory, unverified, is QEMU only.
   - **`limactl shell --sync . default claude "..."`** (v2.1) copies the repo in with rsync. Afterwards it asks Yes / No / "View the changed contents" before writing back to the host. `--sync-exclude` and `.limasyncignore` arrived in v2.3 (https://lima-vm.io/docs/examples/ai/).
   - v2.3.0 fixed a bug where "`limactl shell --sync` applying unreviewed guest changes".
6. **Network**
   - v2.3.0 `networkOpts.hostLoopback` blocks the guest from reaching host loopback services (VZ).
   - No domain allowlist found.
7. **Secrets.** API keys go in as env vars inside the VM ("exporting ANTHROPIC_API_KEY").
8. **Spend.** None.
9. **Agents**
   - Documented: Aider, Antigravity, Claude Code, Codex, Copilot, OpenCode. They run unmodified.
   - "MCP tools for reading, writing, and executing local files using a VM sandbox" (`limactl mcp serve`; v2.3 added a `replace` tool) let an agent on the host do its work inside a VM (https://lima-vm.io/docs/config/ai/outside/).
10–11. **Price / license.** Free, Apache-2.0.

**For Silo.** The review gate before results reach the host (sync, diff, then accept) is a pattern Silo has not planned. The MCP "agent outside, tools inside" mode is a different model from Silo's "whole agent inside".

## UTM
1. **What it is.** A QEMU and Apple Virtualization front-end for Mac and iOS. General purpose.
2. **Where it runs.** macOS (and iOS). It can "Run multiple instances of macOS on your Apple Silicon Mac" (https://getutm.app/), still subject to the two-instance limit in Apple's licence.
3. **Isolation.** Full VM, on QEMU (with HVF) or Apple Virtualization.
5. **Going back**
   - **v5.0.6 (24 Sep 2026)** added a Snapshots tab: save, overwrite, restore, rename, delete, for "QEMU and Apple VMs".
   - Snapshots are scriptable through AppleScript, and `utmctl` gained `snapshot` and `discard-state`.
   - Apple-backend snapshots need macOS 27, and the VM must be stopped or suspended [3rd-party summary of release notes]. "run without saving changes" works on macOS 27.
   - v5.0.9 (16 Nov) is the latest (https://github.com/utmapp/UTM/releases).
   - On macOS 27, scripting can create and install macOS VMs and provision the user account, auto-login and Remote Login on first boot.
4, 6–9. **Recording, network, secrets, spend, agents.** No agent-specific features found. A third-party CLI, `utmd`, adds JSON output for agent pipelines.
10–11. **Price / license.** Free from GitHub; paid on the Mac App Store, where the price is not on the site. Open source; the license is not named on the site (Apache-2.0 from memory).

## Anka (Veertu)
1. **What it is.** macOS VM virtualization for CI fleets: Anka Virtualization CLI, Build Cloud, the free Develop tier, and Flow for local developer VMs (https://docs.veertu.com/anka/).
2. **Where it runs.** Mac hosts, including AWS EC2 Mac. macOS guests are its focus. v3.9.3 (21 Sep 2026) added macOS 27 hosts.
3. **Isolation.** Full VM.
4. **Recording**
   - Only "event logging" in Enterprise Plus.
   - The agent guide's advice: "Log tool invocations and generated scripts where practical."
5. **Going back**
   - "Ability to suspend and then instantly start VMs from a specific state". Clones reuse layers.
   - Suspended VMs cannot move between Apple silicon hosts (an Apple limitation).
6. **Network.** IP filtering per VM: `block local`, `block in to port 22`, allowlist then `block any`. "Advanced network security features require an Enterprise or Enterprise Plus license."
7. **Secrets.** Advice only: "Scope credentials to the project". No proxy injection.
8. **Spend.** None.
9. **Agents**
   - The docs page "Isolating AI Coding Agents with Anka" walks through template, per-task clone, mount only the project, restrict the network, run the agent, delete. It shows `claude --dangerously-skip-permissions` over SSH and a helper "Crypt" that "runs coding agents (Claude Code, Codex, and others) inside the clone" (https://docs.veertu.com/anka/artificial-intelligence/isolating-ai-coding-agents/).
   - The CLI can pull GGUF model weights from Hugging Face onto a VM.
10. **Price**
    - Develop: free, "a single VM at a time", laptops only.
    - Build tiers: price on request. Apple silicon is licensed per host, not per core (https://docs.veertu.com/anka/licensing/).
    - AWS Marketplace: from $0.90/h or $5,880/yr [listing, undated].
11. **Open source.** Proprietary.
12. **2026.** 3.10.0 (9 Oct): USB passthrough, thin OCI tags. 3.10.2 (17 Nov).

## New 2026 Mac apps for agents (top 3)

### MacVisor (ScaleNinja) — the closest match to Silo
- **What it is.** A virtualization platform for Mac: "Run VMs on your Mac as infrastructure" (https://scaleninja.com/macvisor/).
  - Built on Virtualization.framework, with a background service so VMs keep running when the app is closed. Needs macOS 27.
  - Guests: macOS 14–27, Linux, Windows 11 ARM (needs macOS 27.2), FreeBSD.
- **AI sandboxes** (Tech Preview) (https://scaleninja.com/docs/macvisor/ai-sandboxes/):
  - **Guest and project copy:** a disposable VM, Linux or macOS 27, holding a copy of the project with no git history.
  - **Review before apply:** `mvz sandbox diff` "flags (scripts, git hooks, CI files, agent config like .claude/)". `mvz sandbox apply` is all-or-nothing, uses a three-way merge, and can write to a new branch with `--branch`.
  - **Network:** a gateway on the Mac handles it. Modes are `allowlist` (default), `none` (models only) and `open`, with presets such as npm and PyPI.
  - **Keys:** stored in the Keychain; "adds it to each request as it leaves the Mac".
  - **Provider-run tools:** the gateway "refuses provider-run tools that could reach arbitrary sites" (Anthropic web fetch, MCP connector, OpenAI remote MCP) unless `--provider-tools` is on.
  - **Checkpoints:** "saves the whole VM, including what's running", "Rolling back takes seconds", plus a `pristine` checkpoint after setup.
  - **Logs:** `mvz ai log` lists model requests "with model, duration, and tokens"; "Request contents are never logged". `mvz sandbox log --egress` shows allowed and blocked traffic.
  - **Budgets:** "MacVisor sets no budget, so watch the provider's own usage limits."
  - **Agents:** Goose (default), Claude Code, Codex, Aider, OpenCode.
  - **Model routing:** by model name, including to local models (Ollama, LM Studio, llama.cpp).
  - **Subscription sign-in:** only with `--egress open`, signing in inside the VM.
- **Limits** (https://scaleninja.com/docs/macvisor/known-limitations/):
  - "at most two macOS sandboxes".
  - Live snapshots restore "only on the macOS release and the MacVisor version that saved them".
  - "The firewall filters IPv4 only".
  - Sandboxes on the allowlist network can reach each other.
- **Price.** $59 one-time (launch price until 30 Nov, then $69), Duo $79, Business $79 per Mac per year. Closed source. Based in India.

**For Silo:** MacVisor already ships most of Silo's pitch: macOS/Linux VM, placeholder keys, allowlist, whole-VM checkpoints, model-call log with tokens. Silo's differentiators narrow to:
- Budgets enforced at the boundary.
- Automatic checkpoints, and forking.
- Process and file recording.
- Subscription/OAuth agents working with a closed network. MacVisor forces `open` for that.
- Its weak spots to beat: IPv4-only firewall, sandboxes reaching each other, snapshots tied to a version, the forced-open mode for subscriptions.

### VMPal (TablePlus)
- **What it is.** A VM app for Apple silicon (needs macOS 26+). Guests: macOS (27 included), Windows 11 (alpha, "own engine"), Ubuntu/Fedora with nested virtualization (https://vmpal.com/).
- **Agents:** a built-in MCP server added in one click to Claude Code, Codex, Grok Build. The agent can "see the screen, control the keyboard and mouse, and run commands", as user or admin. Agent control can be switched off per VM.
- **Snapshots:** live or stopped; "roll back in seconds"; storage grows only as the VM changes.
- **Network, secrets, recording, budgets:** not found.
- **Price and version:** $34.5–$69 one-time, reports conflict [3rd-party: https://www.everydev.ai/tools/vmpal]. Version 0.124. Launched around Oct 2026.
- **For Silo:** this is the "agent drives a whole Mac GUI" model (computer use). Silo has not planned an MCP server for screen and input; it may be worth having for macOS app testing.

### Parallels Desktop and `pdfm-mcp`
- **What it is.** An MIT-licensed local MCP server with about 34 tools (https://github.com/Parallels/pdfm-mcp):
  - exec in the guest;
  - screenshots and OCR, keystrokes;
  - snapshot create, switch, delete; clone;
  - power and settings.
- **Access controls:** `--read-only`, `--allow-vms`, `--allow-tools`, `--deny-tools`. Telemetry is on by default.
- **Clients:** Claude Code, Codex, Copilot, Gemini CLI.
- **Dates and price:** 0.1.4 became stable 15 Oct 2026. Pro was $119.99/yr, with a $29.99 upgrade promo (Nov 2026) [3rd-party: itwire, 5 Nov 2026].
- **For Silo:** the agent here is the operator of the VM, from outside, not a tenant inside it. There are no network or secret controls.

**Others seen, not covered in depth:**
- Chamber (Tart).
- ClodPod (macOS VM).
- SandVault (separate macOS user plus sandbox-exec).
- agentstep `mvm` (Linux microVMs, M3+).
- inoio/agents-sandbox (microVM with secret injection on the host).
- CodeRunner/InstaVM (on Apple container).
- Macswarm (hosted cloud Macs per Pod-hour).
- A list of about 100 sandboxes: https://gist.github.com/wincent/2752d8d97727577050c043e4ff9e386e

## Cross-cutting observations
- **Placeholder secrets swapped at a host proxy are now standard.** Docker ("sentinel value", headers only), MacVisor (Keychain to gateway) and inoio all do it. Silo's own secrets are no longer a differentiator. What would still set Silo apart:
  - swapping in bodies and other protocols, not only headers;
  - OAuth/subscription agents working without opening the network. Docker runs OAuth on the host; MacVisor forces `open`; Docker issue #11 asks for Claude OAuth tokens.
- **No product found enforces a model spend budget.** MacVisor says outright: "MacVisor sets no budget". Docker: "Model-provider charges are separate." Recording model calls is also rare: only MacVisor, and only token counts. Budgets plus a full recording of the agent's actions are Silo's clearest unique ground.
- **No product found records processes or file changes.** Docker logs network decisions only, for "recent activity, not a permanent audit history". Lima and MacVisor show a diff at the end, not a timeline. Anka only advises logging tool calls.
- **Apple caps every macOS-guest product at two macOS VMs per Mac** ("up to two (2) additional copies or instances"; MacVisor: "Two macOS VMs at a time"). For Silo, forking and parallel agents on macOS guests hit this limit at two. Linux guests are needed for anything parallel, or the cloud on many Macs.
- **macOS 27 changes the VZ toolkit** (WWDC26 session 224):
  - **DiskImageKit stacked ASIF images:** a read-only base, plus cache and copy-on-write overlays. "Stack multiple overlays and you get snapshot- and fork-like behavior" (https://bitrise.io/blog/post/wwdc26-the-virtualization-framework-updates-that-matter-for-large-mac-fleets). A developer adds that this gives "linear incremental checkpoints and linked clones" without flattening back into the base (https://eclecticlight.co/2026/06/12/macos-virtualisation-is-leaping-forward-in-golden-gate/).
  - **`VZMacGuestProvisioningOptions`:** user account, auto-login and SSH set at first boot.
  - **vmnet in VZ:** NAT per VM, port forwards.
  - **USB passthrough.**
  - **Effect on the market:** Tart (`--stacked`), UTM (snapshots on Apple VMs) and MacVisor (requires 27) already depend on these. Silo's checkpoints and forks on macOS should be designed on them, and the minimum macOS version decided accordingly.
- **Licensing:**
  - Tart is FSL-1.1-ALv2 with a "Competing Use" ban, now owned by OpenAI's Agent Infrastructure team. OpenAI could ship its own local macOS-VM sandbox for agents.
  - Apple Containerization (Apache-2.0, now with a cloud-hypervisor/KVM Linux backend), Lima (Apache-2.0) and UTM are the safe bases.
- **The review gate is a pattern Silo has not planned.** Lima `--sync` (Yes / No / View diff), MacVisor `diff`/`apply --branch` (flags hooks, CI and `.claude/` files), and Docker clone mode all keep the agent's writes away from the real repo until accepted. Silo plans restore and fork, but not a "review and apply to the host" step that flags risky files.
- **L7 rules and provider-run tools are an egress gap.** Docker filters on HTTP method and path, and MacVisor blocks Anthropic/OpenAI web-fetch and MCP tools that would let the model reach the web around the allowlist. Silo's host-list policy should consider both. Docker also admits that wildcard defaults like `*.googleapis.com` are broad.
- **Pricing:**
  - Local sandboxes are free (Docker) or a one-time $35–69 (VMPal, MacVisor).
  - Cloud: $0.07 per vCPU-hour (Docker); Anka $0.90/h on AWS.
  - OrbStack Pro: $8 per user per month.
  - Charging a subscription for local-only use will be compared against these.
- **User complaints:**
  - Docker: forced sign-in (#5/#167/#321), proxy changing auth headers and breaking Claude Remote Control (#8), unexpected forwarding of the SSH agent to GitHub (#121), memory caps (#36/#56), macOS hangs (#163).
  - HN (https://news.ycombinator.com/item?id=48223693): sbx is closed source; it "auto-launched Docker Desktop's daemon" over OrbStack; and "Is a container breach really the relevant problem to solve for agents?", pointing at malicious packages and tokens instead. That last point supports Silo's emphasis on recording, secrets and the network over the kernel boundary alone.


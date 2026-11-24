# macOS-guest VMs for coding agents on a Mac (as of 24 Nov 2026)

I read each product's own pages with WebFetch/WebSearch. I could not use the GitHub API for any of these repos (the session has no access to them), so release dates come from the HTML release and commit pages. Some of those pages show no year. Anything I could not confirm is marked **UNVERIFIED**.

The main finding: **only MacVisor comes close to the full goal, and it falls short on budgets.** Most of the tools in this list are plain VM managers. Three of them (shuru, clawcage, mvm) run Linux guests, not macOS.

---

### GhostVM (github.com/groundwater/GhostVM)
- **macOS guest:** yes. "Isolated macOS workspaces for agents, projects, and clients." Built on Virtualization.framework, with each VM stored as a `.GhostVM` bundle.
- **Recording:** none described.
- **Checkpoints:** manual. "Checkpoint and clone instantly with APFS copy-on-write". The CLI has `vmctl snapshot <bundle> create|revert|list|delete`. Suspend/resume is listed separately, and the page doesn't say whether snapshots keep memory. Nothing is automatic. Release notes mention clone-while-running.
- **Network:** "VMs use NAT networking via VZNATNetworkDeviceAttachment". No egress policy or allowlist.
- **Secrets, budgets:** none.
- **Agents:** none named, only "AI agent sandboxing".
- **License and price:** source "License not yet determined". The signed DMG "is free to use for any purpose".
- **Last release:** v3.0.20260601003045 (beta), 1 Jun 2026.
- Source: https://github.com/groundwater/GhostVM

### ClodPod (github.com/webcoyote/clodpod)
- **macOS guest:** yes, via Tart. The default image is `ghcr.io/cirruslabs/macos-golden-gate-xcode:latest`. It "creates a macOS virtual machine sandbox".
- **Recording:** none.
- **Checkpoints:** none. Each instance is "a cheap APFS CoW clone of the base VM", created and destroyed by hand (`clod create`/`clod destroy`).
- **Network:** no allowlist. It only forwards host ports over SSH reverse tunnels.
- **Secrets:** nothing beyond mounting only the project and turning sudo off by default.
- **Budgets:** only a RAM budget ("5/8 of host RAM"). There is no spend budget.
- **Agents:** Claude Code, Codex, Gemini and Cursor Agent, plus the herdr multiplexer.
- **License and price:** Apache-2.0 and free. Tart itself is FSL-1.1, and commercial use "may require a paid license".
- **Last activity:** last commit 28 Oct 2026. Version 1.0.30 was bumped on 17 Oct.
- Source: https://github.com/webcoyote/clodpod

### vm4a, now named EZVM (github.com/everettjf/vm4a)
- **macOS guest:** yes, plus Linux ARM64. It is for "isolated macOS or Linux VMs on Apple Silicon for AI agents to safely run code in."
- **Recording:** none. The CLI returns one "schema-versioned JSON object" per command, but there is no log of what the agent did.
- **Checkpoints:** manual and disk only, taken on a stopped VM using APFS clones. It "intentionally does not support Virtualization.framework machine-state save/restore."
- **Network, secrets, budgets:** none.
- **Agents:** none named. It "will not embed an AI model or require a cloud account."
- **License and price:** MIT, free.
- **Last release:** EZVM 2.0.1, 9 Sep (year not shown; probably 2026).
- Source: https://github.com/everettjf/vm4a

### shuru (github.com/homanp/shuru, a fork of superhq-ai/shuru) — **Linux guest, not macOS**
- **Guest:** "Local-first microVM sandbox for AI agents on macOS". The host is macOS, but the guests are Linux.
- **Recording:** none.
- **Checkpoints:** manual and disk only, with branches. The rootfs is otherwise "ephemeral: the rootfs resets on every run."
- **Network:** off unless `--allow-net` is given. The `--allow-host` flag or `network.allow` restricts it, and if no list is given, all hosts are allowed.
- **Secrets:** a proxy swaps a placeholder for the real key. "The real secret never enters the VM."
- **Budgets:** none.
- **Agents:** shipped as an agent skill for Claude Code, Cursor and Copilot, plus a TypeScript SDK.
- **License, last release:** **UNVERIFIED** (the page didn't show them; there is a CHANGELOG, and v0.1.11 is referenced).
- Source: https://github.com/homanp/shuru

### clawcage (github.com/hackyguru/clawcage) — **Linux guest, not macOS**
- **Guest:** it "sandboxes AI agents in isolated Linux VMs using Apple's Virtualization.framework".
- **Recording:** live view only. "See every HTTP request, tool call, and file change in real time". Persistent logs are not described.
- **Checkpoints:** ephemeral by default ("scratch disk is formatted fresh every boot"). v1.0.0 added cloud sync of snapshots with daily auto-sync and 30-day history on Pro+. Whether these hold memory or disk is not stated.
- **Network:** "no NIC exists in the VM". Everything goes through a host MITM proxy with domain allow/block lists and method/path rules, and corporate policy overrides the user's.
- **Secrets:** "API keys never enter the guest VM". The host proxy injects them.
- **Budgets:** none.
- **Agents:** Claude, Gemini, Codex and "any CLI tool", with OpenClaw and Hermes templates.
- **License and price:** CC BY-NC 4.0. Pro and Pro+ are sold via Polar.sh, prices **UNVERIFIED**.
- **Last release:** v1.1.0, 21 Apr (year not shown; adds a "Remote Mode" cloud VPS).
- Sources: https://github.com/hackyguru/clawcage, https://github.com/hackyguru/clawcage/releases

### CelestoAI SmolVM: "disposable macOS sandboxes"
- **macOS guest:** **yes, it truly offers them.** PyPI says: "On an Apple Silicon Mac, SmolVM can open a temporary macOS desktop". Preparing the image "needs about 50 GB and 20–40 minutes", and "at most two macOS guests can run at once". You view it through Screen Sharing, and each sandbox is a copy-on-write clone of one base image. The macOS backend is QEMU according to PyPI and a third party; **UNVERIFIED** for the macOS path specifically.
- **Recording, secrets, budgets:** none mentioned.
- **Checkpoints:** none for macOS. Snapshots are "Linux-only today".
- **Network:** "Sandboxes have internet access by default."
- **License and price:** Apache-2.0, free.
- **Last release:** 0.0.36, 17 Sep 2026.
- Sources: https://pypi.org/project/smolvm/, https://dev.to/aniketmaurya/disposable-macos-sandboxes-for-ai-agents-test-safely-every-time-3och

### Cirrus Labs Chamber (github.com/cirruslabs/chamber)
- **macOS guest:** yes, Tart VMs (`chamber init ghcr.io/cirruslabs/macos-sequoia-base:latest`). `chamber claude` runs the agent with "the dangerous flags" added for you.
- **Checkpoints:** none. "VMs are destroyed after each run - always start from a clean seed image."
- **Recording, network, secrets, budgets:** none.
- **Agents:** Claude and Codex.
- **License and price:** AGPLv3 and free, plus Tart's Fair Source licence.
- **Last release:** v0.2.0, 10 Dec 2025 (from pkg.go.dev). The repo has 36 commits and looks like a small proof of concept.
- Sources: https://github.com/cirruslabs/chamber, https://pkg.go.dev/github.com/cirruslabs/chamber

### Macswarm: **hosted, not a VM on your Mac**
- **What it is:** "Macswarm gives AI coding agents a cloud Mac". Each Pod is a macOS VM with Xcode, Simulators and XcodeBuildMCP. A developer's sessions share a Pod in separate worktrees, and "The Pod is destroyed ten minutes after the last session ends". Regions are EU and US.
- **Agents:** Claude Code and Codex. Model use is billed by Anthropic or OpenAI to your own account.
- **Recording, checkpoints, network, secrets, budgets:** not covered.
- **Price:** 3 free Pod-hours, then Pro $99/mo for 300 Pod-hours and Team $399 for 1,500. This comes from infrabase.ai; I did not fetch macswarm.com/pricing directly, so treat it as **secondhand**.
- Sources: https://infrabase.ai/agents/macswarm, https://macswarm.com

### MacVisor by ScaleNinja (all requested claims checked)
- **Model-request log with tokens: VERIFIED.** It "lists a sandbox's model requests with the model, time taken and tokens used". "Request contents are never logged." `mvz sandbox log app-1 --egress` shows "what was allowed and blocked". It does not say whether commands or the screen are recorded.
- **Keychain key injection at a gateway: VERIFIED.** "MacVisor adds it to each request as it leaves the Mac, so the agent never sees it". Keys "stay in the Keychain". The gateway "refuses the providers' own internet tools that MacVisor knows of", but "one a provider adds later may get through".
- **Allowlist by default: VERIFIED.** The default egress mode is `allowlist`, enforced by MacVisor's proxy, which "checks the name the agent asks to connect to". The default list includes "npm, PyPI, GitHub and your distribution's mirrors". Installer sites are removed before the `pristine` checkpoint. Limits: the firewall "filters IPv4 only" and drops IPv6, and "traffic between two VMs on the same network never reaches the firewall".
- **Whole-VM checkpoints including running state: VERIFIED.** "A checkpoint saves the whole VM, including what's running." One automatic checkpoint, `pristine`, is taken after setup. All others are manual (`mvz sandbox checkpoint app-1 before-refactor`), and rollback "takes seconds". Live snapshots "restore only on the macOS release and the MacVisor version that saved them" and "only while you are logged in at the Mac with the screen unlocked".
- **"Sets no budget": VERIFIED.** "MacVisor sets no budget, so watch the provider's own usage limits."
- **Subscription sign-in only with open egress: VERIFIED.** Signing in with a Claude or ChatGPT subscription needs "`--egress open`" and no stored provider key. Open egress gives the sandbox "The internet, your local network and your Mac".
- **Two macOS sandboxes at most: VERIFIED.** "at most two macOS sandboxes". Linux sandboxes have no cap. Linux is the default sandbox guest; the macOS option is macOS 27 only. Windows can't be a sandbox. The whole feature is a Tech Preview.
- **Price $59/$69: VERIFIED.** Pro Individual is $59 at launch until 30 Nov 2026, then $69 one-time. Pro Duo is $79, then $99. Business is $79, then $99 per Mac per year, from 3 Macs. There is a 30-day trial. The product is proprietary (EULA).
- **Agents:** Goose (the default), Claude Code, Codex, Aider and OpenCode, each "with its official installer", or none. Its MCP server works with Claude Code, Claude Desktop, Codex and Cursor.
- **Release:** "MacVisor 27", which requires a macOS 27 host. Blog posts are dated 16–24 Nov 2026; the release date itself is **UNVERIFIED**.
- Sources: https://scaleninja.com/macvisor, https://scaleninja.com/docs/macvisor/ai-sandboxes/, https://scaleninja.com/docs/macvisor/known-limitations/

### Other 2026 products found
- **testvm + VM Peek (Flavio Copes): the only other one with recording kept outside the VM.** testvm drives a Tart macOS Sequoia VM and logs every command to `~/Library/Logs/testvm` on the host. The log "can contain agent prompts, command output and screenshots". VM Peek shows the live screen, which agent is using the VM, and the commands. Agents named are Cursor, Claude Code and Codex. There are no checkpoints, network limits, secrets handling or budgets. MIT source, $19 one-time, v1.5.0 on 20 Nov 2026. Source: https://flaviocopes.com/vm-monitor.md
- **VMPal (TablePlus team), vmpal.com:** macOS, Linux and Windows guests. "Take a snapshot before the agent starts" and roll back the whole machine; manual, and memory not stated. Its MCP works with "Claude Code, Codex, Grok Build, Antigravity and other agents". It has no allowlist (port forwarding only), no recording, no secrets handling and no budgets. Version 0.124, macOS 26+ host, paid; sources give either $34.50 or $69, so the price is **UNVERIFIED**. Sources: https://vmpal.com, https://laravel-news.com/vmpal-ai-agent-virtual-machine
- **Lume / cua (trycua):** macOS and Linux VMs on Virtualization.framework with an MCP server. A third-party guide says the VMs have full internet access by default. I found no recording or snapshot features (**UNVERIFIED**). Source: https://github.com/trycua/cua
- **agentstep/mvm:** Linux guests only. It is the one tool here with manual memory+disk snapshots and `--net-policy allow:<domains>`. Apache-2.0. Source: https://github.com/agentstep/mvm
- **Mirage (solcreek/mirage):** described as ephemeral macOS VMs with an MCP server. The page returned 403, so everything about it is **UNVERIFIED**.
- I found no 2026 product that combines a macOS VM with recording and replay of the agent's session. Host-side session viewers (agentplay, agent-sessions, IAXT) exist, but none of them runs a VM.

---

### Summary: how close each comes to the full goal
(macOS VM for unmodified agents + recording outside the VM + automatic checkpoints + secrets outside the VM + budgets)

- **MacVisor is closest, with 3½ of 5.** It has a macOS 27 guest running unmodified, officially installed agents. Outside the VM it logs model requests (model, time, tokens) and egress decisions, but not commands or the screen. Its checkpoints are whole-VM with memory, but only `pristine` is automatic. API keys stay in the Keychain and are injected at the gateway. It has **no budget**, it is capped at **two macOS sandboxes**, and subscription sign-in forces **open egress**, which loses both the allowlist and the key protection.
- **clawcage and shuru do best on network and secrets, but run Linux guests, not macOS.** Both keep keys outside the VM with a host proxy and enforce a domain allowlist. clawcage also shows live HTTP, tool-call and file-change activity. Neither has budgets or memory checkpoints.
- **testvm + VM Peek is the only macOS-guest tool with a recording outside the VM** (commands, prompts, screenshots, on the host). It has no checkpoints, network limits, secrets handling or budgets.
- **GhostVM, EZVM/vm4a and VMPal are macOS VM managers with manual snapshots.** GhostVM has APFS checkpoints and a separate suspend; EZVM is disk-only by design; VMPal has whole-machine rollback. None has recording, egress policy, secrets handling or budgets.
- **ClodPod, Chamber, SmolVM-macOS and Macswarm offer disposable macOS VMs for unmodified agents** (Claude Code, Codex and others), with ephemerality standing in for checkpoints. None has recording, an allowlist, secrets kept outside the VM or budgets. Macswarm is also hosted rather than local.

**Open gap:** nobody combines all five. In particular, **no product found sets a model-spend budget**, and none pairs automatic checkpoints with a macOS guest. The only full outside-the-VM recordings are testvm's command log and MacVisor's model-request and egress logs.

No files were written.

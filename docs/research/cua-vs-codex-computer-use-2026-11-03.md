# Cua vs Codex computer use, and DMCA exposure

Researched 2026-11-03. Not legal advice.

## Verdict

Cua Driver (trycua/cua) is a comparable and safer replacement for the OpenAI computer-use runtime that LCU wraps. The only public head-to-head that tests the same thing LCU does (OpenAI's tool layer driven by a non-OpenAI agent) put Cua Driver ahead: 40/50 vs 30/50 foreground success, 21/50 vs 17/50 background completion. That run is vendor-run, macOS-only and small, so treat it as "at least on par", not as proof Cua is better. Cua Driver is MIT, ships Linux x86-64 and arm64 builds, supports X11 + AT-SPI (what Silo's Xfce guests use), and can be baked into Silo's guest image, which removes the OpenAI download and the Service Terms question entirely. Nothing has been measured on Linux yet, so Silo would need its own smoke run.

## 1. What is being compared

LCU does not supply a model. It exposes OpenAI's computer-use tool layer (`cua_repl`, the "Sky" service, in the ChatGPT app) as an MCP server, and the harness's own model (Claude, GPT) picks the actions. Cua Driver is the same kind of thing: a tool layer exposed over MCP/CLI/SDK. So OSWorld-style leaderboards, which score models, do not answer the question. The right comparison is tool layer vs tool layer with the same agent, and one such benchmark exists.

## 2. Head-to-head: Cua Driver Bench v2 (vendor run)

Source: `libs/cua-driver-bench/` in trycua/cua (README and METHOD.md, cloned 2026-11-03), PR https://github.com/trycua/cua/pull/4694 (opened 2026-10-05, closed 2026-10-11, "Superseded by #4973"), PR https://github.com/trycua/cua/pull/4973 (merged 2026-10-11), outcome in commit https://github.com/trycua/cua/commit/69229a31c55f6693cbab20ac48287d2b17d42bac (2026-10-11 09:10 UTC) and rescoring in https://github.com/trycua/cua/commit/91a758aa914a5a4a671eb338c173fecc81cbc760 (2026-10-11 14:54 UTC).

Setup: macOS 26.5.2 VM, Claude Code + Claude Sonnet 5.5 drives every arm, same prompts, 360 s / 45 turns per trial, no shell tools, 10 tasks x 5 runs = 50 trials per arm and condition, checkers read app state (no LLM judge). The Codex arm is "OpenAI's `cua_repl` launcher from the ChatGPT app 26.930 plugin ... Claude chooses the actions through this MCP bridge; OpenAI's model is not involved." That is exactly LCU's model.

A17.3 outcome (11 Oct 2026, 300 trials, 0 infra exclusions):

| Arm | Foreground success | Background completion | Background focus steals | Swallowed user keystrokes |
|---|---|---|---|---|
| Cua Driver | 40/50 | 21/50 | 2 | 55 |
| Codex computer use (Sky) | 30/50 | 17/50 | 6 | 503 |
| Claude Desktop CU helper | 16/50 | 6/50 | 20 | 439 |

Rescored under the stricter Amendment 22 "user undisturbed" rule: Cua Driver 16/50, Codex 17/50, helper 6/50 (labelled a secondary figure).

Caveats:
- Run by Cua, about its own product. The README says "No results are published here yet"; the numbers above are from pre-registration commits, not a results page.
- n = 50; the README itself says per-category differences "are directional only". 40/50 vs 30/50 has overlapping Wilson 95% intervals (roughly 67–89% vs 46–72%).
- macOS only. Linux: UNKNOWN.
- Codex CLI + GPT-6.1 Sol arms (OpenAI's own agent driving each tool) are registered but "not yet run". UNKNOWN.

## 3. Public benchmarks (OSWorld, ScreenSpot, WebArena)

- Cua Driver or the Cua agent SDK on OSWorld / OSWorld-Verified / OSWorld 2.0: **UNKNOWN**. Not on https://leaderboard.steel.dev/leaderboards/osworld.md (updated 2026-10-11) or https://presenc.ai/research/osworld-computer-use-leaderboard-2026. Cua provides harnesses (Cua Bench, HUD integration of 2025-08-27) but publishes no score of its own.
- "Codex computer use" as a product on OSWorld: **UNKNOWN**. OpenAI publishes model scores only:
  - OpenAI Operator (CUA), January 2025: 32.6% (steel.dev table); Cua's own blog cites 38.1% OSWorld for the same model.
  - GPT-5.4: 75.0% OSWorld-Verified, launch eval (steel.dev).
  - GPT-5.6 Sol (max effort): 62.72% partial / 27.34% binary, OSWorld 2.0 v2026.08.08, official board (presenc.ai).
  - GPT-6 Astra 72.6% and GPT-6.1 Sol 70.5% OSWorld 2.0 partial, from launch coverage / trackers; metric unclear per presenc.ai.
- ScreenSpot / ScreenSpot-Pro, WebArena, any head-to-head Cua vs Codex: **UNKNOWN**. Cua ships a ScreenSpot-v2 runner (`libs/python/agent/benchmarks`) but no published result.
- Community: https://github.com/saariuslystoned/agy-cua/issues/12 (2026-10-15) ran Codex CU only, using Cua Driver for setup; "No performance comparison is claimed." https://codenote.net/en/posts/codex-computer-use-cua-repl-vs-trycua-cua-driver/ (2026-10-01) found no shared code between OpenAI's `cua_repl` and trycua's driver; the shared "cua" name is just "Computer-Using Agent".

## 4. License

From the trycua/cua README "Licensing" section (2026-11-03):
- **MIT**: the cua SDK, CLI, `cua daemon`, **Cua Driver**, Lume, and everything else unless a directory says otherwise. Cua Driver's Cargo workspace declares `license = "MIT"`.
- **FSL-1.1-MIT** (source-available, becomes MIT after two years, no competing hosted service): Cua Spaces apps, `cua-spacesd`, Cua Keyvault, Cua Driver Bench (from 11 Oct 2026).
- **AGPL**: optional `cua-perception` extension (OmniParser model) and optional `cua-som`. Do not ship these.
- "The MIT parts never depend on the FSL parts."

For Silo: ship only Cua Driver (MIT). No OpenAI software, no FSL or AGPL component, so it can be baked into the guest image and redistributed under Silo's MIT licence with a notice.

## 5. Fit with Silo's Linux guests

Silo today (docs/SiloUI-COMPUTER-USE-PLAN.md): v4 guest image with Xfce on X11 and Selkies; each device downloads the ChatGPT `.deb` from OpenAI and mounts it read-only for LCU; LCU registers MCP server `lcu` in Claude Code and Codex; approvals via `permissions.allow: ["mcp__lcu"]` and `[mcp_servers.lcu] default_tools_approval_mode = "approve"`.

Cua Driver on Linux (docs `cua-driver/concepts/platform-support.mdx`): "Linux X11 | X11/EWMH, XTest, AT-SPI | Supported with toolkit limits." Prebuilts for linux-x86_64 and linux-arm64. Runs as a local stdio MCP server; documented integrations for Claude Code and Codex. Cua's own `trycua/cua-xfce` image already bundles it on Xfce.

Swap plan:
1. Add the pinned Cua Driver binary (per-arch SHA-256) to the v4 guest image. Drop the ChatGPT downloader, the read-only app mount and the Settings "Connections" failure UI.
2. Register its MCP server in each harness instead of `lcu`; change the approval lines to the new server name.
3. Keep Silo's accessibility defaults (AT-SPI is what Cua uses too).
4. Re-run Silo's existing computer-use checks (GNOME Text Editor GTK4 typing/paste, Xfce Terminal `pressKey`, file checks) with Claude Code and Codex. These are the Linux numbers that don't exist publicly.
5. Watch for: toolkit refusals of synthetic background input on X11, Chrome on X11 lacking trusted pointer input (`browser_input_trust_unavailable`), and the same Codex sandbox X11 `connect` block LCU hit.

## 6. DMCA: is it just a takedown?

A DMCA notice itself carries no fine. Under GitHub's policy (https://docs.github.com/en/site-policy/content-removal-policies/dmca-takedown-policy) the owner gets "approximately 1 business day to delete or modify the content", otherwise "we will disable the repository". Repeat infringers can have accounts terminated. A counter-notice restores the content unless the claimant files suit within 10–14 days.

But the takedown is not the only remedy available to OpenAI:
- **Copyright suit** (separate from the notice): statutory damages of $750–$30,000 per work, up to $150,000 per work if willful (17 U.S.C. §504(c)), plus possible attorney's fees (§505), if the work is registered.
- **Anti-circumvention** (17 U.S.C. §1201/§1203): $200–$2,500 per act, if a technical protection measure was bypassed.
- **Terms of Use**: suspension or termination of the ChatGPT account(s) involved, and in principle a contract claim.

Track record found: OpenAI has sent DMCA notices against copies of its CUA code (see research/openai-terms.md). No lawsuit or fine against a tool that uses an installed ChatGPT app was found. A DMCA notice only reaches hosted OpenAI bytes; LCU's main exposure (driving the runtime from third-party agents) is a terms issue, which OpenAI would answer with account action or a block, not a DMCA notice. Switching to Cua Driver removes both.

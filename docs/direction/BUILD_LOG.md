# Direction build: log

Plan: [PLAN.md](PLAN.md). Agent rules: [AGENT-BRIEF.md](AGENT-BRIEF.md).

## Resume

- **Started:** 2026-11-10, from main at the commit that adds this file.
- **How work lands:** each item is a delegated agent in its own worktree and branch. It runs its
  own review loop with the `reviewer` model, then reports. The orchestrator reads the report,
  reviews boundary changes itself, merges the branch into the local branch `direction` (checked
  out at `~/code/projects/silo-direction`), regenerates the API files if two items touched them,
  and deletes the item's target folder (`~/code/projects/silo-wt-targets/<item>`). `main` on
  GitHub accepts only pull requests with seven required checks (found 2026-11-10), so the build
  stays on `direction` and goes up as pull requests at the end.
- **Live checks** use the Dev engine, one item at a time. On 2026-11-10 a Dev app and engine from a
  deleted build (`silo-dev-target`) were still running; an item that needs the Dev engine
  replaces them with its own build.
- **Models:** Sonnet implements; Opus for T1, T2 and T5 (they define the security boundary);
  `reviewer` (Codex gpt-6.1-sol high) reviews.

## Status

| Item | State | Branch | Notes |
| --- | --- | --- | --- |
| S1 | merged 2026-11-10 | `docs/s1-gateway-channel-spike` | [evidence](evidence/S1.md). Channel: yes, no patch. Allowlist: needs strict off, a `restore --net-strict` patch, and gateway init fields |
| T1 | running since 2026-11-10 | | task id ends `21b89a79-7c1e-432c-8118-44fc0eddcff6` (Opus) |
| T4 | merged 2026-11-10 | `feat/keep-conflict-choices` | engine tests 273, api 53, silod checkpoints 56 passed in the item; two review rounds; the `silo-ui` crate was not built there |
| T9 | running since 2026-11-10 | | task id ends `5af06c4c-ae51-47ff-a807-035a14df103b` (Sonnet) |
| T12 | running since 2026-11-10 | | task id ends `67f835b1-ca2e-4f89-9c93-4ad720bb45be` (Sonnet) |
| S2 | running since 2026-11-10 | | task id ends `c8ba35ed-17ae-4a93-b259-def2953e1398` (Sonnet), upgrade path spike |
| T7 | running since 2026-11-10 | | task id ends `b9b0ef15-b4e5-4f7f-8b3d-4c63559442bd` (Sonnet); no live macOS check in the item |

Task ids are `node:delegated-task:command%3Amcp%3Ad723019c-0dae-419c-8e5d-2e7d794e60f4%3Adelegate-task%3A<ending>`.
Next waves are in the plan's work items table; start an item only when the items before it in
its dependency chain are merged (T2 and T3 need T1; T5 needs T1, T2 and S1; T6 needs T4 and T2;
T10 needs T2 and T9; T8 needs S1 and T5; T13 needs T8, T10 and T12).

## Decisions taken while planning

These were not asked of the owner. Each is open to change.

1. **Fast-forward does not swap computers.** The direction first said "the fork becomes main".
   With the agent living in main, that would end its session and drop what main wrote since the
   fork (transcripts, history). Fast-forward now means "nothing can conflict, everything is
   applied". Swapping is the explicit Replace main. The direction was corrected.
2. **"Created by", not lineage, decides what a computer may touch.** A fork the owner made by hand
   can hold more access than its parent, so the parent's agent does not get it.
3. **The channel is a loopback port per computer plus a token**, not a new runtime patch, if
   spike S1 holds. The patch (gateway port to a Unix socket) is the fallback and a later hardening.
4. **The `silo` program goes inside the computer.** The direction said "nothing to install"; a
   program pushed by the engine at start gives agents the same command line and MCP server the
   device has, with no second implementation of the tools.
5. **A call that needs approval returns at once with an id** and the client waits by polling, so
   no connection is held open for minutes.
6. **Wire names stay** (`changes.keep`, `changes.promote`). Only what people and agents read is
   renamed. A wire rename can follow.
7. **Limits:** a safety floor on available memory and free disk for calls from computers, a
   warning before it, no counts. The owner asked for no hard caps.
8. **The macOS template keeps Claude Code** until its next revision, to avoid rebuilding every
   user's template now.
9. **Folder forks and `--pr` stay** as device-only commands. They belong to the old model but
   work without `.silo`; removing them is the owner's call.
10. **The hosted reviewer is hidden, not deleted.**
11. **First install of a macOS computer still uses Apple's network.** The disk is Apple's image and
    Silo's scripts; removing that window needs the bootstrap files inside the template.
12. **Not in this build:** the DNS query log, signing, the per-computer helper process for macOS,
    reflected-secret redaction, the `/.msb` review (the build removes Silo's use of the folder for
    requests, which narrows it).

## Found during the build

- **T4:** the `silo-ui` build script printed the contents of `github-build.local.json` into the
  agent's command output during a cargo build. Nothing was committed. The owner may want to
  rotate what that file holds, and the build script should stop printing it.
- **T4, existing behaviour left alone:** on macOS, files under `/etc` are normalised to
  `/private/etc` before the "main moved while starting" check, which may then miss them; a
  gitignored `.env` is carried over by a merge without a credential check (the wall covers Git
  history only); temp-file and temporary-ref cleanup errors in keep are ignored silently.
- **T4, for T6:** CLI and MCP text still says `divergent`; the new refusal reason is
  `choices-needed`, ids look like `file:<path>`, `package:<mgr>:<name>`, `repo:<path>`.

- **S1:** `msb modify` cannot change network rules; the supported way on an existing computer is
  the engine's restore in place (about 1.2 s idle). `--vsock` works and is a second fallback for
  the channel. The guest `silo` builds with `scripts/build-guest-silo.sh` in 36 s (12.7 MB,
  static); plain cargo fails because `ring` needs a C compiler. `current_network_args` rejects
  hostname rules, `restore` has no `--net-strict`, and `portable_export_network` refuses a
  computer whose strict is off. A memory-restored test computer reported a pinned `interface`
  that made export refuse; not checked whether the engine clears it.

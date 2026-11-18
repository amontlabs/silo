## Proposed

- Which agent runs
  - Rule: The engine, and no agent program inside the computer, starts the agent of each fork, so a vendor's nesting limits do not apply.
  - Rule: Silo never starts an agent program, model or provider the user did not choose, with no silent fallback and no default that spends on an account.
  - Wherever Silo starts an agent, the agent program and the model are a setting with a default, set separately for main and for forks.
  - Rule: A computer's agent program, provider, model or effort level changes in one step, and the work stays in the computer.
  - Property: Several agent programs from different vendors run unchanged in one computer at the same time, interactive or headless, each with its own sign-in, without interfering.
  - The first run lists the agent programs installed and signed in inside a computer, unranked, asks once which to use and remembers the answer.
- Sign-in and billing
  - Rule: Silo runs each vendor's agent program as shipped, signed in with the user's ordinary subscription, and requires no API key.
  - Rule: Silo ships no spoofed sign-in, no copy of a vendor's wire protocol and no billing workaround.
  - Before any agent starts, one preflight checks that the program is present, that its sign-in works in this computer and that its account is not rate limited.
  - Property: An agent program that is installed and not signed in is never reported as ready.
  - Rule: A failed preflight gives the exact reason in plain words that an agent can relay.
  - Silo warns when a computer holds both an API key and a subscription sign-in for one agent program, since the key is used first and changes the billing.
  - Rule: Retries are bounded, so a credential that cannot work stops the run, and the message names the agent program and the sign-in that failed.
  - Risk: An agent fixing a failed setup may switch the user to API-key billing; the preflight compares the sign-in in use with the one chosen and reports a change.
- Adapters and versions
  - Rule: Each agent program has one adapter behind an interface that is Silo's own and is not built on a third party's SDK.
  - Each adapter holds the program's launch definition: its flags, its output format and its resume flag.
  - A recipe pins the version of each agent program, so a rebuild or a restored checkpoint runs the version that worked.
  - Rule: What Silo knows about a model, such as its context size, is data that gets updated and is never a constant in the code.
- Checks
  - Test: In release checks, each supported agent program is installed in a fresh computer, starts, signs in and completes a first task, with one test per program.
  - Test: After each release of a supported agent program, a live check runs it headless in a fork from its launch definition, and it completes a task.
  - Test: The first run is done with only a Codex subscription, then with only a Claude subscription; each works, and no step assumes the other.
  - Test: The first run is done with a custom endpoint or a local model; the base URL is passed through and a placeholder API key is accepted.
  - Test: One agent program starts another headless in the same computer, and both finish their work.
  - Test: Claude Code is run inside a computer with the tricks it relies on, such as shell scripts that listen and wait; each works.
  - Test: An agent inside a computer installs and sets up each supported agent program, which arrives through `npx` with a Node runtime, and each then starts.
- Docs
  - The docs have one page per supported agent program that says how to run it in a computer with a subscription.
  - The docs on sign-ins state what Silo relies on for each agent program.
- Other
  - Rule: An agent session that Silo opens always shows that it runs inside a Silo computer, and under which rules.

## Open

- Does the engine start an agent headless in a fork, from a launch definition per agent program, or does this feature only install and open one?
  - Suggested: Yes, kept small: one launch definition per program lets the agent in main hand work to a fork without knowing each program's flags.
- After Claude Code and Codex, does Silo test and maintain more agent programs itself (the Codex app, pi, omp with tern), or leave them to contributed adapters?
  - Suggested: None more for now: the feature names these two, and each supported program adds a release test and a live check to keep passing.
- To find out: Do the terms of each supported vendor allow a subscription sign-in for its agent program running headless inside a computer?

## From the old code

The old code had it partly built, about 2,300 lines, and its approach can be reused.

Lessons:
- An install on an allowlist network fails; the error must say the network blocks it (V G1b).
- Install folders must be readable by the working account (Codex /opt/silo/codex was 0700).
- An allowlist network blocks agent install; say which hosts are needed.
- Never report missing for a computer Silo could not ask; that is unknown.
- On macOS, copy the staged file into a root-only folder before checking its hash, so the account cannot swap it.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/agents.rs`
- `app/SiloUI/src-tauri/engine/src/agents.rs`
- `docs/direction/evidence/T5b.txt`
- `docs/direction/evidence/V.txt`
- `app/SiloUI/src-tauri/guest/agent-tools.py`
- `app/SiloUI/src-tauri/guest/agent-tools-lock.json`
- `app/SiloUI/src-tauri/guest/macos/silo-agent-tools.zsh`
- `docs/product/JOURNEYS.md#L24`
- `docs/SiloUI-GUEST-IMAGES.md#L316`
- `app/SiloUI/src/features/agents/components/agents-section.tsx`
- `app/SiloUI/src/features/agents/model/use-computer-agents.ts`
- `app/SiloUI/src-tauri/src/commands/agents.rs`

Calls: `agents.list`, `agents.install`, `silo agents list`, `silo agents install`, `guest.sshAlias.ensure`, `terminal.command`.

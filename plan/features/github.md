## Proposed

- A repository can be given to a computer without access to its workflows, so that nothing can leave through them.
- The user can set a rule that refuses, or asks before, a push that rewrites the history of a protected branch.
- Property: The GitHub connection spreads its request budget across forks and backs off when GitHub limits it.
- A fork that GitHub's rate limit holds back says so and does not fail.
- Test: Before a preview or a release, sign in to GitHub from an account other than the one that owns the GitHub App; it works, and the GitHub App is public.
- One command clones a repository into a computer and gives that computer access to it.
- Rule: The engine counts its calls to GitHub's API and stays well under its rate limit, since every fork and task adds to them.

## Open

- Is holding issues and comments written by outsiders, until you let them through to the agent, part of this feature, a later one, or out?
  - Suggested: Later, with `triggers`, because `security-framing` already treats the agent as hostile, so the rules bound what an outsider's text can make it do.
- When a computer has its own GitHub identity, whose name do its commits carry: the computer's, yours, or both?
  - Suggested: The computer's, in line with `github-is-storage`, where agents post as a bot; you noted that commits could otherwise be farmed.
- Needs `secrets`, which comes later in the order: both use the same placeholder swap.

## From the old code

The old code had it built and driven end to end, about 10,800 lines, and it needs reworking.

Lessons:
- All-repositories mode with an empty installation catalog gave zero credentials and silent 401s (044 investigation).
- A fork gets only a read-only copy of GitHub access the caller's own access covers.
- A scoped token cannot mint another scoped token; all minting happens from the host's user token.
- 'All repositories' with an empty catalog produced zero grants yet said verified; success must mean an authenticated request worked.
- Never replay an ambiguous code exchange, refresh or mint; GitHub refresh tokens rotate and a consumed one is gone.
- A hosted server was thought necessary; GitHub's PKCE with a public client secret made a server-free flow possible.
- The shipped client secret is public by design; PKCE binds the code, so it is never proof of an authentic install.
- 'Verified' meant only that reconciliation finished; zero grants still showed success.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/github.rs`
- `app/SiloUI/src-tauri/engine/src/github.rs`
- `app/SiloUI/src-tauri/engine/src/github_tokens.rs`
- `docs/SiloUI-GITHUB-IMPLEMENTATION.md`
- `docs/SiloUI-GITHUB-044-AUTH-INVESTIGATION.md`
- `docs/SiloUI-GITHUB-IMPLEMENTATION.md#L190`
- `app/SiloUI/src-tauri/engine/src/github_http.rs`
- `docs/THREAT-MODEL.md#L317`
- `docs/SiloUI-GITHUB-SERVER-FREE-RESEARCH.md`
- `docs/evals/results/2026-11-03-74bd4f0a.md`
- `app/SiloUI/src/features/application/pages/github-page.tsx`
- `app/SiloUI/src/features/github/components/github-access-editor.tsx`

Calls: `github.state`, `github.connect`, `github.connect.callback`, `github.connect.reopen`, `github.disconnect`, `github.access.set`, `github.configuration.save`, `github.configuration.retry`, `github.repositories.manage`, `github.repositories.refresh`, `computers.access`, `computers.identities.configure`, `computers.identities.verify`.

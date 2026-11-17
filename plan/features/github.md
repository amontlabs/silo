## Open points

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

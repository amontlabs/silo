## Decided

- 2026-11-10, owner: personal GitHub tokens are kept and become ordinary secrets bound to GitHub's hosts, several allowed, assigned per computer.

## From the old code

The old code had it built, about 1,400 lines, and it would be written again.

Lessons:
- An agent is never bound to a token by default; token access names the token secret by id.
- Only a 401 or a bad-credentials 403 disconnects a token; a rate limit or timeout keeps the previous status.
- Silo cannot narrow a personal token: the agent gets its full permissions.
- A personal token carries all its permissions by design; repository scoping exists only on the signed-in GitHub App path.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/github.rs`
- `app/SiloUI/src-tauri/engine/src/github_personal_token.rs`
- `docs/SiloUI-GITHUB-PERSONAL-TOKENS.md`
- `app/SiloUI/src-tauri/engine/src/github.rs`
- `app/SiloUI/src-tauri/engine/src/secrets.rs`
- `docs/ROADMAP.md#L487`

Calls: `github.personalToken.save`, `github.personalToken.remove`, `secrets.save`, `github.configuration.save`.

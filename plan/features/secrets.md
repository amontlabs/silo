## From the old code

The old code had it partly built, about 5,600 lines, and it needs reworking.

Lessons:
- TLS must be intercepted only for hosts a secret is allowed for, or agents' own provider calls fail certificate checks (ZCODE-TLS).
- An unmatched placeholder must pass through unchanged: blocking it stalled agents that echoed it into model requests (AGENT-PLACEHOLDER-STALL).
- Values go to the runtime on stdin, never in arguments, config files or the environment.
- Intercept TLS only for hosts a secret is allowed for; intercepting everything broke agents that drop inherited CA settings (ZCode).
- An allowed host that echoes headers reveals the value; restrict hosts to services you trust.
- Values reach the runtime on stdin, never in arguments, config or environment; secret names that would override proxy or loader variables are refused.
- An unchanged placeholder an agent echoes in a request must pass through, or the TLS connection closes and every retry stalls.
- Dev rebuilds lose Keychain access, which blocked every live secret check.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/secrets.rs`
- `app/SiloUI/api/src/methods/credentials_used.rs`
- `app/SiloUI/src-tauri/engine/src/secrets.rs`
- `app/SiloUI/src-tauri/engine/src/secrets_runtime.rs`
- `docs/SiloUI-SECRETS.md`
- `docs/SiloUI-SECRETS.md#L237`
- `app/SiloUI/patches/microsandbox-secret-values-stdin-0.7.14.patch`
- `docs/THREAT-MODEL.md#L242`
- `docs/evals/results/2026-11-03-0dae8805.md`
- `docs/SiloUI-CLI.md#approvals`
- `app/SiloUI/cli/src/cli.rs`
- `app/SiloUI/src/features/application/pages/secrets-page.tsx`

Calls: `secrets.list`, `secrets.save`, `secrets.remove`, `secrets.retry`, `computers.credentials`, `silo secrets`, `silo secrets set`, `silo secrets assign`, `silo secrets list`, `silo secrets unassign`, `engine.credentials.status`, `engine.credentials.repair`.

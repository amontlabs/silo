## Proposed, not confirmed by the owner

### For whoever builds it

- What always holds
  - Property: Nothing run inside a computer yields a secret's real value, whether the agent program's file tool, the shell, a script or a binary placed inside.
  - Property: In the engine, the client and the logs, a secret value is held in a type that cannot be printed, formatted or serialised.
  - Property: Command output returned to a caller and transcripts exported from a computer contain no real secret value; any that appears is redacted.
- Entering and handing out
  - Rule: A secret's value is entered in Silo's own prompt, on the same trusted path as an approval and outside any chat, so the agent learns only its name.
  - Detail: A secret given to a computer or a fork can have an end time, after which it is taken back without its value ever being shown.
  - Detail: When Silo withholds or changes a credential, it says so in one plain line with the reason.
  - Detail: A secret is checked against common formats when entered, such as Stripe API keys, OAuth tokens and Postgres connection strings.
  - Rule: A payment credential, such as a wallet key or a card, is held as a secret like any other.
  - Rule: This feature builds no integration for a named service; an outside service is used through one secret and one allowed host.
- Credentials that change
  - Detail: Substitution keeps working for a credential that lasts two hours, a sign-in that needs a person, and a rotated value.
  - Find out whether a subscription's browser sign-in, with its callback or copy-link flow and refreshed tokens, works with substitution at all.
  - Find out how a sign-in that needs a hardware key held by a person (FIDO2) gets into a computer.
- Docs
  - To find out: Check against the real design whether the process that holds real secret values is separate from the one that runs the VMs and from every computer.
  - The docs say which process holds the real secret values, and whether it is the process that runs the VMs.
  - The docs say that substitution happens in the network stack that carries a computer's requests, and that no proxy is configured inside the computer.
  - The docs show one worked example, Linear with a scoped key, and the pattern behind it.
  - The docs say that file permissions cannot keep a secret from an agent, because they need a second user, Git does not carry them, and macOS behaves differently.
- Tests
  - Test: After a run, search every log, event and journal entry for the real value of each secret; none appears.
  - Test: Run a local balancer that holds several accounts inside a computer under its network rules; the real credentials never appear inside.
- Other
  - Detail: A secret can be limited to reading, for example sent only with GET requests to its hosts, so an agent can read a chat and cannot post to it.
  - Property: Swapping a secret into a request to a model provider leaves the rest of the request unchanged byte for byte, so the provider's prompt cache still hits.
  - To find out: Read fucory's MicroSandbox fork with the iron-proxy features and how Centaur injects secrets from outside the sandbox, and record why iron-proxy was not adopted.

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

# Codex boundary review: in-guest channel and network policy (T5, T8, MicroSandbox 0.7.14)

Reviewer: lastdrop `reviewer`, ran on Codex gpt-6.1-sol (high). Read-only, static review of
`direction` at `672b40b31` on 2026-11-12. No edits, tests or engine launches.

## Findings

1. **High: reconciliation can exhaust the device's disk.** `engine/src/guest/process.rs:158`
   copies stderr into a temporary file without a byte limit. Guest root can replace a command
   used by endpoint checks or reconciliation with an endless stderr writer, filling host storage
   and disrupting the engine and other computers. The timeout limits duration, not disk use.
   Fix: cap stderr while collecting it, terminate on overflow, enforce an aggregate spool budget.
2. **High: rebuild temporarily removes network restrictions.** `engine/src/rebuild/engine_env.rs:777`
   creates the staging computer under a new id without copying its network policy, so it
   defaults to open. `rebuild/mod.rs:443` then replays the recipe after carrying home and
   workspace; a modified project script the recipe runs can send data to an unlisted host.
   Fix: persist the source's effective policy and preset pins under the staging id before creation.
3. **High: macOS imports default to open.** `engine/src/macos_computers.rs:3677` publishes the
   imported computer without saving a closed network policy; `network_policy/mod.rs:1044` treats
   an absent record as open. Fix: save closed policy before publication and keep it through
   import recovery.
4. **Medium: scrubbed exports keep a valid source credential.** `engine/src/backup_controller.rs:1745`
   skips token revocation for sources exported through hidden copies. Deleting the endpoint file
   does not erase its bytes from backing layers (as `export_scrub.rs` documents), and guest root
   can copy the token into a workspace file beforehand. Fix: revoke the source token for every
   export, including scrubbed copies; propagate revocation failures.
5. **Medium: a default-policy `-undo` keeps the original channel exception.**
   `rebuild/engine_env.rs:914` inherits no policy record when the original had none;
   `runtime/checkpoints.rs:1743` then skips network reconciliation because undo has neither a
   stored policy nor tools. Its runtime keeps the original gateway-port rule; after the original
   is deleted and the port released, undo can reach another device service that binds it.
   Fix: persist an explicit undo policy without the original channel rule before undo can start.
6. **Medium: GitHub grants bypass preset-version pinning.** `network_policy/mod.rs:650` adds the
   current GitHub preset's hosts directly, so a future preset expansion widens existing
   allowlist computers with GitHub grants without accepting a preset update. Fix: pin
   grant-derived host lists too and expose expansions for approval.
7. **Medium: the public-suffix guard accepts omitted shared suffixes.** `network_policy/mod.rs:400`
   checks only a small bundled set; `*.duckdns.org` passes although `duckdns.org` is on the
   Public Suffix List. Fix: use a maintained PSL implementation covering ICANN and private suffixes.

## What holds

Listener identity fixes the computer caller; fresh HMAC proofs bind authentication to nonce and
port; client fields cannot elevate it to owner. Ordinary forks get separate ports, and copied
parent credentials cannot authenticate on those ports. Reconciliation uses fixed guest commands
and validated host record names. Ordinary Linux fork and restore reapply policy, Linux imports
save closed policy, and widening requests pass through the approval gate. JSON 403 responders do
not dial the denied destination.

Documented limits remain: allowlist DNS carries arbitrary query names; `strict=false` permits
shared-address and domain-fronting exceptions; device processes can occupy handshake slots;
taken channel ports stay guest-reachable until restart. Same-computer restore can keep its
current token, so restore is not credential revocation.

Fix the rebuild policy inheritance before exercising another restricted-computer rebuild.

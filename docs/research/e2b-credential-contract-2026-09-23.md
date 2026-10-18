# E2B credential contract audit

Date: 2026-09-23. Static source audit only. No provider request, real token use,
VM/container/service mutation, or credential value was used. The PoC was not run.

## Finding

Silo's current authorization contract cannot be replaced by the PoC broker as it
stands. The PoC establishes a useful synthetic tracer for a single HTTP/1.1
request path through a per-workspace reverse tunnel. It does not yet reproduce
Silo's supported authentication modes, all generic-secret behavior, current
grant reconciliation, or revocation/replay guarantees. The largest qualification
gap is that the snapshot test accepts either transport failure or 403 after
restore, and has no simultaneous successful request proving the restored
workspace uses current authority.

The trusted owner is the Silo native process and its secure credential store.
Guest root and snapshot bytes are untrusted. In the proposed E2B integration,
the credential owner/broker must remain outside the guest and map each current
runtime incarnation to current workspace policy before granting egress.

## Current Silo contract

| Path | Supported client / destination | Operation and permission | Credential boundary and update behavior |
| --- | --- | --- | --- |
| GitHub App OAuth | Guest `git`, Git LFS, `gh`, and HTTPS clients using GitHub Authorization; destinations `github.com`, `api.github.com`, `uploads.github.com` | Workspace policy chooses selected repositories or all repositories authorized to the installed App. Default is read-only. Per-repository `allowPushes`, or explicit all-repository changes, supplies write authority. Provider-issued child tokens are grouped by owner and read/write scope. Host Push gets its own explicit one-repository write token and does not enable guest writes. | Parent OAuth access/refresh credentials remain in Keychain/Secret Service. Native code mints expiring repository-scoped children and gives the runtime profile to the host runtime command through `SILO_GITHUB`, not argv/config/logs. Guest receives `$MSB_SILO_GITHUB`; the bundled patched proxy substitutes only the exact Authorization placeholder under verified TLS, including Git Basic and Bearer. Request bodies, queries, unrelated headers, parent-token fallback, and redirects are not substitution channels. Policy removal narrows locally before replacement grants are minted; token changes close active proxy connections. |
| GitHub personal token | Guest Git/Git LFS/`gh` and GitHub HTTPS API through the same fixed GitHub hosts | Classic and fine-grained PATs are accepted after read-only `/user` validation. In token mode the token's full provider-granted authority applies; repository and read/write OAuth controls are hidden. It is not silently replaced by OAuth. | PAT stays in a separate secure-store entry. Runtime uses profile v2, exact Authorization-header substitution for Bearer/Basic, no body/query substitution, and live update/connection invalidation where the VM proxy supports it. Removal detaches locally; Silo does not revoke the PAT at GitHub. Incompatible proxy versions must restart before token mode can be reported applied. |
| Generic secrets | Applications that send an assigned placeholder in proxied HTTPS requests to assigned hosts and trust the sandbox CA | User-selected workspace assignment and exact host, wildcard host, or explicitly acknowledged `*`. A secret may serve any operation understood by that application; Silo does not impose GitHub repository or HTTP-method policy. Matching TLS traffic can receive the value. | Values live in Keychain/Secret Service; JSON stores immutable references and public policy/status. Runtime config stores host source references and `$MSB_NAME`, not plaintext. Existing-value rotation, domain changes and removal apply live and close connections; a newly added variable on a running VM is deferred to next boot. Start resolves current assignments. A matching server can reflect or otherwise return a secret; arbitrary output is not universally scrubbed. Private signing keys are not supported by this substitution mechanism. |

OAuth scope construction is per GitHub owner and repository IDs. Unknown or
non-repository App permissions fail closed. All mode means all repositories
currently/future authorized to that App, not every repository on GitHub. OAuth
and PAT authority are intentionally different: PAT mode carries the PAT's full
granted scope. GitHub native control-plane calls are made directly by the Rust
backend to fixed HTTPS GitHub destinations, with redirects disabled, bounded
response/time limits, and redacted errors.

Source references: [GitHub implementation plan](../SiloUI-GITHUB-IMPLEMENTATION.md),
[personal token contract](../SiloUI-GITHUB-PERSONAL-TOKENS.md),
[generic secret contract](../SiloUI-SECRETS.md),
[github.rs](../../app/SiloUI/src-tauri/engine/src/github.rs),
[github_tokens.rs](../../app/SiloUI/src-tauri/engine/src/github_tokens.rs),
[github_personal_token.rs](../../app/SiloUI/src-tauri/engine/src/github_personal_token.rs),
[github_http.rs](../../app/SiloUI/src-tauri/engine/src/github_http.rs),
[secrets.rs](../../app/SiloUI/src-tauri/engine/src/secrets.rs),
[secrets_runtime.rs](../../app/SiloUI/src-tauri/engine/src/secrets_runtime.rs), and
[runtime.rs](../../app/SiloUI/src-tauri/engine/src/runtime/mod.rs). The native runtime
adapter sends only the workspace's current grant profile through a host-only
environment reference while applying it and checks the runtime update before
caching it.

## PoC gap matrix

| Contract area | PoC behavior inspected | Gap / consequence |
| --- | --- | --- |
| Placeholder substitution | Supports one literal `$SILO_GITHUB` placeholder as Bearer or Git Basic; compares the complete Authorization value before substituting. | This is not Silo's `$MSB_SILO_GITHUB` runtime profile contract. It does not exercise the patched MicroSandbox placeholder protocol or prove interoperability with the eventual E2B data path. |
| General secrets | None in the credential broker. | No arbitrary named secrets, host allowlist semantics, generic TLS substitution, or current host-secret-store mapping. Keep this explicitly out of the PoC claim. |
| Grant shape | Each in-memory `Grant` contains one credential, repo-name map and enabled bit. Two broker listeners are created, one per selected existing desktop. | No owner identity, repository IDs, App permission set, OAuth child-token mint/refresh/expiry, all-repository mode, personal-token mode, or Host Push distinction. There is no durable current-policy lookup on each request. |
| Destinations | HTTPS CONNECT allows only `github-fixture.test` and `api.github-fixture.test` at port 443. Host header is checked; upstream TLS verifies with the fixture cert; forwarding headers are allowlisted. | Good fixture containment, but no real GitHub redirect/LFS object host, DNS rebinding, IPv4/IPv6, CONNECT variants, proxy bypass/direct egress, trust-store behavior, or production hostname/scheme/port policy. |
| Operations | Small custom path/method recognizer for Git smart HTTP, LFS and a REST-shaped `/repos/fixture/...` path. GraphQL is rejected. Bodies are capped at 32 MiB. | This is a tracer allowlist, not Silo's GitHub grant contract. Methods/path shapes, duplicate/conflicting auth, query parsing, redirect handling, streaming, slow clients, cancellation, malformed framing and response-size behavior are underqualified. The receiver reads a declared body before policy checks. |
| Positive receipt | Fixture upstream computes SHA-256 of Authorization token and a response can carry the digest; Git commit/LFS blob are independently checked. | No durable independent receipt record with per-request/run correlation. Shared in-memory `SEEN` and `SEEN[-1]` can associate the wrong receipt under concurrency. The qualification does not assert preserved unrelated headers/body. HTTP 200 by itself is not sufficient. |
| Negative controls | Wrong placeholder, denied repo, wrong origin, spoofed workspace header, GraphQL and raw reflected token cases are asserted. | Denials are not paired consistently with independently verified absence of an upstream receipt. Repo parsing is fixture-specific. No per-operation test for every accepted scope, alternate upload host or request identity override. |
| Two workspaces | Separate listeners and `Grant` instances; workspace B attempts to claim A in a header and is denied on its own listener. | Current-workspace binding depends on listener/tunnel topology, not an authenticated workspace/runtime claim checked by the broker. There is no cross-routing/port/path/runtime-ID adversarial matrix or simultaneous interleaved positive receipt test. |
| Rotation and revocation | In-memory grant rotation changes subsequent fixture receipt; `revoke()` makes later requests 403. | Rotation is not tied to Silo's persisted policy revisions or secure store. Revoke serializes only authorization selection; an already-authorized forwarded request/stream continues. No idle/concurrent/in-flight/slow stream semantics are defined or tested. |
| Checkpoint/revert | Makes checkpoint after revoking the grant; after revert accepts either transport failure or 403 and checks runtime ID changed. | This does not prove current-authority replay. It lacks a successful allowed control from restored state, an explicit no-receipt oracle for the old token, source/fork identity separation, grant change while checkpointed, and egress quarantine until policy rebind. The reverse tunnel may simply be broken after replacement. |
| Restart/failure | Grants and credentials are process-memory objects; finalizer terminates SSH tunnel and stops broker servers. | Process restart likely loses the grant, but no fail-closed restart test or owner/broker restart case is run. Tunnel identity, stale-forward cleanup, owner loss, host loss and recovery are unqualified. |
| Secret leakage | Broker avoids access logs and rejects raw/base64 credential reflection in a bounded response; test scans `/home/user` for credential bytes. | The scan excludes guest memory, process buffers, swap, caches and logs outside home; the response filter cannot detect arbitrary encodings or cooperating-origin exfiltration. State the allowed-origin trust model; do not claim universal anti-exfiltration. |

The PoC currently tests curl Bearer and Git Basic, Git smart HTTP push/clone,
LFS upload/download, one REST-shaped path, and GraphQL refusal against
synthetic fixture credentials. It does not exercise `gh`, real GitHub App or PAT
flows, generic secrets, or real provider permissions. Those are unrun, not
implicitly passed by the Silo implementation's separate historical tests.

## Smallest controlled fixture plan

Run this only later on a fresh run-owned fixture, with synthetic values and no
provider account. Keep it independent of the preserved incident environment.

1. Create two isolated workspaces A/B and one TLS recording upstream. Use
   unique random synthetic tokens for each workspace. The upstream stores only
   `{run_id, request_id, host, path, method, auth_scheme, sha256(received_token),
   body_sha256}` in a protected append-only test receipt file, flushes and fsyncs
   each record, and never writes raw Authorization. Validate the digest against
   the expected synthetic token from the host-side fixture process. This server
   is an independent oracle: the broker cannot manufacture its receipt.
2. Grant A read on fixture repo A and write on B; deny C. Grant B a different
   token and a separate scope. Interleave positive and negative curl Bearer,
   Git Basic, Git smart HTTP clone/fetch/push and LFS batch/object upload and
   download requests. Check commit/object hashes and receipt digests per request.
   Try guest-selected workspace/runtime headers, placeholders, paths, proxy
   ports, alternate hostnames, duplicate auth headers, redirects and GraphQL.
   A denied request must have no matching receipt.
3. Add a tiny `gh` REST request against the fixture. Keep GraphQL explicitly
   denied until a fixture can enforce the same GitHub-issued permission boundary;
   do not promote a custom GraphQL policy engine from this experiment.
4. Rotate A's grant while idle, with concurrent new requests, and during an
   intentionally slow upload. Record the precise boundary: queued/new requests
   use the new digest, while any request already admitted is either allowed to
   finish or actively canceled according to an explicit policy. Revoke A and
   require denied positive-control connectivity plus zero new A receipts. Test
   broker/owner loss as fail closed.
5. Checkpoint a guest with only placeholders. Change A's grant, then revert and
   fork. Before allowing egress, bind each new runtime incarnation to its
   current workspace and policy. For each restored runtime, require both a
   successful current-token request with a matching new digest and a denied old
   token request with no corresponding receipt. Verify source/fork tunnel IDs,
   owner keys and grants are distinct. A transport error does not count as a
   policy denial.
6. Exercise one generic-secret positive and wrong-host negative as a separate
   path only after the broker demonstrates exact current secret assignment and
   host rules. Do not infer generic-secret support from GitHub's special path.

Capture run ID, UTC timestamps, host/runtime/template revisions, request IDs,
policy revision, runtime IDs, tunnel IDs, bounded request classes and receipt
digests. Store raw fixture evidence under the ignored verification tree; track
only redacted summaries. No live token or real-provider traffic belongs in this
fixture.

## Trust boundary and revocation questions

The source PoC's claimed binding is: one host-created reverse SSH tunnel to one
guest, mapped to one loopback broker listener with a fixed in-memory grant.
Guest headers do not select the workspace. That is a reasonable fixture seam,
but the current code does not establish the full production trust boundary.

- Who authenticates and owns the reverse tunnel endpoint? Is its private key
  unique per workspace/runtime incarnation, and is the broker listener bound to
  that identity instead of a reusable local port?
- On revert/fork, can any pre-snapshot SSH process, tunnel, port forward, or
  cached broker session survive and reach the prior grant? What exact operation
  revokes that route before the replacement is allowed network egress?
- Does the broker derive authority from the owner-side current grant on each
  request, or trust a copied/static grant object? What happens on policy-store
  lock, owner disconnect, version mismatch, broker restart or stale runtime ID?
- What is the revocation linearization point? In the current tracer, it is the
  lock-protected check/copy of the credential. Requests admitted before revoke
  can continue forwarding afterward. Is that acceptable for a body already in
  flight, or must active sockets be canceled? Define and test both.
- Does rotation invalidate cached authorization, TLS tunnels, retry bodies,
  client credential helpers, diagnostics and credentials retained by the guest?
  How are already-delivered requests and arbitrary allowed-origin reflection
  bounded in product language?
- Is policy installed and acknowledged before a restored/forked guest process
  can make network requests? The current PoC accepts a dead tunnel as a replay
  denial; this is not proof of current-policy application or usable egress.
- Does viewer ownership/mode reuse an epoch or cleanup path that can accidentally
  terminate an authorized credential tunnel? Conversely, can viewer code access
  the broker listener or owner control API through same-origin guest content?

Until these questions have evidence, classify PoC credential substitution as
**synthetic fixture behavior only**. The cutover gate remains open.

## Audit evidence and limits

Read completely: repository `AGENTS.md`; all 847 lines of
`docs/SiloUI-E2B-QUALIFICATION-HANDOFF.md`; PoC `credential-broker.py`,
`credential-qualification.py`, and `runtime.py`; product-facing GitHub,
personal-token and generic-secret contract docs. Reviewed the relevant
authorization, scope, credential-store, runtime-delivery and fixture-test
sections of `github.rs`, `github_tokens.rs`, `github_personal_token.rs`,
`github_http.rs`, `github_live_tests.rs`, `secrets.rs`, and `secrets_runtime.rs`.

Commands used were `cat`, `sed -n`, `nl -ba`, `wc -l`, `rg -n`, and `rg --files`
against tracked source/docs only. No test, Python PoC, browser, SDK, VM, provider
request, or credential-store operation was run.

Claims still untested in this audit: runtime behavior of the PoC, active stream
revocation under load, actual reverse-tunnel authorization/rebind behavior,
snapshot/fork interactions, independent receipt persistence, all denial cases,
real GitHub OAuth/PAT behavior through E2B, and E2B support for generic secret
substitution. The next test is the fresh two-workspace synthetic recording
upstream fixture above, with a positive restored-state control and per-request
independent digest receipts. Only after that deterministic seam passes should
real-provider authorization be considered.

## Subsequent fixture result

A separate 2026-09-23 local TLS loopback test added an independent, fsynced
JSONL receipt containing request ID, destination, method, path, authorization
scheme, and token/body digests only. `test_credential_receipts.py` passed one
allowed Bearer and one allowed Git Basic request with matching synthetic-token
digests. A wrong-workspace/repository request was denied and produced no
matching upstream receipt. The first test failed because the PoC broker
discarded `X-Fixture-Request-ID`; the narrow fix forwarded that fixture header.
This is a controlled single-workspace fixture result. It does not satisfy the
two-workspace, current-grant replay, generic-secret, or real-provider gates.

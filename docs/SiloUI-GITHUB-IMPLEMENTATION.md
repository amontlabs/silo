# GitHub implementation plan

2026-09-09. User authorized implementation, parallel subagents/review, removal of
the POC/obsolete designs, and an **All repositories** option in Silo. This is the
single current plan.

## Behavior and scope

Connect in the browser through the Silo GitHub App, authorize selected/all GitHub
repositories, return to Silo, and choose selected/all repositories for each computer.
**All repositories** means all repositories the connected App/user can access,
including future authorized repositories, never arbitrary private repositories.
A successful connection enables GitHub access automatically; each computer still needs
a repository selection. Explicitly disabling access persists until another connection
or enable action. Default to read-only; **Allow GitHub changes** covers repository Git/API writes.
Selected mode retains per-repository write controls; all mode has an explicit
read-only default and an all-repository write choice. No unrelated UI changes.

Run standard Git/LFS/gh. GitHub-issued user tokens enforce selected repositories
and read/write groups per owner. No command allowlist, broad-token fallback or
GraphQL permission engine. Queries and mutations use their corresponding narrow
authority. The user accepts combined requests crossing credential boundaries can
fail; never automatically replay or split mutations after partial success.

## Native implementation

Silo connects directly to GitHub from its Rust backend. There is no hosted Silo
service, Node sidecar or token-bearing WebView request. The public native-client
App configuration includes a client ID and client secret; this is not a private
key. User access/refresh credentials remain in Keychain or Secret Service.

- `github.rs`: external-browser OAuth, S256 PKCE, random state, temporary loopback
  callback, secure account storage, real installation catalog, durable computer policy,
  truthful progress/error events and main-window-only mutations.
- `github_tokens.rs`: direct code exchange, refresh, restricted-token creation,
  individual-token retirement and whole-authorization revocation. Repository-only
  permission validation and read/write maps are explicit. Unknown grants fail.
  Rejected code-exchange sessions attempt individual-token retirement before
  returning an error. GitHub [omits refresh and expiry fields when user-token
  expiration is disabled](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app);
  Silo requires rotating sessions. Cleanup targets the new token so an existing
  connection that shares its authorization remains valid.
- `github_http.rs`: fixed GitHub HTTPS destinations, no redirects, bounded response
  size/timeouts, redacted errors and retry gates. OAuth errors returned with HTTP
  200 fail. Empty HTTP 204 revocations succeed; already-absent 404 is idempotent.
- The existing MicroSandbox patch attaches only scoped credentials to the exact
  Authorization placeholder under verified TLS. It never substitutes the GitHub
  token into arbitrary bodies/query parameters or uses the parent token as a
  fallback. For opaque GraphQL IDs, bounded restricted read probes resolve the
  owner before forwarding the original request once.
- Guest Git/gh/LFS tools are baked into the bundled Ubuntu 24.04 image, built
  from signed Ubuntu packages. Selected/all controls and live Git/jj author
  updates remain.
- Explicit host Push transfers committed Git/LFS objects into isolated bundled
  host Git. Guest hooks/configuration and uncommitted files do not become host
  configuration or committed data. No guest write permission is temporarily added.

The hosted-service source, package, Docker image configuration and test harness
were removed. Its applicable request contracts now run as native tests. No UI
layout, labels or controls were changed in the server-free conversion.

## Live edits and failures

Repository choices save immediately. Additions and write enablement are combined
for 500 ms after the latest edit. Removals detach affected authority locally before
network work. Failed local updates remain errors rather than claiming removal.
Invalid remaining selections also detach old grants before reporting the error.

Only changed owner/read/write groups request new credentials. Unchanged scopes
reuse unexpired tokens. Superseded network results cannot attach to a newer policy.
Starting a computer and replacing its profile are ordered per computer. Applying a new profile
closes existing proxy connections without rebooting the computer. A read-token change
can briefly pause writes for the same owner while its paired profile is rebuilt.
Git identity changes do not request tokens or wait for GitHub. The repository
catalog refreshes every five minutes for all connected, enabled accounts, including
selected mode and accounts with no computer grants yet. Previously it refreshed only for
All repositories, leaving newly authorized repositories absent from the picker.

Confirmed rate limits honor Retry-After and exhausted x-ratelimit-reset with
exponential backoff and positive jitter. Automatic retries are bounded at five;
explicit Retry retains GitHub's imposed waiting period, including after relaunch.
Ambiguous code exchange, refresh or mint outcomes are not automatically replayed.
Safe reads and idempotent revocations can retry. Guest writes are never replayed.
Repository Refresh resets only safe operations after acquiring the GitHub operation
lock. Stopped token mints and rotating OAuth refreshes require their explicit Retry;
refreshing the catalog preserves their refusal and every server waiting deadline.

When refresh succeeds but secure storage fails, the renewed credential is retained
in host memory for storage retry, tied to the original account token. The consumed
refresh token is not submitted again. Disconnect uses the renewed credential if
necessary. If the process dies before that credential can be saved, reconnect can
be required; no plaintext fallback is used.

Public snapshots never read Keychain/Secret Service. They use only a process-local
observation of credential expiry, absence or a safe error. The existing worker
performs secure-store operations and publishes changes; no token bytes enter that
observation. Initial permission waiting uses the existing unavailable notice and
disables retry. Routine reads retain previously verified unexpired state until a
result arrives. This prevents a Keychain permission dialog from freezing the entire
app's initial state load. A blocked-reader regression verifies the boundary.

The callback parser reads bounded complete headers across fragmented input and
ignores unrelated local traffic. Invalid authenticated responses, empty codes,
wrong state, duplicate codes and declined authorization cannot create a session.

## GitHub App and build configuration

Reuse **microsandbox-workspaces**, owned by **0xpolarzero**:

- App ID: `4605731`.
- Public client ID: `Iv23liEjp3VnGe0sw2LU`.
- Callback: `http://127.0.0.1/github/callback`, wildcard matching disabled.
- Device Flow disabled for the desktop PKCE flow.
- Expiring user tokens already enabled.
- Repository permissions only; organization/account permissions remain absent.

The callback and Device Flow changes were saved and verified in authenticated Zen
on 2026-09-09. The user generated the client secret; the native macOS build was
configured and real browser sign-in succeeded as the existing account. The value
was not committed or written to test output.
No private key is required and none must be bundled.

Native builds load `SILO_GITHUB_CLIENT_ID`, `SILO_GITHUB_CLIENT_SECRET` and
`SILO_GITHUB_APP_SLUG` from the ignored `app/SiloUI/github-build.local.json`.
Explicit environment variables override the file; the rolling release workflow
supplies them through repository Actions secrets. These are native compile-time
settings. `build.rs` tracks both the file and environment changes and rejects
missing or invalid values. See [local setup and release configuration](SiloUI-RELEASES.md).
The client-secret value is extractable from a distributed native binary and is
not treated as a confidential security boundary. Do not commit actual values to
source or confuse them with users' bearer tokens. There is no service URL setting.

Client IDs can contain dots: GitHub's [App response](https://docs.github.com/en/rest/apps/apps#get-the-authenticated-app)
documents `Iv1.ab1112223334445c`. Native token operations accept both that format
and the current alphanumeric format, while rejecting path separators, control
characters and standalone `.` or `..` URL path segments.

The [user installation endpoint](https://docs.github.com/en/rest/apps/installations#list-app-installations-accessible-to-the-user-access-token)
lists installations of the authenticated token's App. Its installation `client_id`
is optional in GitHub's [OpenAPI installation schema](https://github.com/github/rest-api-description/blob/main/descriptions/api.github.com/api.github.com.json).
Scoping accepts an omitted ID and rejects a mismatched or malformed ID when present;
owner, suspension and repository permission checks still apply.

GitHub distinguishes [incorrect App credentials from an invalid refresh token](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app#troubleshooting).
Only `bad_refresh_token` tells the disconnect flow that its stored refresh
credential cannot renew. Other OAuth errors keep revocation pending and retain
the credential, rather than reporting a completed disconnect after a configuration
failure. Provider error descriptions are never included in diagnostics.

## Verification

Latest completed checks for the server-free conversion:

- `npm --prefix app/SiloUI test`: 484 passed across 52 files.
- `npm --prefix app/SiloUI run lint`: passed.
- `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml --offline -- --test-threads=1`:
  198 passed, 4 ignored, including permission-denial and blocked-Keychain regressions.
  The final worker lock adjustment also passed all 34 focused GitHub tests.
- Direct native operations: 8 contract tests and 12 transport/retry tests,
  including actual private-loopback HTTP responses.
- The authenticated native harness and nested real-VM regression passed together
  in 68.91 seconds, then again in 71.02 seconds with stricter denial assertions.
  Three private synthetic fixtures were explicitly authorized for the existing App.
  No working repository was used for mutations.
- `npm --prefix app/SiloUI run desktop:build:debug`, with native App configuration:
  the ad-hoc signed macOS bundle built successfully and completed browser sign-in.
  Existing bundle-size and unused-function warnings remain; notarization was not
  requested.

The existing hardware regression previously passed through actual production
Start/Restart in 24.50 seconds using a synthetic rejected credential and removed
its own VM. The runtime patch has not changed during this conversion, so that is
retained evidence rather than a newly claimed hardware run. Patch SHA:
`e6868dfbef5e7800949adbad98bda7fa501e8df47ac7146e601b50f145349ee8`.

### Authenticated evidence and repeatable run

Follow `app/SiloUI/tests/live/README.md`. The native test uses actual production
operations against three explicit private fixtures: read A, write B, deny C.
It checks REST/GraphQL boundaries, node-ID mutations, child re-scoping/reset,
All repositories and individual-token revocation without losing the parent.
Optional computer execution tests real Git, gh, LFS bytes and live access changes using
only scoped credentials in the native child process.

Observed with real GitHub on 2026-09-09:

- Browser Connect/return completed and showed the real connected account.
- Final configured macOS bundle reopened normally with the real dev/playgrounds
  VMs running and personal stopped. GitHub showed Connected as @0xpolarzero;
  the personal repository picker listed all six authorized repositories, including
  the three new private fixtures. No sandbox grant was selected during UI checks.
  The app was left open on GitHub. The earlier instance was closed through its
  Quit menu and its process exit verified before relaunch.
- The prior rebuilt instance's loading stall was traced to Keychain with a brief
  process stack sample. The final build loaded normally; the deterministic blocked
  credential regression separately verifies pending authorization cannot block
  public snapshots. No system permission was bypassed.
- Read tokens accessed fixtures A/B and received explicit permission denial for C.
  Write tokens accessed B and were denied A/C. REST and GraphQL were checked,
  including opaque node IDs and unchanged issue data after denied writes.
- GitHub refused child-token minting with HTTP 401 and the exact message
  “A scoped token cannot create another scoped token.” Token reset did not expand
  access. Rate limits, transport failures and unknown responses were not proof.
- All-repositories tokens accessed all three authorized fixtures. Retiring one
  child left the parent and sibling usable. Every tracked test child was revoked.
- The actual VM cloned/fetched/pushed Git, used gh and round-tripped 1 MiB through
  Git LFS. Real bearer tokens were absent from guest environment/Git configuration.
  Write removal, full removal and restoration took effect with unchanged boot ID.
- Both passing runs removed their disposable VM and branch. Four marked test
  issues, including diagnostic runs, were closed; only the main branch remained.
  Private fixture repositories and uploaded LFS test objects remain for evidence.

The first run timed out and was not counted as success. Subsequent diagnostic
runs exposed incorrect test assumptions about GitHub's 403 repository denial and
401 scoped-parent refusal; predicates now require exact permission errors.

Browser cancellation, PKCE rejection, refresh rotation/storage failure and other
error handling have focused automated coverage; real expiry-driven refresh,
Host Push against GitHub and Linux hardware still need explicit live checks.

### Exact UI verification after App setup

1. Launch `app/SiloUI/src-tauri/target/debug/bundle/macos/Silo.app` in normal mode.
2. Open GitHub and click Connect. Confirm Connecting feedback, authorize through
   GitHub in the external browser and return to the same app window.
3. Select repositories for a computer; toggle write permission, then switch to All
   repositories. Confirm persisted choices and real applying/success states.
4. Change access while the computer is running. Verify allowed and forbidden Git/gh
   requests and unchanged boot ID; do not infer success from a green card alone.
5. Reopen onboarding GitHub/review and confirm identical choices, completion and
   compact existing cards. Skipping GitHub must still permit setup completion.
6. Test cancellation, denied access, network failure and Retry. Inspect normal-size
   screenshots for unchanged layout and single-line normal captions.
7. Relaunch and verify saved account/policy restoration. Disconnect and verify both
   local access removal and GitHub revocation results without fabricated success.

## Sources and design evidence

The full rationale and hostile-case test matrix are in
`SiloUI-GITHUB-SERVER-FREE-RESEARCH.md`.

- [GitHub native-client best practices](https://docs.github.com/en/apps/creating-github-apps/about-creating-github-apps/best-practices-for-creating-a-github-app).
- [GitHub's own distributed PKCE client](https://github.com/github/github-mcp-server/blob/main/docs/oauth-login.md).
- [Scoped user-token API](https://docs.github.com/en/rest/apps/apps#create-a-scoped-access-token).
- [OAuth and token revocation](https://docs.github.com/en/rest/apps/oauth-applications).
- [PKCE support](https://github.blog/changelog/2025-07-14-pkce-support-for-oauth-and-github-app-authentication/).
- [Rate-limit handling](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api#handle-rate-limit-errors-appropriately).
- [Pinned MicroSandbox source](https://github.com/superradcompany/microsandbox/tree/5eca4de8bf233e57f114140f8c076ea8c96f21ab).
- [Git LFS batch API](https://github.com/git-lfs/git-lfs/blob/main/docs/api/batch.md).

## Connection defaults and empty-policy regression (2026-09-09)

An empty native policy array incorrectly bypassed host-author defaults. Drafts now
resolve each computer independently: existing policy first, then the actual host Git/jj
author. No configured author means empty fields with identity application disabled;
repository access does not depend on inventing an author. Unfinished identity text
is preserved locally and is not submitted by unrelated repository changes.

A rejected configuration save now ends local applying feedback, preserves the
valid repository catalog, and lets the existing Retry action resubmit that intent.
Stale rejections cannot override newer edits; whole-configuration rejection settles
all outstanding local computer edits included in that request.

Validation: 492 frontend tests across 52 files, 35 focused native GitHub tests,
lint/type checking, and the configured macOS debug bundle passed. The real app
showed the host author populated for all three VMs. After Keychain access resolved,
selecting All repositories for dev saved the selection without the invalid-identity
error and settled its loading state. The existing VMs then reported real runtime
failures: missing GitHub protocol support, old identity environment variables that
require restart to remove, or missing guest Git. No existing VM was recreated.

Automatic approval review blocked confirming Disconnect on the live account;
the confirmation was canceled. Fresh-connect defaults have regression coverage,
but manual reconnect was not verified. A separate fresh-VM UI check was attempted;
the creation form disappeared before input could be applied, so no test VM was
created and that check is not counted as passed. Earlier fresh-VM integration
results above remain separate from this UI regression check.

## Keychain access during a session

Account credentials and the runtime-token ledger each use a serialized host-memory
cache. The first read opens the system credential store; repeated reads reuse its
result, including absence or failure. Writes persist to Keychain before updating
the cached value; unchanged values do not trigger writes. Failed reads/writes stay
blocked until the user explicitly connects, refreshes repositories, retries GitHub
configuration, or disconnects. Disconnect clears the cached account only after
successful credential deletion. No cache is sent to the UI or saved as plaintext.

This prevents background reconciliation from repeatedly asking for permission.
Development bundles are ad-hoc signed: a rebuild can still require renewed macOS
approval. A release needs a stable signing identity; never broaden a Keychain
item's access to all applications to hide the prompt. Apple documents both
[Keychain access control](https://developer.apple.com/documentation/security/access-control-lists)
and [renewed prompts when a trusted app changes](https://support.apple.com/en-gb/guide/keychain-access/kyca1331/mac).

Validation: all 38 GitHub native tests passed, including one read across concurrent
callers, no repeated reads or unchanged writes, cached denial until explicit retry,
write-failure handling, and cached absence after deletion.


## Verification expansion (2026-09-10)

Current completed evidence:

- 71 focused native GitHub tests passed; four authenticated/hardware tests stayed
  opt-in. Coverage includes stale policy results, removal before network changes,
  cached Keychain denial, refresh rotation/storage failure, callback validation,
  bounded rate-limit backoff, ambiguous mutation refusal and token redaction.
- All seven Host Push tests passed: committed-only import, hostile destinations,
  managed/running guards, binary transfer limits and bounded host output.
- The actual signed bundled runtime passed the isolated synthetic GitHub hardware
  regression in 11.46 seconds: guest tools and placeholder credentials, verified
  TLS to GitHub, production Start/Restart attachment, live profile removal without
  reboot, Git identity and binary transfer. Its VM was removed afterward.

After explicit approval, `python3 app/SiloUI/tests/live/browser-regression.py`
passed the complete authenticated workflow in 58.19 seconds on macOS, using an
isolated Zen sign-in and the existing three private test repositories:

- Production PKCE exchange and refresh rotated both access and refresh tokens.
- GitHub enforced selected-repository read/write bounds for REST and GraphQL,
  including opaque issue IDs and direct child-token scope/reset escalation probes.
  The fixture issue remained unchanged after denied mutations.
- All-repositories access included the authorized fixtures. Revoking one child
  token denied that token while preserving the parent and sibling token.
- A real disposable VM cloned, fetched and pushed Git; used `gh` REST/GraphQL;
  and round-tripped a 1 MiB LFS object. Removing writes and then all access took
  effect without changing the VM boot ID; restoring access also needed no reboot.
- Host Push published two committed changes and their LFS data while guest writes
  were disabled. It excluded dirty files and did not execute the hostile fixture
  hook. A fresh clone verified the uploaded LFS contents.
- Cleanup closed the marked issue, removed the fixture branch and VM, and revoked
  the isolated token and children. The saved Silo account was untouched.

An earlier attempt failed before exchange because the test process lacked network
permission. An explicitly network-enabled, credential-free native DNS/TCP/HTTPS
probe passed in 0.46 seconds, followed by the fresh successful sign-in above. No
production transport or firewall change was needed; temporary diagnostics were
removed. Authenticated subprocess output stays suppressed to protect credentials.

A second full run with `SILO_GITHUB_TEST_INFLIGHT=1` passed in 83.71 seconds.
The guest kept one verified-TLS connection open across two successful authenticated
private-repository HTTP requests, with a one-second persistence check after each.
Changing write access to read-only then closed that existing connection within
five seconds. The remaining workflow and cleanup passed again. This directly
checks live connection cancellation on a write-policy change; it does not undo a
request GitHub has already accepted or establish mid-upload rollback.

These are native/runtime integration tests, not a complete manual UI sign-in test
or proof of every `gh` command. No security test proves absolute immunity to
credential exfiltration.

The repeatable runner and its exact boundaries are documented in
`app/SiloUI/tests/live/README.md`. Rechecked GitHub's official
[scoped-token endpoint](https://docs.github.com/en/rest/apps/apps#create-a-scoped-access-token)
and [rate-limit guidance](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api#handle-rate-limit-errors-appropriately).
The tests keep permission refusal distinct from network/rate-limit failures.

### Object transfer failure diagnostics (2026-09-15)

The guest-to-host copy in [`host_push.rs`](../app/SiloUI/src-tauri/engine/src/host_push.rs)
previously discarded stderr and mapped every nonzero exit to “Object transfer
failed or exceeded the available temporary space budget.” That message did not
establish disk exhaustion. Remote `repository.push` executes this copy on the
device hosting the computer, before the device uploads the imported objects to GitHub.

Failures now include the source object, computer, process status, copied bytes,
remaining transfer budget, and bounded runtime stderr with sensitive-looking
words redacted. Only SIGXFSZ establishes the temporary file size limit failure;
other process failures retain their own diagnostics. These details travel in the
existing failed push result. They do not add a separate activity journal.

Validation: `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml
host_push::tests` passed all eight tests with synthetic GitHub configuration.
Fixtures cover binary copying, actual kernel file-size enforcement, missing-file
stderr and exit status, and bounded diagnostics that drain noisy output. This
run did not inspect a packaged app or reproduce the failure on `dev-zeronival`.

### Empty LFS objects and push progress (2026-09-16)

A push failed while copying
`lfs/objects/e3/b0/e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
This is SHA-256 of empty content. Git LFS's
[pointer decoder](https://github.com/git-lfs/git-lfs/blob/main/lfs/pointer.go)
recognizes empty Git blobs as empty pointers. Its
[uploader](https://github.com/git-lfs/git-lfs/blob/main/commands/uploader.go)
prints these pointers in `UploadPointers` dry-run output, but `prepareUpload`
skips zero-size objects during real uploads. A cache file is unnecessary.

Silo filters this empty-content identifier from the guest-to-host transfer plan.
Nonempty objects still require copying and SHA-256 verification. Missing
nonempty objects still fail; the fix does not enable incomplete LFS pushes.
The bundled-Git regression imports a committed empty file and a nonempty LFS
pointer, checks the real dry-run output and absent empty-object cache, and
verifies that only the nonempty identifier requires transfer.

Push progress belongs to the production source while the native invocation is
pending. Cached remote snapshots must not erase it or allow a duplicate request.
The command result supplies the terminal state before subsequent remote polls.

Validation: the bundled-Git regression failed before the empty-object filter and
passed afterward. `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml
host_push::tests` passed all nine tests with synthetic GitHub configuration.
Frontend regression coverage includes stale remote snapshots during a pending
push, duplicate clicks, both terminal outcomes, and a snapshot arriving after
completion. No packaged app or live `dev-zeronival` push was inspected in this run.

### Proposed replacement for host-push transfer planning (2026-09-16)

Research only; this replacement is not implemented or benchmarked. Preserve
explicit host-authorized publishing while the computer's GitHub policy remains
read-only. Running guest `git push` under temporarily elevated computer-wide policy
would change that boundary. Running in the guest also executes its hooks;
current Host Push intentionally does not execute those hooks on the host.

Prototype a host-owned bare publishing repository, with ordinary Git fetch from
the guest and ordinary Git/LFS upload to GitHub. Keep guest hooks and
configuration out of the host repository. Pin the selected commit and target
branch before transfer. Test standard LFS transport before designing any Silo
adapter: Git LFS supports pure SSH, and charmbracelet/git-lfs-transfer provides
an existing server implementation. This is a candidate dependency requiring
compatibility, packaging, permissions and maintenance evaluation, not an
established Silo capability. Silo's existing bridge would carry the commands and
bytes; it must not interpret packfiles, pointer contents, or human dry-run output.

Primary sources:

- [Git remote helpers](https://git-scm.com/docs/gitremote-helpers.html): native
  Git service connections, including upload-pack.
- [Git LFS server discovery](https://github.com/git-lfs/git-lfs/blob/main/docs/api/server-discovery.md): SSH transfer discovery.
- [git-lfs-transfer](https://github.com/charmbracelet/git-lfs-transfer): existing
  pure-SSH server implementation.
- [Git LFS fetch](https://github.com/git-lfs/git-lfs/blob/main/docs/man/git-lfs-fetch.adoc): `--all` covers all commits reachable from selected refs,
  whereas a default fetch is not a complete publication-history migration.
- [Git LFS local file adapter](https://github.com/git-lfs/git-lfs/blob/main/docs/man/git-lfs-standalone-file.adoc): an alternative only where the source is
  already exposed as a local filesystem; avoid introducing a filesystem mount
  solely for this feature.

The proof must include LFS objects referenced only by historical commits,
empty files, pruned guest caches whose objects still exist upstream, truly
missing objects, non-fast-forward rejection, dirty worktrees, linked worktrees,
custom LFS storage, and denied concurrent guest writes. Compare final refs and
fresh-clone LFS bytes against a normal Git/LFS push in disposable fixtures.
Measure cold and repeated push bytes, disk use and duration before selecting a
bounded per-repository cache. A remote operation ID must outlive the requesting
connection; disconnect is an unknown outcome until reconciled, not proof of failure.

### Standard host publishing implementation (2026-09-16)

This supersedes the earlier empty-object filter and proposed replacement above.
The production path no longer builds full Git bundles, parses LFS dry-run
output, computes LFS object paths, or copies/hash-checks those objects itself.

1. Authorize the host for the selected GitHub repository; do not elevate the computer's
   GitHub permission. Capture the source branch and commit in a temporary Git ref.
2. Run bundled host Git `fetch` through a private OpenSSH connection to the computer's
   Git upload-pack service. Keep SSH configuration outside the user's SSH config.
3. Run standard `git lfs fetch --all` for the captured ref over pure SSH. The
   pinned upstream `git-lfs-transfer` server is installed in an operation-specific
   guest temporary directory. Its read view points to `LocalMediaDir` resolved by
   guest `git lfs env`, supporting linked worktrees and custom LFS storage without
   reproducing storage-resolution rules. This parses storage metadata, not a
   human-readable transfer plan. No source cache directory is created if absent.
4. Ask Git to reject a non-fast-forward before LFS transfer. Let Git LFS determine
   required uploads and verify their bytes. A failed LFS upload permits one
   standard upstream LFS fetch/retry to recover pruned historical objects. No Git
   ref is updated until Git LFS reports success; incomplete pushes stay disabled.
5. Run standard non-force Git push of the captured commit to the captured branch.
   Record the result and update source tracking metadata to that exact commit.
   Remove the temporary source ref and server directory on normal completion.

The host publishing repository is private and contains only host-created
configuration. Guest hooks, credential helpers and `.git/config` are never
imported or executed on the host. This is explicit committed-state publication;
it does not promise to execute the computer's custom pre-push hooks or honor its
arbitrary push configuration. The final Git push still checks concurrent remote
updates. Guest changes made after capture do not change the published commit.

The cache is keyed by computer, source path and GitHub repository and bounded to
2 GiB across repositories between operations. Oversized individual caches are
removed after use; otherwise least-recently-used caches are evicted. A process
lock excludes simultaneous use/eviction and is inherited by Git so an application
crash cannot let a new host process reuse files still being written. Commands
also enforce per-file disk limits, preserve free disk space, bound/drain output,
and keep sanitized Git failure diagnostics. Persistent cache data has no tokens.
The GitHub LFS upload remains the authority for missing-object errors; a source
transfer alone cannot prove publication will succeed.

`repository.push.start` persists a client operation ID before dispatch and returns
immediately. `repository.push.status` observes that same job. Lost replies are
queried before reusing the same ID, and duplicate clicks for a repository return
the existing job. Older devices receive an update-required message instead of a
fallback to the custom synchronous transfer. The owner process records terminal
results independently of its SSH observer. The main window and status panel both
persist dismissals.

A device restart with no recorded terminal result yields `unknown`, not a fabricated
failure or success. The user must check the branch on GitHub and explicitly choose
“I’ve checked GitHub” before a separate retry. This does not automatically verify
GitHub. The inherited cache lock prevents reuse while surviving Git children still
hold it. For idempotency, journal records are not silently evicted: the current
journal has a 10,000-record/16 MiB limit and refuses new starts at capacity. Future
retention changes must reject expired IDs rather than replay them.

Verification uses disposable local repositories, bundled Git/LFS, an upstream
SSH-server prototype, journal fixtures, and frontend fixtures. It does not claim
an authenticated GitHub push from a live computer, a two-device session, or a packaged
app inspection. The opt-in authenticated computer test continues to invoke the same
production publication function; the obsolete custom binary-copy check was removed.

Final focused verification for the replacement:

- `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml host_push -- --test-threads=1`
  with synthetic GitHub configuration: 24 passed; one subprocess helper is ignored
  by the ordinary harness and executed by its parent lock-inheritance test.
- `cargo test ... editor::tests`: five passed; one opt-in live test skipped.
- Frontend production-source, connections, application-window and status-panel
  tests: 183 passed; `npm --prefix app/SiloUI run typecheck` and `lint` passed.
- `npm --prefix app/SiloUI run test:release`: 33 passed before the additional
  license-permission regression; the final packaging file's three tests passed.
- Runtime-cache, artifact-transfer and release-workflow Python checks: 35 passed.
- Both Linux ARM64 and AMD64 helper builds were verified; the current local staged
  helper remains ARM64. The stage cache restored without network access.

Failed publications discard their cache. A synced `.active` marker also causes
cache recreation after a crash once the inherited process lock is free, avoiding
stale Git lock files. Incremental fetch verifies incoming objects using
`fetch.fsckObjects`; publication does not rescan every cached object on each push.

### Repository discovery cutoff regression (2026-09-20)

The live `dev` sandbox contained 216 `.git` entries. The discovery script in
`src-tauri/src/host_push.rs` piped `find` through `head -201`, then exited with
status 1 at entry 201. `runtime::run_msb` rejected the nonzero exit, discarding
all earlier records and showing the generic committed-repository read warning.
A disposable 216-entry fixture reproduced exit 1 after 200 records.

Discovery now scans without an entry-count cutoff. Its existing 30-second guest
deadline, 45-second host deadline, and 1 MiB runtime output budget still apply.
Guest repository commands use `--no-start` so a concurrent stop cannot trigger
an automatic boot. The corrected script completed against the live sandbox,
returning 182 records; nine ownership rejections and one invalid gitfile remained.
No Git ownership exceptions were added. Detached HEAD and other entries excluded
by the existing discovery rules remain outside this fix.

Live script verification used the running debug bundle's `Contents/MacOS/msb`
at `app/SiloUI/src-tauri/target/debug/bundle/macos/Silo.app`, with `--no-start`.
This checks discovery against live data, not the updated packaged UI.

`cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml host_push -- --test-threads=1`
passed 25 tests with one subprocess helper ignored by the ordinary harness.
The 216-entry regression failed before the change and passed after it.

`npm --prefix app/SiloUI run desktop:build:debug` compiled the updated app and
assembled the debug bundle, but Tauri's signing step failed intermittently on
Git helpers, including on an unsandboxed retry. Signing the affected helpers
individually and resealing the outer bundle with `codesign --force --sign -`
succeeded; `codesign --verify --deep --strict` then passed for the exact debug
bundle above. The running app was not quit or relaunched because Quit stops
Silo-owned local VMs. The updated packaged UI therefore remains unverified.

All ignored Rust live regressions require
`SILO_LIVE_TEST_CONFIRM=disposable-test-fixtures` before touching the runtime,
network, or editor integration. Authenticated GitHub regressions additionally
require `SILO_GITHUB_TEST_CONFIRM=private-test-repositories`. The isolated browser
regression checks both confirmations before exchanging its authorization code.
The guest regression runs the already-built test executable with an exact test
selector; it does not start nested Cargo or share Cargo build locks.

### Repository push preflight execution

The async push task obtains its planned commit count on the blocking pool before
publishing its pushing state. Although the count is cached, resolving its cache
key reads the selected runtime generation and takes the migration progress mutex.
This filesystem and lock boundary follows
[Tokio's blocking-work guidance](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

### Policy revision numeric boundary

GitHub policy revisions are persisted as Rust `u64` values and sent over IPC as
JavaScript numbers. The [ECMAScript specification](https://tc39.es/ecma262/multipage/numbers-and-dates.html#sec-number.max_safe_integer)
defines the largest safe integer as 9,007,199,254,740,991. Larger integers can
share the same number representation. Silo's frontend schema already rejects
unsafe integers, so an unsafe saved revision made the GitHub snapshot unavailable.
The native document loader now rejects that revision without rewriting the file.

Every native policy increment uses
[`u64::checked_add`](https://doc.rust-lang.org/std/primitive.u64.html#method.checked_add)
and enforces the JavaScript boundary. Wrapping would reuse old revision stamps;
saturation would stop distinguishing changes. An exhausted revision therefore
returns an error before publishing a new policy. The last safe revision remains
readable, and a save that changes no choices still succeeds. Reconnection checks
the next revision before replacing the account credential or detaching computer access.

Regression fixtures cover unsafe persisted values, exhausted policy edits without
document mutation, and the last safe increment followed by save, reload, and a
no-op edit. These use temporary files and synthetic native-test configuration.

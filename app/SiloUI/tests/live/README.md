# Authenticated GitHub regression

This opt-in test calls Silo's native token implementation directly against GitHub. It requires no Silo server. Default tests never contact GitHub or mutate repositories.

## Requirements

Use three **disposable private repositories under one owner**, each authorized for the Silo GitHub App. Enable Issues in the write fixture. The App must have the approved repository permissions (including Contents and Issues write). The parent token must be a valid user token for that App, not a PAT. The deliberately denied fixture is authorized at the App level but excluded from the VM grants.

Provide these values through a private local environment or secret manager. Never paste token values into chat, command arguments, committed files, logs or screenshots:

- `SILO_GITHUB_CLIENT_ID`, `SILO_GITHUB_CLIENT_SECRET`
- `SILO_GITHUB_TEST_USER_TOKEN`
- `SILO_GITHUB_TEST_READ_REPO`, `SILO_GITHUB_TEST_READ_REPO_ID`
- `SILO_GITHUB_TEST_WRITE_REPO`, `SILO_GITHUB_TEST_WRITE_REPO_ID`
- `SILO_GITHUB_TEST_DENIED_REPO`, `SILO_GITHUB_TEST_DENIED_REPO_ID`
- `SILO_LIVE_TEST_CONFIRM=disposable-test-fixtures`
- `SILO_GITHUB_TEST_CONFIRM=private-test-repositories`

Repository names use `owner/name`. IDs are the GitHub numeric repository IDs. The confirmation authorizes mutations only in the explicitly named disposable fixtures. Exact names, IDs, common ownership and privacy are verified before minting tokens or making mutations. Parent authorization is never revoked.

From the repository root:

```sh
cargo test --manifest-path app/SiloUI/Cargo.toml --workspace --offline \
  github_authenticated_native_workflow -- --ignored --test-threads=1
```

The test verifies:

- Native read/write token creation and exact repository boundaries through REST and GraphQL.
- Forbidden repository access by both repository name and GraphQL node ID.
- One uniquely marked issue created using the write token; REST and node-ID writes denied using the read token; an allowed node-ID update.
- Child-token re-scoping through native code and directly through GitHub, plus direct child-token reset: neither may expand repository or write authority. Network, rate-limit and unexpected responses are not accepted as proof of denial.
- “All repositories” includes all three authorized fixture repositories.
- Individual child revocation leaves the parent and sibling token usable.
- Finally, the test issue is closed and child tokens are individually revoked, including tokens created by the escalation probes.

A failure reports only the stage, never a token or raw HTTP response. Check the named write repository for an issue titled `Silo authenticated regression <UUID>` if cleanup is unconfirmed. Closed issues remain as test evidence. A lost response to token mint or issue creation is ambiguous: it cannot safely be retried or guaranteed cleaned up; inspect the disposable fixtures and let unknown short-lived tokens expire. Never run this against working repositories.

## Actual VM Git, gh and LFS

Also provide `SILO_TEST_MSB` and `SILO_TEST_LIBKRUNFW` pointing to Silo's patched MicroSandbox binary and library, and set `SILO_GITHUB_TEST_VM=1`. Run the same command with hardware virtualization permissions.

After native checks, the harness directly invokes the already-built test executable with an exact selector for the existing ignored `github_authenticated_guest_workflow` test. Its process receives only scoped child tokens, fixture names and the required host toolchain environment. It does not receive the App client secret or parent user token. Child output is suppressed to avoid credential disclosure.

The guest test uses a temporary managed VM to check real Git clone/fetch/push, `gh`, Git LFS roundtrip, absence of real tokens in guest environment/configuration, live write removal, full access removal, restoration and unchanged VM boot ID. It also snapshots the source while write access is assigned, restores a fork after assigning that target a read-only profile, and verifies read access works while issue mutation and Git push fail immediately. The restore does not repair policy after guest start. It creates a unique test branch and deletes it during cleanup. Uploaded LFS objects can remain in GitHub storage after branch deletion; these repositories must be disposable.

This harness does not automate browser consent, sign-in cancellation, token refresh expiry or Linux hardware. Those require their own explicit verification. A passing ordinary suite or a compiled ignored test is **not** an authenticated integration pass.

## Isolated browser authorization and real token refresh

When the saved Silo session cannot be read, do not substitute the GitHub CLI's
unrelated token or change Keychain permissions. Set the same fixture variables,
plus `SILO_TEST_GIT` (the bundled Git executable) and `SILO_TEST_GIT_SUPPORT`
(the bundled `git-support` directory), then run:

```sh
python3 app/SiloUI/tests/live/browser-regression.py
```

Open the printed authorization URL in the already signed-in browser. This uses
the existing App permissions and a separate loopback callback. It never reads or
replaces Silo's saved account. The native test runs the production PKCE exchange,
refreshes that isolated session immediately, verifies access/refresh rotation,
then runs the native and VM tests. Cleanup revokes only the isolated test token,
never the entire App authorization. Tokens remain in process memory and child
environments; the launcher does not create credential files or print callbacks.
The ignored local build configuration supplies client settings unless explicitly
overridden in the environment. Browser consent expires after five minutes.

The authenticated VM test now additionally checks:

- Actual `gh api graphql` repository queries and opaque issue-ID mutations.
- Removing write access rejects the mutation and preserves the issue title.
- The production Host Push object-transfer path while guest access stays read-only.
- A second 1 MiB LFS roundtrip with exactly the expected two new commits.
- Guest pre-push hooks do not execute on the host, and uncommitted files remain
  local and absent from the remote clone.

The shared transfer function starts after the production UI's authorization and
managed-VM guards; the live test supplies only its explicit disposable VM and
scoped fixture token. It proves transfer behavior, not a click through the real
Push button. Ordinary tests cover the guards separately. A passing live test
must not be claimed when the browser or credential prerequisite is unavailable.

### Existing authenticated connection cancellation

Set `SILO_GITHUB_TEST_INFLIGHT=1` with the browser runner above to also check a
held TLS connection. The guest uses `openssl s_client` with certificate verification,
requires two successful private-repository reads on the same connection and checks
that it remains open before removing write access. The connection must close within
five seconds of that policy update. Early server closure is reported as inconclusive,
not successful revocation. No production code or guest package installation is added.
Only fixed diagnostic messages can escape the captured child output.

This passed with the complete authenticated workflow on macOS in 83.71 seconds on
2026-09-10. It tests connection cancellation, not reversal of requests GitHub has
already accepted or cancellation of every possible long-running GitHub operation.

# Silo 0.4.4 GitHub authentication investigation

2026-09-14. Investigation of a report from another device: GitHub connected,
All repositories selected, fake credentials receive GitHub HTTP 401, but the
Silo placeholder causes curl error 52 for GET /user and Git info/refs.

## Scope and evidence

Tag `v0.4.4` resolves to `9ce02908787c55dbd5f5b0101e32ebdeb637d084`.
`git diff v0.4.4 -- app/SiloUI/src-tauri/src/github.rs
app/SiloUI/src-tauri/src/runtime.rs app/SiloUI/patches` was empty.
The other device's persisted status, runtime credential profile, and logs were
not inspected. No live computer or packaged application was launched or modified.
No real credentials were read. The report is supplied evidence, not a locally
reproduced network trace.

## Confirmed defects and limits

### All repositories can succeed with zero credentials

[`scopes`](../app/SiloUI/src-tauri/engine/src/github.rs#L923) groups only repositories
in the cached GitHub installation catalog. With access enabled, all mode and
an empty catalog, it returns Ok with zero groups. There is no all-mode empty
catalog validation. [`runtime_grants_for`](../app/SiloUI/src-tauri/engine/src/github.rs#L994)
therefore succeeds with zero grants; [`profile`](../app/SiloUI/src-tauri/engine/src/github.rs#L586)
produces `{"version":1,"owners":[]}`. The apply loop can attach this disabled
profile and report "GitHub access verified."

This is a concrete path matching the report. It is not proof that the other
device has an empty catalog. Sign-in checks whether an installation exists,
not whether that installation supplies at least one accessible repository.
Catalog refresh also accepts an empty list.

### Success indicators do not establish usable computer access

[`public_snapshot`](../app/SiloUI/src-tauri/engine/src/github.rs#L529) derives Connected
from the observed account credential expiration, independently of computer grants.
That distinction is valid, but Connected plus the All repositories checkbox
cannot diagnose whether the computer has credentials.

The stronger false-success defect is in [`apply`](../app/SiloUI/src-tauri/engine/src/github.rs#L676):
"GitHub access verified" means reconciliation finished without error. It does
not perform an authenticated computer request. It can report success with no grants.

### Proxy denial becomes an unexplained connection drop

The [bundled runtime patch](https://github.com/0xpolarzero/silo/blob/d3481b294342784bbf1047e6d94587381eee2479/app/SiloUI/patches/microsandbox-create-stopped-0.6.17.patch#L2349)
rejects empty profiles, malformed profiles, expired selected credentials,
missing owner grants, unavailable write credentials and malformed token values
through the same `ViolationAction::Block` result. The inherited TLS relay in
`crates/network/lib/tls/proxy.rs` returns PermissionDenied and drops the stream
without writing an HTTP error response. Its generic log text even describes
all of these cases as a placeholder sent to a disallowed host.

This explains curl 52 / GnuTLS premature termination without requiring a daemon
crash. Fake credentials bypass Silo token selection, so their HTTP 401 responses
only demonstrate that the TLS/network route to GitHub works.

GET /user has an explicit read-token route, including for multiple owners. Its
failure is not an expected limitation of repository-scoped routing. An absent
repository owner alone cannot explain both reported failures. Empty or expired
profiles can; malformed profile/token data can also block at this boundary.

### Additional recovery risk found by code inspection

[`apply_github_policy`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L1267) removes the
boot-profile cache before capability checks and runtime update, but failure
leaves the higher-level ACTIVE grant cache untouched. [`apply`](../app/SiloUI/src-tauri/engine/src/github.rs#L734)
skips attachment when desired grants equal that higher-level cache.

A concrete sequence to cover in a regression is: A applied successfully; update
to B fails after boot cache removal; user restores A while its grants are still
unexpired. Desired grants equal ACTIVE A, so attachment is skipped and success
can be reported although the boot cache remains empty. A later computer start uses
the disabled default. This sequence is statically identified, not executed in
the isolated tests below and not established as the other device's trigger.

## Deterministic verification

An isolated Rust harness copies the production scopes function and the token
selector from the tracked runtime patch. Only Document and ViolationAction
context types are replaced with minimal fixtures. Tokens are synthetic.

Command:

```sh
cargo test --offline --manifest-path app/SiloUI/src-tauri/target/verification/github-044-investigation/Cargo.toml
```

Result: 4 passed, 0 failed. It confirms:

- All mode with an empty catalog returns zero scopes successfully.
- Empty profiles block GET /user and Git read info/refs.
- Expired profiles block the same requests.
- Valid synthetic profiles select a token for both requests.

These are production-function behavior probes, not a full application or
network reproduction. Generated harness/build output stays under ignored target.

## Next diagnostic action

Obtain the affected computer's operation/error, cached repository count,
and refresh deadline from the other device's Silo status. These are the minimum
missing facts to distinguish the reproduced empty-catalog path from credential
renewal/attachment failure. Do this before restarting, which changes evidence.
Do not place a PAT in the guest to work around the failure.

## Proposed first-repository fix (2026-09-14 follow-up)

The user confirms the account has no repositories and rejects a manual Test
access button. Empty repositories must be a supported connected state.

Preserve authorized installation owners and their permissions independently of
the repository catalog. Build all-mode owner grants from that inventory even
when an owner has zero repositories. Derive write eligibility from the explicit
all-repository changes setting rather than a nonempty existing-repository ID
list. Keep selected-repository grants narrow. Route authenticated-user creation
to the authenticated user's owner, not an arbitrary owner or an ambiguous
multi-owner write fallback; cover gh's actual GraphQL creation path as well.

GitHub documents that POST /user/repos accepts GitHub App user access tokens
with Administration repository permission (write):
https://docs.github.com/en/rest/repos/repos#create-a-repository-for-the-authenticated-user

GitHub's scoped-token endpoint supports an owner target and permission map;
repository names/IDs are optional:
https://docs.github.com/en/rest/apps/apps#create-a-scoped-access-token

These docs support testing owner-scoped credentials for the zero-repository
flow. They do not by themselves prove issuance and creation work together for
an empty installation. Validate that exact flow before adopting the change;
do not expose or fall back to the parent account token. No separate account
credential broker is justified before testing the existing scoped mechanism.

Acceptance flow: an empty installed account with all mode and changes enabled
can authenticate, create its first repository using gh, and push. Read-only
mode rejects creation clearly. Reconnection, expiry and multiple owners must
retain the same boundaries. Automatic status checks and meaningful proxy errors
replace the proposed manual diagnostic button. This is a proposal, not a shipped
fix or a live verification result.

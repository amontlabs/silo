# Secrets

Silo stores general secret values in the macOS Keychain, the only supported host today.
The engine's credential store also builds against the Linux Secret Service, but no Linux
host build ships yet and that backend is untested. `secrets.json` contains names, immutable value references,
computer assignments, allowed hosts, pending updates, safe failures, and bounded activity
history. Values are never returned in application snapshots. The editor sends a
value only when adding or replacing one, then discards its draft on successful save.

The credential store is read once per session, including caching denied access;
an access failure expires after ten seconds so later starts can retry. An explicit
save/delete/retry also retries a failed store operation. There is no plaintext
fallback. A value is stored before its reference is published. Superseded values
are pruned after reconciliation.

Remove deletes the credential-store value and removes the secret from the list
before returning, without waiting for any computer. Future starts resolve the
current assignments and receive no removed value. Silo records possible access
by each affected computer in an internal `pendingRevocations` journal containing
only the computer name, secret name, secret ID, and immutable value generation.
A credential-store deletion failure remains retryable and excludes that secret
from future boot material; it does not report successful removal.

State refreshes retry pending revocations in the background on the computer's owner
device, including for remote rows. A busy, transitional, paused, or unreadable
computer keeps its record. A running computer clears it only after both durable and
active runtime configuration confirm that the name is absent. Missing, deleted,
stopped, or crashed computers clear it without a live update. A verified restart
with the current assignments also clears it. Pending computers show “May still
have access to GITHUB_TOKEN until it restarts” and offer Restart through the
existing lifecycle controls. Unreachable computers never disable Edit or Remove.

Revocation sends only the removed name to the runtime and does not read remaining
secret values. Re-adding a name creates a new secret ID and value generation.
Under the computer gate, a retry rechecks current assignments and never removes a name
assigned to a replacement. A verified live replacement clears earlier records;
a deferred replacement keeps the warning until restart. Each completion clears
only its own records, preserving later removals of the same name. Secret updates
also resolve current material after acquiring the computer gate so an older queued
update cannot restore a value removed while it waited.

MicroSandbox receives host environment source references, not inline stored values.
The guest sees `$MSB_NAME` through its named environment variable. Existing values
and domain restrictions update live; adding a new variable on a running computer waits
for the next boot. Silo never restarts a computer implicitly. Runtime configuration is
checked after updates, including source reference, allowed hosts, placeholder, and
TLS requirement. The bundled runtime patch closes active proxy connections after
secret policy changes. Already delivered requests cannot be recalled.

TLS interception applies only to destinations matching an assigned secret's
allowed hosts. Other destinations retain their normal server certificates, so
clients that discard inherited CA settings can still connect. Exact hosts,
wildcards, and the explicitly acknowledged `*` assignment all use the same host
matching as secret injection. A `*` assignment therefore still intercepts all
destinations. Network policy is checked before this routing decision.

Changing or removing a secret updates the interception scope live and closes
existing proxy connections so their next connection uses the current policy.
This is a Silo-specific behavior of the bundled runtime; it does not disable
certificate verification or learn exceptions from failed TLS handshakes. Clients
connecting to secret destinations must still trust the computer's CA. See the
[ZCode TLS investigation](SiloUI-ZCODE-TLS-INVESTIGATION.md) for the regression
and design rationale.

Every boot resolves the current desired secret configuration; unavailable required
credentials block startup rather than restoring old material. Start completion
clears pending state only when the desired revision still matches. Secret and
GitHub updates share the existing runtime locks and preserve each other's config.

The secrets read command runs document I/O and JSON decoding on Tauri's blocking
pool. A slow document read therefore leaves the async executor available to other
commands. The FIFO regression supplies a blocked file in a temporary directory
and requires an independent future to run before releasing the read. This uses
[Tauri's async command execution](https://v2.tauri.app/develop/calling-rust/#async-commands)
and the supported [blocking-work boundary](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

## Boundary

This mechanism supports credentials transmitted in proxied HTTPS requests. It does
not support private keys that a guest application needs for local signing. Allowed
servers receive the real value; a server that reflects it can reveal it to the
guest. Restrict allowed domains to trusted services. The existing `*` control
requires explicit acknowledgement that any HTTPS server could receive the value.
Guest output may contain sensitive user data; generic logging is not a guarantee
of redacting arbitrary data echoed by external services.

Silo backup archives do not include the device credential store. A restored computer must
use the device's current secret assignments, not recover secret values from its disk.
Checkpoint restores follow the same rule for GitHub access: the restore command
uses the profile assigned to the target computer before the guest resumes. The
source computer's cached grants and credentials held by historical checkpoint
memory are not used. If the target has no current GitHub profile, restore starts
without GitHub access.

## Sources

- [MicroSandbox placeholder substitution](https://microsandbox.dev/blog/sandboxes-that-lie-about-their-secrets)
- [Official Rust SDK modification API](https://github.com/superradcompany/microsandbox/blob/main/docs/sdk/rust/sandbox.mdx)
- Current runtime pin: [`runtime-inputs.json`](../app/SiloUI/runtime-inputs.json).
  The upgrade records in [runtime packaging](SiloUI-RUNTIME-PACKAGING.md)
  state which checks were repeated on each runtime version.
- Historical MicroSandbox 0.7.2 source: `60d4dc8a436fb9365491567ec21d073e924e3c6d`.
  In that revision, the SDK's
  [live secret update implementation](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/sdk/rust/lib/sandbox/modify.rs#L1577)
  resolves values for rotation and sends only `SecretLiveChange::Remove { name }`
  for removal. B-27 used that existing API without a runtime patch change.
- Silo's checked-in runtime patch contains policy-change connection cancellation.

## Manual checks

Use disposable values and computers. Never use a real credential for an echo-service test.

1. Add a secret to a stopped computer; verify its name and domains, then relaunch Silo.
2. Start the computer; inspect its environment and durable runtime config. Only the
   placeholder and host source reference should appear.
3. Send an HTTPS request to an allowed test server and verify substitution there;
   send the placeholder to a different server and verify blocking.
4. Rotate the value, change domains, and remove access while the computer runs. Verify
   the new policy on new and previously open connections, without changing boot ID.
5. Add another secret while running. Only that secret should show the affected computer
   as requiring restart; rotating the first secret must not add a restart notice.
6. Restart, then verify the new variable and cleared pending state.
7. Deny credential-store access and retry: preserve edits, show the unlock message,
   and do not repeatedly prompt in the background.
8. Make a running computer unreadable or transitional, then remove its secret. Verify
   the list and credential-store value disappear immediately, the computer warns
   about possible access, and background refresh clears the warning after confirmed
   revocation. Restart, stop, and deletion must also clear that computer's record.
9. Re-add the same secret name while an old revocation is pending. Confirm the old
   retry never removes the replacement value, including when updates overlap.
10. Verify the warning and Restart action on a remote row through its owner;
    keep the warning on an unreachable owner until a fresh owner snapshot clears it.

## Verification on 2026-09-09

- Frontend: 515 tests passed across 54 files, including native permission wiring,
  async save failure/draft preservation, safe unlock guidance, pending state and retry.
- Native ordinary suite: 215 passed, five opt-in tests ignored; the existing
  permission-dependent runtime-alias check was excluded.
- Isolated real macOS MicroSandbox test passed: guest placeholders, source-only
  durable config, allowed HTTPS substitution, blocked destination, live rotation,
  domain restriction, deferred additions, restart activation, deletion, and boot IDs.
  An unchanged HTTPS connection handled a second request on the same socket;
  rotation and removal then prevented existing connections from using old values.
  The expanded live test passed in 22.11 seconds. The disposable VM was removed.
- Real production app: added a disposable value through Keychain, relaunched,
  edited domains without reentering the value, and removed it. A second check
  started dev through Silo, verified its placeholder, removed access live, and
  stopped dev again. Metadata had no plaintext value. Secrets was left empty.
- Linux hosts are not supported today; the Secret Service backend has never been exercised.

Run the opt-in runtime test with the signed bundled `msb` and library paths in
`SILO_TEST_MSB` and `SILO_TEST_LIBKRUNFW`, with the explicit confirmation below:

```sh
SILO_LIVE_TEST_CONFIRM=disposable-test-fixtures \
cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml \
  live_secret_adapter_uses_refs_and_preserves_boot_for_live_updates \
  -- --ignored --test-threads=1
```

It uses a temporary computer, disposable values, Ubuntu packages, and public HTTPS echo
endpoints. It never reads Silo's user credentials or modifies the user's computers.

## Verification on 2026-09-25

- Focused production selector regression: passed with separate source write and
  fork read-only profiles; the old unconditional-disabled restore path failed
  this same assertion. An unassigned restore target still selects no profile.
- Signed-runtime synthetic VM check: a full checkpoint restored through
  `checkpoints::start_pending` and the disposable fork was removed.
- Isolated live GitHub acceptance: the private fixture source was snapshotted
  while write access was assigned; the target fork received its current
  read-only host profile before production restore. The restored guest could
  read the private repository but could not mutate the test issue or push a
  branch. Guest environment/config checks found no real token. The test issue
  was closed, child tokens and the isolated OAuth session were revoked, test
  branches removed, and both test VMs deleted. Two 1 MiB Git LFS test objects
  remain unreferenced in the disposable fixture after branch cleanup.

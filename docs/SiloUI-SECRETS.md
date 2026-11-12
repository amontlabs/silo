# Secrets

Silo stores general secret values in the macOS Keychain on a Mac. On a Linux host the engine
stores them as files (see [Linux hosts](#linux-hosts)); no Linux engine release ships yet.
`secrets.json` contains names, immutable value references,
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

## GitHub tokens

A personal GitHub token is a secret of kind `github-token`. It has a name (a label, so hyphens are
allowed), a value in the same credential-store vault as other secrets, and the GitHub account
Silo learned when it checked the token against GitHub's read-only `/user` endpoint. You can keep
several. Add one from the Secrets page with Add GitHub token; it takes a name and the token only.

- **It is not an environment secret.** It has no allowed hosts of its own (they are GitHub's) and
  is never delivered to a computer as a `$MSB_NAME` placeholder. Assigning it to a computer
  (`silo secrets assign`) does not apply: a computer uses a token because its GitHub access names it
  (`tokenSecretId` in the computer's GitHub settings, see
  [GitHub personal tokens](SiloUI-GITHUB-PERSONAL-TOKENS.md)). The `computers` of a token secret
  in `secrets.list` lists the computers whose GitHub access uses it.
- **Checking.** Saving a token checks it with GitHub first and stores nothing if GitHub refuses
  it. The GitHub worker checks every token at startup and every five minutes (30 seconds after a
  failure). A token GitHub rejects shows as disconnected, with the account it last belonged to,
  and the computers using it lose GitHub access without falling back to anything else.
- **Remove.** Removing the secret first detaches every computer using it, then deletes the value.
  A failed deletion stays retryable and keeps the computers detached.
- **Saved data.** `secrets.json` gains `kind` and `account` for these secrets only; secrets
  saved by earlier versions load as ordinary secrets. An earlier Silo cannot read a
  `secrets.json` that holds a token secret.

## Linux hosts

A Linux server has no desktop session, so there is no Secret Service (or unlocked login keyring)
for the engine to use. On Linux, `silod` installs a file store as the `keyring` default at
startup, before anything reads a credential: every item the engine keeps in the Keychain on a
Mac (GitHub sign-in, token ledger, personal token, secret values) is one file,
`<data>/credentials/<service>/<account>`, where `<data>` is `$XDG_DATA_HOME/<identifier>`
(`~/.local/share/org.silo.dev` for Dev). The bridge and the guest helper install the same store,
so nothing reaches `keyring`'s default backend.

- **Files.** The directories are mode 0700 and the files 0600, owned by the engine's account.
  Every directory from `$XDG_DATA_HOME` down to `credentials/` must belong to that account or root
  and be writable by nobody else, or startup stops; the message names the directory, its mode and
  group, and the `chmod g-w` (or `go-w`) that fixes it. Write access for the owner's user-private
  group counts as nobody else (Debian's `StrictModes` rule: the owner's primary group, no member
  but the owner; Silo adds: not root or gid 0, a local account and group whose `/etc/passwd` and
  `/etc/group` lines agree with the lookups, and no other local account with it as primary
  group), so a `~/.local/share` of mode 0775 made under umask 002 is accepted. Missing
  directories are created through directory descriptors (`mkdirat`, `O_NOFOLLOW`, `fchmod`) with
  fixed modes whatever the umask: 0755 up to `$XDG_DATA_HOME`, 0700 below it, and only inside an
  existing directory that belongs to the account or root and nobody else can change. After the store's directory is opened, every
  step goes through directory descriptors without following links (`openat`, `renameat`,
  `unlinkat`, `fsync`). A file is written whole: a new `.staged-*` file, `fsync`, `renameat` over
  the old one, then `fsync` of the directory. Staged files a crash left behind are removed when
  the engine starts and when it deletes a credential.
- **Refusals.** A link, a file or directory of another account, or a file others can read is
  refused instead of used. The refusal is `credential_access_denied` with the message
  "Silo's engine refused its credential file `<path>` `<reason>`", which names the file, never
  the value, and `engine.credentials.status` lists that item as refused.
- **Choosing the store.** `SILO_CREDENTIAL_STORE=secret-service` uses the desktop Secret
  Service instead, for a Linux machine that has one; that backend is still untested. The engine
  records the store it used first in `<data>/credential-store`. Later starts, and the helpers, use
  the recorded store when the variable is unset and refuse a different value, naming the record:
  move the credentials yourself, then delete the record.

What this means: on a Linux host the values are plain bytes on disk, readable by that account
and by root. That is the same trust the engine already gives its own state there (computer
disks, `secrets.json`, Connections keys), but weaker than the Keychain: nothing asks before a
process of the same account reads a value, nothing is encrypted at rest beyond the disk's
own encryption, and a backup of the home directory copies the values. `silod` refuses to run as
root or `nobody`; run it as a dedicated account in the `kvm` group, and use full-disk
encryption on a host that holds real credentials. See
[Linux hosts in the threat model](THREAT-MODEL.md#linux-hosts).

## Container computers

A container computer (Linux hosts, opt-in) takes secrets through the same commands, API methods
and screens as any Linux computer: `silo secrets set|assign|unassign`, the Secrets page, the
`computers.credentials` line. Only the place that holds the values differs. The engine checks the
material once with the shared rules (`secrets_runtime::validate_material`) and asks the runtime to
`modify` the computer; for a container that request is answered by `engine/src/containers/`
(`secrets.rs`), which keeps names and allowed hosts in the computer's record and hands the values
to the computer's gateway (`silo-macos-gateway`, the network stack macOS computers also use),
a host process of the engine's account. The gateway substitutes `$MSB_<name>` in request
headers, only toward an allowed host and only after verifying that host's TLS certificate;
other TLS keeps its public certificate. The guest's environment holds the placeholder.

As for a virtual machine, a secret assigned to a running container applies at its next start;
changing an assigned secret's hosts or value, and removing one, apply at once. The gateway fails
closed: an update it does not confirm stops it, the computer then reads as stopped (its network is
gone), and `silo start` brings up a new gateway with the current secrets. The gateway records the
secrets it used and the attempts it blocked in `<runtime home>/sandboxes/<name>/logs/secret-usage.jsonl`,
the file a Linux computer's events are read from, so the credentials-used line needs no
container case. Differences from a virtual machine are in the
[threat model](THREAT-MODEL.md#container-computers): a process that escapes the container lands as
the engine's account, which holds the credential store and the gateway's memory.

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

## Credentials a fork used

Forks get none of the credentials Silo manages unless you name them; files on disk are copied.
For the ones you do name, `silo status <fork>` and `silo diff <fork>` print a line such as
`credentials used: OPENAI_API_KEY → api.openai.com (POST ×3)`, or `credentials used: none`. The engine method `computers.credentials` returns the same data as
`{used: [{secret, host, method, count, lastAt}], blocked: [{secret, host, method, count, reason, lastAt}]}`.

The network proxy appends one JSON line per event to `secret-usage.jsonl`: in the sandbox's
`logs/` directory for a Linux computer, in the computer's folder for a macOS computer (whose
gateway output also goes to a `gateway.log` there, which moves to `gateway.log.1` at the next gateway start once it reaches 1 MiB). Lines hold names, hosts and
methods, never a value or a placeholder. Substitution in headers, the request line and the body is recorded, as are the GitHub profile's uses and refusals. Silo gives each ordinary secret `passthrough=*`, so a placeholder sent to a host the secret is not allowed for goes out unchanged and is not recorded; blocked attempts come from the GitHub profile, an over-long placeholder or a placeholder in a place Silo does not substitute. Blocked attempts carry a reason: `host_not_allowed`, `tls_identity_required`, `proxy_metadata`, `unsupported_location`, `placeholder_too_long`, or a `github_*` reason. A request with no host to name shows as `(unknown host)`. A file belongs to one computer, so a fork
counts from its own first request. The event file is capped at 1 MiB with one older generation, so a very busy computer loses its oldest events; a small `secret-usage-summary.tsv` beside it keeps one counter per secret, host, method and reason (at most 512 of them, then an `(other)` row) and is never rotated. `computers.credentials` reads the counters, and the events only when `since` is given. Method and host are cleaned before they are written (a method that is not a short HTTP token becomes `other`, a host over 253 bytes `(invalid host)`), and a host that contains a configured placeholder is written as `(redacted host)`. Hosts are lowercased without port or trailing dot and methods are uppercased; a used event falls back to the HTTP `Host` when the connection has no name. When reading, lines over 4 KiB are skipped, each list keeps at most 512 rows, text is capped and control characters are shown as `?`. Aggregation is `credentials_usage.rs` in the engine.

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
- Linux hosts were not supported on this date; the Secret Service backend has never been exercised.

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

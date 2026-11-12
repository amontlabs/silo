# Network implementation

Silo connects TCP development services to local loopback ports. The local computer
implementation below was verified on 2026-09-10. Remote computer services now use
[SSH tunnels through their owning device](SiloUI-CONNECTIONS.md). UDP
management, traffic inspection, and firewall editing are not implemented.

## Runtime and state

Silo reads guest TCP listeners through bounded `msb exec --no-start` calls to
`/proc/net/tcp` and `/proc/net/tcp6`. Established outbound connections are not
services. Failed inspection produces an error, never an empty success.

The bundled MicroSandbox patch extends its existing PortPublisher and control
socket with list/add/remove/probe operations. Local listeners bind only to
127.0.0.1. Automatic host ports come from the operating system; explicit port
conflicts preserve the previous mapping. Removing a mapping also closes existing
relays. A direct guest TCP handshake, subject to ingress policy, establishes
reachability. Listening or publishing alone does not mark a service reachable.

Desired mappings are saved atomically in the runtime metadata directory's
`network.json`. Failed removals remain recorded until confirmed; successful
removals are pruned. Start/restart reapplies desired mappings after releasing the
GitHub revision lock. Stopped computers stay stopped. Existing computers need one restart to
load the extended runtime; subsequent port changes are live.

Guest checks run outside the mutation lock. Observations recheck both settings
and published endpoints before reporting success, so a concurrent change cannot
publish an old reachable endpoint. Work is bounded and computer reads run in batches
of three. No extra Silo proxy or database is introduced.

## Public suffix list for allowlist wildcards

A `*.` host in a network policy is refused when its base is a public suffix, because the
rule would allow every host anyone registers under it (`*.duckdns.org`, `*.github.io`). The
engine decides this with the Public Suffix List (ICANN and private sections) from the `psl`
crate, which compiles Mozilla's list into the build. The bundled `public_suffixes.json` stays
as an addition for shared-hosting names the list does not carry (`amazonaws.com`,
`wordpress.com` and others Silo already refused) and as the early answer in the settings view.
A base is also refused when every name directly under it is a suffix (`*.sch.uk`, which the
list covers with a `*.` rule).

Compared on 2026-11-12:

| Crate | License | Last release | The list |
| --- | --- | --- | --- |
| `psl` 2.1.251 | MIT or Apache-2.0 | published most days, following the list | compiled in (no fetch, no `std`, no unsafe code); ICANN and private |
| `publicsuffix` 2.3.0 | MIT or Apache-2.0 | 2024-11 | parses a list text that Silo would have to bundle or download and refresh |
| `addr` 0.15.6 | MIT or Apache-2.0 | 2022-08 | a wrapper over an old `psl` |

`psl` was chosen: maintained, permissive, nothing to fetch at run time, two small crates
(`psl`, `psl-types`) with no further dependencies. Its cost is that the list is as old as the
locked version, so the list is refreshed by raising `psl` in `Cargo.lock` (`cargo update -p psl`
with the lock file otherwise unchanged), and a Silo release carries the list it was built with.
Adding it changed `app/SiloUI/Cargo.lock` by `psl`, `psl-types` and the engine's dependency
entry only (a plain `cargo add` also rewrote unrelated `windows` entries, which were left out).
`--locked` builds work offline once the crates are in the Cargo cache like any other dependency.

## UI

Network retains cached rows during five-second refreshes and uses skeletons for
the first load. Add port chooses a computer, guest port, optional local port, and HTTP,
HTTPS, or TCP. Adding a port exposes a service; it does not start a service.
Discovered services show the computer only. Their + action connects immediately with an
automatic local port and HTTP as the initial protocol. Edit changes the protocol
or local port with Save/Cancel; computer and guest port stay fixed. The response
includes configuredHostPort separately from the actual endpoint so editing the
protocol preserves Automatic or an explicit local override. Addresses use actual host ports.
Waiting, not exposed, and unknown states remain distinct. Errors stay compact;
removal uses the existing inline Cancel/Remove confirmation pattern.

Browser actions use the current browser setting and recheck the endpoint before
opening it. Status-bar Open site shares this state. The status window can read
and open ports but cannot change mappings. Fixtures remain in preview/test code.

### Computer hostnames and Safari

Published websites use a per-computer `.localhost` hostname. Silo passes this
hostname unchanged to the selected browser. Safari resolves these names
automatically on macOS 26 onward; Silo also supports macOS 14 and 15, where
users should select Chrome or Firefox. The existing **Copy 127.0.0.1 address**
action bypasses hostname resolution, but shares a cookie hostname with other
services opened at that IP, regardless of port.

Primary evidence: [WebKit bug 160504, comment 19](https://bugs.webkit.org/show_bug.cgi?id=160504#c19)
reports failure on macOS 15.7 and success on macOS 26.0; [comment 20](https://bugs.webkit.org/show_bug.cgi?id=160504#c20)
confirms that the fix was implemented in OS frameworks. This review checked
Silo's URL construction and launch code; it did not launch Safari or a live VM.

## Verification

Focused frontend tests cover loading, cached refresh, errors, add/remove,
confirmation dismissal, actual addresses, browser settings, and status actions.
Native tests cover listener parsing, validation, persistence, and control replies.
Runtime tests cover binding, conflicts, idempotence, removal, and probe failure.
### macOS live evidence, 2026-09-10

The final signed bundled runtime was exercised with two disposable 512 MiB VMs
using `public.ecr.aws/docker/library/python:3.12-alpine`. Both VMs and their
isolated runtime home were removed afterwards; the existing `dev` data was not
changed by this runtime test.

Verified:

- HTTP response bodies complete and deliver EOF; HTTPS passes through unchanged.
- Binary TCP request/response works, and removing a mapping closes an established
  connection.
- Automatic ports are nonzero and idempotent. An occupied explicit port preserves
  the existing working mapping. Two VMs can publish guest port 3000 simultaneously
  with different local ports.
- A removed local port can immediately be published again.
- Direct probes report success for IPv4 and dual-stack services, and failure for
  guest-loopback-only, IPv6-only (with the runtime's IPv4 guest target), and absent
  services. No temporary host listener is created by a probe.
- `msb ssh serve --stdio --no-start` rejects a stopped VM without starting it.

The live test exposed an existing upstream publisher bug: guest EOF was not
forwarded to the local client, so a normal HTTP/1.0 response could hang after its
headers. The patch now preserves queued bytes and forwards TCP half-closes in
both directions. A focused regression verifies that a client can finish its
request, half-close, receive the complete response, and observe EOF. All 11
publisher tests passed with loopback socket permission. The patched source also
passed the full release CLI build; the final repeat build was a 0.63-second no-op.

The runtime patch is checked against the pinned source archive before packaging.
Silo owns desired mappings and reapplies them after boot; the runtime control
socket owns only currently bound listeners. Current control operations are
`ports_list`, `port_add`, `port_remove`, and `port_probe`. Probe work is bounded
(16 queued probes, 500 ms handshake deadline, 800 ms response wait); a failed
probe request remains an error rather than reporting a service ready.

## Native Silo UI verification

The production development bundle at
`app/SiloUI/src-tauri/target/debug/bundle/macos/Silo.app` was rebuilt with
`npm run desktop:build:debug` and driven through macOS accessibility. No fixture
mode or test UI was enabled.

- Network → Add port → dev, guest port 43117, automatic local port, HTTP:
  row showed Waiting for service with its actual local address.
- A temporary Perl HTTP server inside dev made the row Reachable. The address
  returned `Silo network test OK` with curl; Open in Zen displayed the same text.
- Remove showed inline Cancel/Remove, removed the host listener, and left the
  guest service visible as Not exposed. The old local address refused connections.
- A saved waiting port survived Restart dev and received a new automatic local
  port. It did not become Reachable without a service.
- The temporary server/log and port rules were removed. Final Network showed
  No configured ports. The final app remains open with dev Running; existing
  files, GitHub settings, and secrets were preserved.

Validation: 569 frontend tests, 236 native tests (6 integration tests ignored by
ordinary runs), TypeScript checking, lint, and the final desktop build passed.
The SSH/SCP integration test was also run explicitly against the canonical bundle
and passed. Live Zed proof is recorded in SiloUI-EDITOR-HANDOFF.md. Status menu
behavior is covered by component/bridge/permission tests; its native tray click
was not accessible to the UI driver. Linux and VS Code were not tested live.

## Primary references

- [Pinned networking overview](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/docs/networking/overview.mdx)
- [Pinned builder](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/sdk/rust/lib/sandbox/builder.rs)
- [CLI documentation](https://github.com/superradcompany/microsandbox/blob/main/docs/cli/sandbox-commands.mdx)

Linux code paths require verification on a Linux desktop before release; macOS
results do not prove Linux application launching or runtime packaging.

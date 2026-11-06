# Silo threat model

This document says what Silo protects, against whom, by which mechanism, and what it does
not protect. Every mechanism cites the code that implements it, so a reader can check the claim
against the source. It describes `main` at the commit this file was last changed in; when the code
and this document disagree, the code is right and this document has a bug.

Scope: the Silo engine (`silod`, `silo-engine`, the bundled MicroSandbox runtime and the macOS
network gateway) on its only supported host today, an Apple Silicon Mac. Linux computers and
macOS computers both run as guests on that Mac. `silod` also builds for Linux hosts; no Linux
release ships yet, and what changes there is in [Linux hosts](#linux-hosts). Everything else
describes the Mac.

## Summary

Silo assumes that whatever runs inside a computer is hostile: an agent that has read a malicious
issue, web page or dependency and now follows the attacker's instructions with root in the guest.
Silo tries to keep that agent from reaching the Mac, the Mac's network, other computers, raw
credentials and repositories the user did not grant. It does **not** try to keep the agent from
sending the data it can already see to the internet: outbound traffic to public addresses is
allowed, and there is no per-host egress filter yet.

| The agent in a computer can | The agent in a computer cannot (if the boundary holds) |
| --- | --- |
| Read and change everything in that computer, as root | Read or write the Mac's files, except, on Linux computers, the runtime's own read-write control folder at `/.msb` and two read-only folders lent to computers with built-in computer use; macOS computers get no host folder |
| Connect to any public internet address, any port (except UDP to port 443, which blocks QUIC) | Connect to the Mac's loopback, private-range (LAN, VPN) addresses, link-local or cloud-metadata addresses, or another computer (macOS computers: only once setup is complete) |
| Use an assigned secret against that secret's allowed HTTPS hosts | Read the secret's value, or send it to a host outside the secret's allowed list (unless the user assigned `*`; see boundary 3) |
| Read granted repositories; write only those granted for changes | Read the user's GitHub sign-in or any GitHub token; touch ungranted repositories (OAuth mode) |
| Send the data it can read to any public host | Talk to the engine socket on the Mac |

## Attacker

**Primary attacker: a prompt-injected agent in a guest.** It controls every process in one computer
and has root there: Silo's working account `silo` has passwordless `sudo`
([`working-account.py#L276`](../app/SiloUI/src-tauri/guest/working-account.py#L276), and the same
for macOS computers in [`offline_setup.rs`](../app/SiloUI/src-tauri/engine/src/macos_computers/offline_setup.rs#L935)).
It can install software, change the guest's trust store, kernel parameters and network settings,
and craft any packet, DNS query or HTTP request. It sees the placeholders for its secrets and the
read-only folders lent to it. It knows how Silo works, because Silo is open source.

**Secondary inputs it can shape**, which Silo's host side reads and must treat as untrusted:
guest web content shown in the desktop viewer, file names and sizes in `/workspace`, Git
repositories that the user pushes from the host, output relayed into logs, files the user downloads
from the computer, clipboard content copied out of a computer, the RFB stream of a macOS screen
viewed from another Mac, the heartbeat and other files the guest writes into `/.msb`, exported computer archives that are later imported, and the SNI, HTTP request line,
headers and body of GitHub requests, which the proxy parses on the Mac to choose a token.

Not an attacker in this model: the user, apps the user installs on the Mac, and other processes
running as the same macOS user (see [non-goals](#non-goals)).

## Assets

In rough order of damage if lost:

1. **The Mac and its user account**: files, processes, Keychain, browser sessions.
2. **The user's GitHub authority**: the OAuth user token in the Keychain, any personal access token,
   and repositories the user did not grant to this computer.
3. **Secret values** stored in the Keychain and assigned to computers (API keys and similar).
4. **Services reachable from the Mac but not from the internet**: anything on the Mac's loopback,
   the LAN, a VPN, or cloud metadata endpoints.
5. **Other computers** on the same Mac and their data.
6. **The engine**: `silod`, which can create, start, export and delete every computer, and change
   GitHub and secret assignments.
7. **Data inside the computer** (source code, repository contents, files the user put there). Silo
   does not protect its confidentiality against the agent; see
   [what Silo does not protect](#what-silo-does-not-protect).

## Trust boundaries

### 1. The virtual machine

| | |
| --- | --- |
| Linux computers | MicroSandbox 0.7.6 (libkrun with its own libkrunfw kernel, on Hypervisor.framework), pinned by commit and SHA-256 in [`runtime-inputs.json`](../app/SiloUI/runtime-inputs.json), with Silo's patches in [`patches/`](../app/SiloUI/patches). |
| macOS computers | Apple's Virtualization.framework, configured in [`macos_computers/engine.rs`](../app/SiloUI/src-tauri/engine/src/macos_computers/engine.rs). |
| What it gives | A separate kernel per computer. Guest root is not host root. |
| Host side | The `msb` runtime and the virtual devices it implements run as the user's own process, with the hardened runtime, the hypervisor entitlement and `disable-library-validation` (library
loading is instead pinned by a code-hash library constraint), and **without** the macOS App Sandbox ([`macos_release_signing.py#L17`](../app/SiloUI/scripts/macos_release_signing.py#L17)). `silod` has only the virtualization entitlement ([`Entitlements.plist`](../app/SiloUI/src-tauri/silod/macos/Entitlements.plist)). |
| Residual risk | A bug in libkrun, its virtio devices, the network stack or Virtualization.framework that gives code execution in the host process gives the attacker the user's account. Silo adds no second layer (App Sandbox, separate user) today. |

### 2. Network egress: public addresses only

Silo passes no `--net-*` rule when it creates a Linux computer, so the computer gets MicroSandbox's
default policy, the `Public` profile
([MicroSandbox `types.rs`](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/network/lib/model/policy/types.rs#L823)).
The `--net-strict=true` flag
([`runtime/mod.rs#L6533`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L6307)) only pins
hostname-rule enforcement and has no effect while no hostname rule exists. On checkpoint restore,
Silo re-passes the same rules from the computer's recorded policy
([`checkpoints.rs#L467`](../app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs#L467)). The resulting
configuration, which exports compare against, is
[`github-network-default.json`](../app/SiloUI/src-tauri/guest/github-network-default.json):

- `default_egress: deny`, then allow DNS (port 53) to the gateway, and allow every protocol and port
  to the `public` destination group. `default_ingress` is `allow`, but nothing reaches the guest
  except through the published ports described below.
- `public` is decided by destination IP. Loopback, RFC 1918, CGNAT `100.64.0.0/10`, IPv6 ULA,
  link-local, `169.254.169.254`, multicast, the gateway itself and unspecified addresses are not
  public, and everything else is, including reserved ranges such as `198.18.0.0/15` and
  `240.0.0.0/4` ([MicroSandbox `destination.rs`](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/network/lib/engine/policy/destination.rs)).
- DNS goes through the gateway, which forwards to the Mac's own resolvers (`nameservers: []`), with
  `rebind_protection: true`. Rebind protection returns NXDOMAIN for any answer in a private or
  reserved range, so LAN and VPN names that resolve to private addresses do not resolve; it does
  not stop connecting by IP. Query names still reach the Mac's resolvers, including a VPN's
  split-DNS resolvers, and DNS queries are an unfiltered outbound channel: the guest can also talk
  directly to any public resolver.

So the guest cannot open a connection to a service listening on the Mac's loopback or LAN address,
to a router admin page, to a Tailscale peer, or to another computer's guest address.

macOS computers that have finished setup run behind a per-computer gateway child,
`silo-macos-gateway` ([source](../app/SiloUI/src-tauri/macos-gateway/README.md)), which uses
the same MicroSandbox network stack. The VM's only network device is a file-handle attachment to
that child ([`engine.rs`, `start_machine_once`](../app/SiloUI/src-tauri/engine/src/macos_computers/engine.rs)),
and the child applies the `Public` profile when no other policy is given
([`network.rs#L147`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L147)).

Limits of this boundary:

- **No hostname or destination allowlist.** Any public host, any port, TCP and UDP (except UDP to port 443). This is what
  "no egress filter yet" means.
- **macOS computers before setup completes use Apple NAT.** Until a macOS computer's setup,
  personalization and network bootstrap are complete, and when it boots into Recovery, it runs on
  `VZNATNetworkDeviceAttachment` with no Silo policy
  ([`boots_behind_gateway`, `engine.rs#L959`](../app/SiloUI/src-tauri/engine/src/macos_computers/engine.rs#L959);
  [`engine.rs#L577`](../app/SiloUI/src-tauri/engine/src/macos_computers/engine.rs#L577)). In that
  state it can reach the Mac and the LAN. Silo's own provisioning is the expected workload then,
  but a user can start a computer whose setup failed.
- The classification is by IP, with no notion of "the LAN" beyond the private ranges. A LAN host
  reachable by a global IPv6 address (SLAAC) or a non-RFC 1918 IPv4 address is "public" and
  reachable, as is a service on the Mac's own public address, if it has one. The IPv6 case applies
  to Linux computers only; macOS computers have no guest IPv6
  ([`macos-gateway/src/network.rs#L158`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L158)).

**Ingress** (into the guest) is opened only by the user: published ports bind `127.0.0.1` on the Mac
([`microsandbox-live-public-ports-0.7.6.patch`](../app/SiloUI/patches/microsandbox-live-public-ports-0.7.6.patch)),
and opt-in SSH access binds `127.0.0.1`, plus one chosen interface address when the user picks
one, never a wildcard
([`ssh_access.rs#L132`](../app/SiloUI/src-tauri/engine/src/ssh_access.rs#L132)). Any local process
on the Mac can reach a published port. Every macOS computer behind its gateway also always
publishes guest ports 22 (SSH) and 5900 (VNC) on Mac loopback at ephemeral ports, for Silo's own
use; the gateway refuses any other set
([`macos-gateway/src/network.rs#L113`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L113)).

### 3. The placeholder proxy (secrets)

Secret values live in the Keychain ([`credential_store.rs`](../app/SiloUI/src-tauri/engine/src/credential_store.rs)).
The guest never receives a value. It receives an environment variable whose content is a
placeholder such as `$MSB_OPENAI_API_KEY`. The runtime on the Mac holds the value and replaces the
placeholder in outbound requests ([docs/SiloUI-SECRETS.md](SiloUI-SECRETS.md)):

- Each secret is passed to the runtime as `NAME:passthrough=*@host,...`
  ([`secrets_runtime.rs#L139`](../app/SiloUI/src-tauri/engine/src/secrets_runtime.rs#L139)), with
  the value delivered to the runtime process on standard input (`MSB_SECRET_VALUES_STDIN`), never
  in arguments, the configuration file or the host environment; the stored configuration holds
  only an opaque source name `SILO_SECRET_<hash>`
  ([`runtime/mod.rs#L1841`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L1803),
  [`microsandbox-secret-values-stdin-0.7.6.patch`](../app/SiloUI/patches/microsandbox-secret-values-stdin-0.7.6.patch)). MicroSandbox's parser then sets
  substitution to **headers only** (no query, no body) and, on Linux computers, lets the unchanged
  placeholder go to any host, `passthrough=*` ([MicroSandbox `common.rs` `parse_secret`](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/cli/lib/commands/common.rs#L2893)).
- The real value is written only into intercepted TLS requests whose SNI and DNS-resolved
  destination match one of the secret's allowed hosts, with the upstream certificate verified
  (`require_tls_identity`, the runtime default, which
  [`secrets_runtime.rs`](../app/SiloUI/src-tauri/engine/src/secrets_runtime.rs) confirms is not
  disabled when it verifies the runtime configuration). Plain HTTP never receives it. A wildcard
  allowed host such as `*.example.com` delivers the value to every matching subdomain.
- Changing or removing a secret closes the computer's open TLS connections on port 443,
  intercepted or relayed, so a connection made under the old policy cannot keep using it (`until_policy_change` in
  [`microsandbox-silo-network-0.7.6.patch#L1941`](../app/SiloUI/patches/microsandbox-silo-network-0.7.6.patch#L1941)).
- macOS computers get the same secrets through their gateway child
  ([`macos-gateway/src/network.rs#L179`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L179)),
  with one difference: no passthrough hosts, so a placeholder sent to a host that is not allowed
  hits the violation action instead of passing through unchanged. The engine sends the values to
  that child, a process of the same user, over its private control socket.
- Secret names that would override proxy, TLS, loader or Silo variables are refused
  ([`secrets.rs`, `reserved_secret_name`](../app/SiloUI/src-tauri/engine/src/secrets.rs)).
- Backups and checkpoints never carry values; a restored computer uses the current assignments.

What the agent can still do with a secret: **use** it. Anything the allowed host lets that key do,
the agent can do (spend credits, read the account, delete resources). And an allowed host that
echoes request headers back gives the agent the value ("reflected-secret exfiltration", a
[non-goal](#non-goals)). The `*` (any host) assignment exists, requires an explicit acknowledgement in the Secrets
page (the engine API itself accepts `*`), removes this boundary for that secret, and puts every
TLS connection of that computer under the interception CA.

### 4. TLS interception scope

To substitute a header, the proxy must terminate TLS, so it presents a certificate from the
computer's interception CA, which the guest trusts. Silo patches MicroSandbox so that only
connections whose SNI matches a **current secret's allowed hosts** are intercepted; every other TLS
connection is relayed with the server's own certificate
([`microsandbox-silo-network-0.7.6.patch#L1991`](../app/SiloUI/patches/microsandbox-silo-network-0.7.6.patch#L1991),
with its test `intercepts_only_current_secret_destinations`). Interception is limited to port 443, and
all UDP to port 443 (QUIC) is dropped for every destination, intercepted or not; the proxy verifies the upstream certificate
(`intercepted_ports`, `block_quic_on_intercept`, `verify_upstream` in
[`github-network-default.json`](../app/SiloUI/src-tauri/guest/github-network-default.json)).
macOS gateways use the same settings by runtime default, and the gateway refuses an
intercepted-port set without 443 ([`network.rs#L68`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L68)).

Every Linux computer is created with the GitHub secret, so `github.com`, `api.github.com` and
`uploads.github.com` are always intercepted. A macOS computer with a GitHub profile has
`github.com` and `api.github.com` intercepted
([`macos-gateway/src/network.rs#L194`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L194)).
For macOS computers the CA is per computer: it is stored in the Mac's Keychain
(`engine.credentials.status` lists "one proxy CA per macOS computer",
[docs/SiloUI-ENGINE.md](SiloUI-ENGINE.md)) and written to a private temporary file for the gateway
process while the computer runs
([`macos-gateway/src/network.rs#L132`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L132)).

Linux computers share one interception CA, generated by the runtime and kept as a `0600` file
under the runtime home; it is not per computer and not in the Keychain. Only its certificate is
written to the computer's `/.msb/tls/`.

The CA's private key never enters the guest. The guest is root, so it can remove the CA or add its
own; that only breaks its own connections.

### 5. GitHub: tokens minted on the host, never shown to the guest

The OAuth user token stays in the Keychain on the Mac. For each computer, Silo asks GitHub for
child tokens through `POST /applications/{client_id}/token/scoped`
([`github_tokens.rs#L329`](../app/SiloUI/src-tauri/engine/src/github_tokens.rs#L329)), restricted to:

- **one owner** and the **selected repository IDs** (or all repositories of that owner's App
  installation when the user picks "all");
- **read permissions by default**: `metadata`, `contents`, `issues`, `pull_requests`, `statuses`,
  `checks`, all `read` ([`COMPUTER_READ`, `github_tokens.rs#L57`](../app/SiloUI/src-tauri/engine/src/github_tokens.rs#L57)).
  New grants start with changes off
  ([`github-page.tsx#L51`](../app/SiloUI/src/features/application/pages/github-page.tsx#L51));
- a **write** token only for repositories where the user allowed changes, with `contents`,
  `issues`, `pull_requests`, `statuses` and `workflows` write, and `metadata`, `checks` and `actions`
  read
  ([`COMPUTER_WRITE`, `github_tokens.rs#L66`](../app/SiloUI/src-tauri/engine/src/github_tokens.rs#L66)).
  Admin and secret-bearing permissions are never requested.

On Linux computers these tokens are handed to the runtime as the `SILO_GITHUB` secret
([`runtime/mod.rs#L6303`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L6303)); on macOS
computers the same profile goes to the gateway over its private control descriptor
([`github.rs#L1305`](../app/SiloUI/src-tauri/engine/src/github.rs#L1305),
[`macos-gateway/src/network.rs#L194`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L194)),
which links the same patched proxy. The guest's
`GH_TOKEN` is the placeholder `$MSB_SILO_GITHUB`
([`runtime/mod.rs#L6309`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L6309)). For each
request to the GitHub hosts (three on Linux computers, `github.com` and `api.github.com` on macOS
computers) the patched proxy picks the owner and the read or write token
([`microsandbox-silo-network-0.7.6.patch#L385`](../app/SiloUI/patches/microsandbox-silo-network-0.7.6.patch#L385));
an unknown owner, a write without a write token, or an expired token is blocked. A request that
names no owner uses the computer's only owner, or for reads the alphabetically first owner; such a
write with several owners is blocked. For a GraphQL request with several owners and no
`variables.owner`, the proxy itself runs bounded, read-only introspection probes with the owners'
read tokens to find the owner, and never forwards their responses to the guest. Only HTTP/1.1 is
routed to the GitHub hosts; HTTP/2 there is blocked. Substitution is header-only and needs an
intercepted TLS connection. That selection is
routing, not the security boundary: **GitHub enforces the token's repositories and permissions**.
A guest that tricks the selector gets at most a request sent with another token this computer
already holds; it never sees a token.

Host push is a separate, user-confirmed path: the Mac fetches the committed objects from the guest
over a private SSH transport, then pushes one confirmed commit to one branch. In OAuth mode the token is limited to
`metadata: read` and `contents: write` on that single repository and revoked when the push ends
([`HOST_PUSH`, `github_tokens.rs#L77`](../app/SiloUI/src-tauri/engine/src/github_tokens.rs#L77);
[`push_authorized`, `github.rs#L2439`](../app/SiloUI/src-tauri/engine/src/github.rs#L2439)). Git
reads the token from an inherited pipe, never from arguments, environment or files
([`host_push.rs`](../app/SiloUI/src-tauri/engine/src/host_push.rs)).

Limits:

- **Personal access token mode has no repository scoping.** The proxy sends the personal token for
  every request to the GitHub hosts (`profile.version == 2`,
  [patch line 394](../app/SiloUI/patches/microsandbox-silo-network-0.7.6.patch#L394)), so the agent
  has the token's full permissions. The README says so. Host push in this mode also uses the full
  personal token, which is not narrowed or revoked
  ([`github.rs#L2369`](../app/SiloUI/src-tauri/engine/src/github.rs#L2369)).
- **Read is still read.** With a read grant the agent can read every granted repository, including
  private ones, and send their contents anywhere (boundary 2 does not stop it).
- A write grant includes `workflows: write`: the agent can change GitHub Actions workflows in those
  repositories, and whatever those workflows can reach.

### 6. The `/workspace` disk and host folders

Each Linux computer's `/workspace` is an owned disk image attached as a block device
(`--mount-owned /workspace:kind=disk`,
[`runtime/mod.rs#L6518`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L6292)), and its root
filesystem is a separate disk. No host folder of the user's is shared into a computer. A Linux
computer sees one host folder, and a computer created with built-in computer use sees up to two
more. macOS computers get none.

The first is MicroSandbox's own per-computer runtime directory, mounted **read-write** at `/.msb`
(virtiofs tag `msb_runtime`). It is a control channel: the host writes scripts and TLS
certificates into it, and the guest writes a heartbeat file that the host reads. MicroSandbox
serves it from a passthrough filesystem rooted at that directory, refuses a symlinked root, and
caps guest writes at 16 MiB
([MicroSandbox `vm.rs`](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/runtime/lib/runner/vm.rs#L2014),
[`protocol/lib.rs`](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/protocol/lib/lib.rs#L81)). The same directory
also holds MicroSandbox's per-computer checkpoint state (`root-disk.json`, `checkpoint-store/`,
`restore-activation.json`, `runtime-boot-id`), which the host parses with size bounds and content
hashes but which the guest can overwrite or delete. An eval run on a Mac
found the share read-write from the guest and could not reach a host canary file through it.
**This share has not had a security review.** The guest can change what the host later reads
there, and any escape from the passthrough root would be a host file write.

The other two, present only on computers created before Cua Driver (v4 guest image), are
Silo-managed **read-only** legacy shares at `/opt/silo/chatgpt` and `/opt/silo/lcu`. Silo no
longer downloads ChatGPT or LCU, so both host folders are empty: Silo recreates them empty before a
start, restore or fork, passes them again on a checkpoint restore, strips them from exports, and
new computers never get them ([`legacy_mounts.rs`](../app/SiloUI/src-tauri/engine/src/legacy_mounts.rs)).

Host features read the disk only through the guest: directory listings run a bounded script in the
guest without following links ([`files.rs`](../app/SiloUI/src-tauri/engine/src/files.rs)), and
file transfers use `sftp` as the working account with sizes bounded on the host
([`transfer.rs`](../app/SiloUI/src-tauri/engine/src/transfer.rs)). The two read-only shares are
served by the `msb` process on the Mac (virtiofs), so that process parses the guest's filesystem
requests; read-only is enforced on the host side.

Imported Linux computer archives are parsed on the Mac: a zstd-compressed tar is streamed through
caps on entry types, paths, entry count and size
([`backup.rs#L3552`](../app/SiloUI/src-tauri/engine/src/backup.rs#L3552)).

Exception: **macOS computer disks are attached on the Mac** with `hdiutil`, and their Data volume
is mounted with `diskutil` while Silo edits it, during first setup and when an exported macOS computer is imported
([`offline_setup.rs`](../app/SiloUI/src-tauri/engine/src/macos_computers/offline_setup.rs);
import at [`macos_computers.rs#L3230`](../app/SiloUI/src-tauri/engine/src/macos_computers.rs#L3230)).
An imported archive from a computer an agent controlled is therefore parsed by the Mac's APFS
driver. Silo also reads the imported account record (`dslocal/.../silo.plist`) and a fixed list of
launch agents and receipts from that volume with the `plist` crate, under a size bound
([`offline_setup.rs#L354`](../app/SiloUI/src-tauri/engine/src/macos_computers/offline_setup.rs#L354)). Import only
archives you trust.

### 7. The engine socket: same user only

`silod` listens on `~/.silo/engine.sock` (`~/.silo-dev` for Silo Dev). The directory must be owned
by the engine's user with mode `0700`
([`paths.rs#L85`](../app/SiloUI/src-tauri/silod/src/paths.rs#L85)), the socket is set to `0600`
([`socket.rs#L122`](../app/SiloUI/src-tauri/silod/src/socket.rs#L122)), and every accepted connection
is checked with `getpeereid` before a frame is read: a different effective UID is dropped (the check compares the effective UID only; group and
process identity are not checked)
([`socket.rs#L165`](../app/SiloUI/src-tauri/silod/src/socket.rs#L165)).

That is the whole client check. **Any process running as the same user has the engine's full
authority**: it can list, export or delete computers, change GitHub and secret assignments, and
open the guest over SSH. There is no authenticated client identity (the handshake's client name
is self-declared) and no audit log of which client asked for what. The guest is not such a
process: the socket is on the Mac's filesystem, which the guest cannot see.

A second path exists for Connections. With **Allow connections from other devices** on, a paired
device reaches this engine over the Mac's SSH server. Its key is installed with
`restrict,command="exec ~/.local/bin/silo-remote --remote-bridge"`
([`remote.rs#L26`](../app/SiloUI/src-tauri/engine/src/remote.rs#L26),
[`channel.rs#L106`](../app/SiloUI/host/src/channel.rs#L106)): no pty, forwarding or agent, and
only that command whatever the client asked for (`SSH_ORIGINAL_COMMAND` is never read). The link
names `silod` inside the app bundle. Started through it, `silod` accepts no further arguments,
replaces itself with the file in its bundle
([`lib.rs#L60`](../app/SiloUI/src-tauri/silod/src/lib.rs#L60)), verifies that bundle's signature
as the engine does ([`app.rs#L49`](../app/SiloUI/src-tauri/silod/src/app.rs#L49)), reads one framed
request from its input and relays it to `~/.silo/desktop-remote/control.sock` (`0600` in a `0700`
directory, no peer-UID check;
[`remote.rs#L2893`](../app/SiloUI/src-tauri/engine/src/remote.rs#L2893)). The owner refuses the
request unless Connections are on, the protocol version matches and, except for the handshake,
the request names this device's id
([`remote.rs#L3136`](../app/SiloUI/src-tauri/engine/src/remote.rs#L3136)); the id is not a secret
(the handshake returns it), so the key is the whole check. The owner then serves the methods in
`METHODS` and `OWNER_METHODS`
([`remote.rs#L295`](../app/SiloUI/src-tauri/engine/src/remote.rs#L295),
[`#L365`](../app/SiloUI/src-tauri/engine/src/remote.rs#L365)), including delete, checkpoint
restore, guest exec, file reads and writes, exports, and the `guest.ssh` and macOS display byte
streams. Turning Connections off refuses every request but leaves installed keys in
`authorized_keys`; removing a device revokes its key. A device that holds that key, or a
same-user process that connects to `control.sock`, has that authority. The same-user link under
`~/.local/bin` is part of that trust: a same-user process can repoint it, which is a non-goal
below.

At start, `silod` verifies its own bundle signature against `SiloEngineCodeRequirement`, and a
production engine refuses to run without one
([`code_signature.rs`](../app/SiloUI/src-tauri/silod/src/code_signature.rs)). That protects which
binary serves the socket, not who connects to it.

### 8. Host surfaces that read guest-controlled data

| Surface | Control | Code |
| --- | --- | --- |
| Linux desktop viewer | Guest web content loads in a child webview that matches no Tauri capability; only the local `desktop-shell-*` webview has desktop permissions. The loopback desktop proxy checks Host and Origin, uses an HttpOnly cookie, strips client credentials, and reaches the guest through a Unix socket in a private directory. The guest page answers bridge requests only on the proxy's reserved `/__silo/v1/` route with a single-use nonce. | [`capabilities/desktop-viewer.json`](../app/SiloUI/src-tauri/capabilities/desktop-viewer.json), [`desktop_viewer.rs`](../app/SiloUI/src-tauri/engine/src/desktop_viewer.rs), [`desktop_proxy.rs`](../app/SiloUI/src-tauri/engine/src/desktop_proxy.rs) |
| macOS screen, same Mac | Native `VZVirtualMachineView`, no webview. | [docs/SiloUI-MACOS-COMPUTERS.md](SiloUI-MACOS-COMPUTERS.md) |
| macOS screen, another Mac | A noVNC page in a Tauri webview speaks RFB to the guest's Screen Sharing through a loopback WebSocket endpoint (random per-window token, Host and Origin checks, one connection, message size limit) spliced to the owner over the SSH bridge. The webview parses guest-controlled RFB data. | [`macos_display_gateway.rs`](../app/SiloUI/src-tauri/engine/src/macos_display_gateway.rs), [display path](SiloUI-MACOS-COMPUTERS.md#on-another-mac) |
| Clipboard | Inside the viewer, both directions run only on an explicit user action (shortcut, menu item or button). An engine client can trigger the same transfer for a Linux computer it is viewing (`clipboard.transfer`), and read or write a macOS computer's clipboard (`clipboard.read`, `clipboard.write`). There is no background sync. | [`viewer_clipboard.rs`](../app/SiloUI/src-tauri/engine/src/viewer_clipboard.rs), [`display.rs#L1009`](../app/SiloUI/src-tauri/silod/src/methods/display.rs#L1009), [`macos_computers.rs`, `macos_clipboard_read`](../app/SiloUI/src-tauri/engine/src/macos_computers.rs#L2489) |
| Host push | Host Git uses a sanitized configuration and fixed executable; the guest supplies objects through standard protocols, not hooks or configuration. | [`host_push.rs`](../app/SiloUI/src-tauri/engine/src/host_push.rs), [`host_push_transport.rs`](../app/SiloUI/src-tauri/engine/src/host_push_transport.rs) |
| Editor and terminal | The user's editor, and a terminal for a computer on another device, connect over SSH with no agent forwarding (`ForwardAgent no`, `IdentityAgent none`), a per-computer `known_hosts` and `StrictHostKeyChecking yes`; guest paths are URL-encoded and control characters refused. A local terminal for a Linux computer runs `msb exec --tty` through MicroSandbox's guest agent, not SSH; a local terminal for a macOS computer runs `/usr/bin/ssh -t` with the same no-forwarding, pinned-host-key settings ([`ssh_alias.rs#L126`](../app/SiloUI/src-tauri/engine/src/macos_computers/ssh_alias.rs#L126), [`#L258`](../app/SiloUI/src-tauri/engine/src/macos_computers/ssh_alias.rs#L258)). The editor's own remote trust model still applies. | [`editor.rs`](../app/SiloUI/src-tauri/engine/src/editor.rs), [`terminal.rs`](../app/SiloUI/src-tauri/engine/src/terminal.rs) |
| Guest agent | In every Linux computer, MicroSandbox's `agentd` talks to the `msb` process on the Mac over virtio-serial (CBOR) for readiness, exec and heartbeats. Silo parses `msb exec` output (repository discovery, directory listings). Silo configures no virtio-vsock routes. | [`runtime/mod.rs`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs), [MicroSandbox `protocol/lib.rs`](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/protocol/lib/lib.rs) |
| Published ports | Guest services appear on Mac loopback; opening one in a browser runs guest content at a per-computer `.localhost` origin; a page there can still send cross-site requests to other loopback services ([`network.rs#L87`](../app/SiloUI/src-tauri/engine/src/network.rs#L87)). | [`network.rs`](../app/SiloUI/src-tauri/engine/src/network.rs) |

### 9. The in-guest client: a scoped token for a computer's own agent

Design and install steps: [the in-guest client](SiloUI-IN-GUEST-CLIENT.md). A Linux computer can be
given a token (`silo guest-client install`) that lets an agent in it call a short list of engine
methods through the `/.msb/silo-bridge` folder. The engine serves it from
[`guest_bridge.rs`](../app/SiloUI/src-tauri/silod/src/guest_bridge.rs) and checks each call in
[`guest_client/scope.rs`](../app/SiloUI/src-tauri/engine/src/guest_client/scope.rs).

What the token can do: fork its own computer (secrets only those the settings list for that agent,
none by default; at most 32 forks per token and 3 running or starting), start, stop and diff the
forks it made, ask for review of those forks, read the review requests and steps of itself and those
forks, and follow the jobs it started. Forks the person made, or another token made, are not
reachable, and list results hold only the computer and its own forks.

What it cannot do, and the engine refuses before any handler runs: keep, promote, undo, throw away
or drop (`changes.keep`, `changes.promote`, `changes.undo`, `changes.requests.keep`,
`changes.requests.throwAway`, `computers.delete`), change secrets or GitHub access, run commands or
move files in any computer, publish ports, act for another device, act on a computer that is
neither itself nor a fork it made, or mint, revoke or read tokens. It cannot make checkpoints,
capture or baseline either. `fold` and `auto_keep` are not part of its policy: nothing is kept from
inside.

What a compromised guest gains: the agent already controls the computer, so the token adds the
ability to make up to 32 forks of it, 3 running at once (a disk and CPU cost; the engine adds no
total RAM limit, each running fork uses the memory it was created with, and `fork_ram_budget`
applies only to forks the host CLI makes) and to put review requests in front of
the person, whose summary text the agent wrote. A fork, request or summary can be flooded or
misleading; a keep still shows the engine's own diff, pinned to its digest. It gains no host file,
secret, GitHub token or other computer's data.

- **Replay and theft.** Frames are files the guest writes, so anything in the guest that can read
  the token file can use the token, and a copy of it elsewhere is refused unless it is in the
  folder of the computer it was minted for. There is no per-request signature or nonce check; the
  folder is the identity, and the token is the installer's intent. Request ids are consumed when
  read, and answers expire after ten minutes.
- **Forks.** A token verifies only for its own computer, so a fork that kept the file cannot use it
  and forks made from inside also have the file scrubbed.
- **Promote and undo.** Both revoke the tokens of the computers they swap, so the agent in a former
  main stops; a new token is installed on purpose.
- **Parsing and links.** The guest can replace `silo-bridge`, `req` or `resp` with links, and on a
  macOS host a guest symlink is a real host symlink. The engine opens each component with
  `openat(O_NOFOLLOW | O_DIRECTORY)` through directory descriptors (`rustix`), afresh each pass,
  and stops serving a computer whose folder is a link. Request files are regular files only
  (checked after opening), at most 256 KiB, `<32 hex>.json` names only; answers are written `0600`
  by rename. At most 64 answers wait unread and a full folder drops new requests; a call with no
  valid token gets no answer; at most 256 directory entries are examined per look. The share itself
  still has no security review (boundary 6); this adds a parser of guest-written JSON on the Mac.
- **Other guest users.** `req/` is `1733` and `resp/` `0711`, requests and answers `0600`; the
  engine gives each answer the stat override of the user that wrote the request, so that user
  (and root) reads it and no other guest user does. The override format is taken from MicroSandbox
  0.7.6 source and is not yet checked in a running guest.
- **Forks and jobs.** A fork is the token's only after its job finished and the fork exists below
  the computer (recorded by id); forks the person made are never reachable, and a failed fork
  never claims a computer. A token holds at most 32 forks, 3 running or starting, and 16
  unacknowledged jobs.
- **Logging.** The engine writes one line per call to its log: the computer, the method (only a
  name from the allow-list; anything else is logged as "not offered"), and whether it was refused.
  Parameters and tokens are not logged, and a call with a bad token is logged without its method.
  There is no audit trail beyond that.

The 2026-10-02 security review checked these surfaces in source
([report](SiloUI-CODE-REVIEW-PASS-3-SECURITY-2026-10-02.md)); it was not a live exploit campaign.

## Linux hosts

Scope: `silod` built for Linux (x86_64, aarch64), running Linux computers over KVM with the same
patched MicroSandbox runtime, and managed from another device through Connections
([`silod` on a Linux host](SiloUI-ENGINE.md#silod-on-a-linux-host)). There is no app, no macOS
computer, no viewer and no clipboard on the host. The boundaries above apply unchanged except
where this section says otherwise.

| | Mac | Linux host |
| --- | --- | --- |
| Hypervisor | Hypervisor.framework under libkrun, in the signed `msb` | KVM under libkrun, in `msb` running as the engine's account. Silo adds no seccomp filter or namespace around it. |
| Which binary serves the socket | The bundle's code signature | Location and ownership: before it reads anything, `silod` refuses an engine directory, any directory above it, `silod`, any entry beside it, or anything under `resources/` that an account other than its own or root owns or can change, and links that leave the directory ([`install_trust.rs#L46`](../app/SiloUI/src-tauri/silod/src/install_trust.rs#L46), called from [`linux.rs#L38`](../app/SiloUI/src-tauri/silod/src/linux.rs#L38)). Before the first `msb` launch the agent checks the MicroSandbox manifest against the runtime inputs compiled into the build and hashes `msb` and `libkrunfw` against it ([`dependencies.rs#L933`](../app/SiloUI/src-tauri/engine/src/dependencies.rs#L923), [`linux.rs#L123`](../app/SiloUI/src-tauri/silod/src/linux.rs#L123)); `libkrunfw`'s hash is compiled in, `msb`'s comes from that manifest, which the ownership check covers. Nothing is signed: signed archives are roadmap milestone 11. |
| Credential values | Keychain items whose access list names the engine | One 0600 file per item under 0700 directories at `$XDG_DATA_HOME/<identifier>/credentials/<service>/<account>` ([`credential_file_store.rs#L105`](../app/SiloUI/src-tauri/engine/src/credential_file_store.rs#L105)). Every process of the engine's account and root can read them, and a backup of the home directory copies them. `SILO_CREDENTIAL_STORE=secret-service` is untested. |
| Client check | `getpeereid` | `SO_PEERCRED` ([`socket.rs#L76`](../app/SiloUI/src-tauri/silod/src/socket.rs#L76)); the same effective-UID comparison |
| Paths | `~/Library/...` | XDG data, cache and state directories ([`paths.rs#L36`](../app/SiloUI/src-tauri/silod/src/paths.rs#L36)); `~/.silo` and `~/.silo-dev` hold the socket on both |
| Management | The app, or Connections | Connections only (the bridge of boundary 7) |

**The VM.** Run the engine as a dedicated account in the `kvm` group, never as root: `silod`
refuses euid 0 and `nobody` (uid 65534) for the agent, the bridge and the guest helper
([`install_trust.rs#L34`](../app/SiloUI/src-tauri/silod/src/install_trust.rs#L34)). A libkrun or virtio escape then
gives the attacker that account, as on a Mac, and on a dedicated account that is the engine's
data and credentials, not a person's files. The engine starts `msb` in its own process group
([`runtime/mod.rs#L1780`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L1780),
[`guest/process.rs#L27`](../app/SiloUI/src-tauri/engine/src/guest/process.rs#L27)), so a terminal's Ctrl-C reaches
`silod` alone and its session end stops the computers in order; a second SIGINT or SIGTERM exits at
once ([`agent.rs#L233`](../app/SiloUI/src-tauri/silod/src/agent.rs#L233)).

**Egress.** The computers get the same network profile and MicroSandbox policy as on a Mac
(boundary 2), now in front of the host's network. It has not been evaluated on KVM: that eval comes
with the KVM CI job, and until then this document claims nothing stronger for a Linux host than the
code reading of boundary 2.

**Host folders a guest sees.** The same as boundary 6: the read-write runtime share at `/.msb`
(served from the runtime's per-computer directory,
[`guest_client/spool.rs#L123`](../app/SiloUI/src-tauri/engine/src/guest_client/spool.rs#L123) for the bridge folder in
it), the two empty read-only legacy folders on older computers, and `/workspace` as a disk image.
The credential directory is never a mount source or inside or above one; a unit test checks that
against the storage, share, guest-image and legacy paths
([`credential_file_store.rs#L809`](../app/SiloUI/src-tauri/engine/src/credential_file_store.rs#L809)).

**Secrets and GitHub.** The placeholder and token logic of boundaries 3 and 5 is unchanged; only
where the values rest differs. The file store opens every directory and file without following
links, through directory descriptors, checks every directory from the XDG data directory down,
and refuses a link, another account's file, or a file others can read
([`credential_file_store.rs#L133`](../app/SiloUI/src-tauri/engine/src/credential_file_store.rs#L133),
[`#L359`](../app/SiloUI/src-tauri/engine/src/credential_file_store.rs#L359)). A refusal is reported as
`credential_access_denied`, naming the file and never the value
([`credential_store.rs#L50`](../app/SiloUI/src-tauri/engine/src/credential_store.rs#L50)). The engine records the store
it used first and refuses a different `SILO_CREDENTIAL_STORE` later
([`linux.rs#L185`](../app/SiloUI/src-tauri/silod/src/linux.rs#L185)).

**Engine socket and in-guest client.** Boundaries 7 and 9 unchanged, with `SO_PEERCRED` in place of
`getpeereid`.

Not defended on a Linux host:

- **Processes of the engine's account.** They have the engine's full authority (boundary 7) and can
  read every credential file. Nothing asks first, as the Keychain can on a Mac.
- **Root**, which can read and change everything above.
- **A writable install directory.** The ownership check makes `silod` refuse one, but only at
  start: an account that can change the directory later, or a writable directory the check does
  not cover, can still replace what the engine runs next. The check reads mode bits only, so
  write access granted by a POSIX ACL is not seen. Nothing is signed.
- **Other local users reaching loopback listeners.** Published ports, SSH access to a computer and
  host-push listen on the host's loopback, which every account on the host can reach. Use a
  single-tenant host.
- **The Secret Service backend**, which has never been exercised.

## What Silo does not protect

These follow from the design today, not from bugs:

- **Data the agent can read can leave.** Repository contents, files in `/workspace`, and responses
  from services the agent can call can be sent to any public host, including through an allowed
  host (for example a gist or an issue on a repository it can write).
- **Credentials can be used, not just stolen.** A scoped secret or GitHub token works for the agent
  exactly as for the user, within its scope.
- **Guest integrity.** The agent is root in the guest. Anything inside a computer, including tools
  Silo installed there, can be changed by it. Roll back with a checkpoint taken before the
  compromise; a restore replaces the disk, not what the agent already sent or did with its
  credentials.
- **Resource abuse.** CPU, memory and disk are capped per computer
  ([`runtime/mod.rs#L6508`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L6282),
  [`engine.rs#L546`](../app/SiloUI/src-tauri/engine/src/macos_computers/engine.rs#L546)). Outbound bandwidth and the
  number of connections are not: `max_connections` is null in
  [`github-network-default.json`](../app/SiloUI/src-tauri/guest/github-network-default.json), no rate limiter is set,
  and the macOS gateway runs the single-tenant profile
  ([`network.rs#L209`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L209)). Host-side readers are bounded:
  guest writes to `/.msb` at 16 MiB, relayed command output at 1 MiB
  ([`runtime/mod.rs#L125`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L125)).
- **Personal access token mode.** The agent's GitHub requests carry the whole token, unscoped
  (boundary 5).
- **macOS computers before setup completes.** They run on Apple NAT and can reach the Mac and the
  LAN (boundary 2).
- **Untrusted imported archives.** A macOS import mounts the guest's disk on the Mac; import only
  archives you trust (boundary 6).

## Non-goals

- **Malware running as the same user on the Mac.** It can already read the user's files, connect to
  the engine socket (boundary 7), read any Keychain item the user has approved for it, and trigger
its own Keychain prompts. Silo does not
  defend against it.
- **Hypervisor and kernel 0-days.** A VM escape in libkrun, Hypervisor.framework,
  Virtualization.framework or macOS is out of scope; Silo tracks upstream fixes but adds no
  defence-in-depth layer today.
- **Reflected-secret exfiltration.** If an allowed host returns the request's headers (debug echo
  endpoints, some error pages), the guest sees the real value. Restrict secrets to hosts you trust.
- **Egress filtering.** The policy is default-deny with one allow rule for every public address.
  There is no per-computer hostname or destination allowlist, no mode that blocks public
  addresses, and no DNS query log yet; DNS queries go to the Mac's resolvers unfiltered. See
  [planned changes](#planned-changes-that-will-change-this-document).
- **Hostile apps that embed the engine.** An app that talks to `silod` is trusted with the same
  authority as Silo.app.

## Planned changes that will change this document

- Per-computer egress policy (block public addresses by default, or an allowlist, with a DNS log).
- Per-client socket authentication (macOS code signature, Linux `SO_PEERCRED`) and an audit log of
  client, method and computer.
- On a Linux host: signed engine archives (roadmap milestone 11), and the KVM isolation eval that
  comes with the KVM CI job.
- A security review of the read-write `/.msb` share (boundary 6).
- A public, re-runnable isolation eval in `docs/evals/` whose results back each "cannot" above.

Until the eval exists, the "cannot" column is supported by code reading, Silo's unit tests and
the manual checks recorded in [docs/SiloUI-SECRETS.md](SiloUI-SECRETS.md) and
[docs/SiloUI-NETWORK-PLAN.md](SiloUI-NETWORK-PLAN.md), not by an adversarial test suite. The `/.msb` eval result in boundary 6 is not yet recorded in the
repository.


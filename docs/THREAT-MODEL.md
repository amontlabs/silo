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
allowed by default, and the per-computer network policy (an allowlist or closed) narrows that but does not stop sending data to a host it allows.

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

#### Network policy per computer

Each computer has a mode, set by you (`silo network policy`, `network.policy.set`) and stored with
the computer ([`network_policy/`](../app/SiloUI/src-tauri/engine/src/network_policy/mod.rs)):

| Mode | Rules the runtime gets | What the guest can reach |
| --- | --- | --- |
| `open` (the default; every computer saved before the policy existed) | name lookups, Silo's own rules, every public address | what this boundary describes above |
| `allowlist` | name lookups, one rule per allowed host on TCP port 443 (the registry presets also name port 80 for Ubuntu's archives), Silo's own rules, and **no** public-address rule | only the listed hosts, plus GitHub's hosts while the computer has GitHub access and the hosts of each secret assigned to it |
| `closed` | Silo's own rules only | nothing outbound, and no name lookups. A container's blocked state is the same mode. |

Host patterns are lowercase DNS names of at least two labels, or `*.` and such a name (which also
covers the name itself); addresses, ports, bare top-level names, names that stay on the device and
a `*.` pattern over a public suffix (`*.co.uk`, `*.github.io`) are refused. The suffixes come from
a small fixed list kept next to the presets
([`public_suffixes.json`](../app/SiloUI/src-tauri/engine/src/network_policy/public_suffixes.json));
it holds the common multi-label suffixes and shared-hosting names, not the full public suffix
list, so a wildcard over a suffix that is not on it is accepted. Presets (npm, PyPI, crates.io,
Ubuntu, Homebrew, GitHub, Anthropic, OpenAI) expand to host lists from one data file in the engine.
Each preset has a version and a policy stores the version it was approved at: when an update of
Silo adds hosts to a preset, a computer keeps the list it was approved with until you accept the
update (`network.policy.get` lists it as a preset update; setting the policy again with the same
presets accepts it). The data file keeps the list of every earlier version of a preset for as long
as a saved policy may name it; a version Silo does not know allows nothing. A policy is read from
one file; when it cannot be read (damaged, or written by a newer Silo) a computer does not start,
is not created, and its gateway gets no way out, and the file is left alone. A computer with no
saved policy is judged and run by the rules its runtime holds (deny-all rules are closed), never
as open by default; an import saves an explicit closed policy. The policy follows the computer: a
checkpoint restore and a fork keep it, an export drops it, an import starts closed, and a computer from before the
policy loads as open. A computer's agent may read its own policy and those of computers it made,
may narrow either freely, and asks you to widen one (see [the policy page](SiloUI-POLICY.md)); a
computer it makes gets its own policy or a narrower one.

What the allowlist stops: a connection to a public address that is not on the list, by name or by
address (the runtime checks the name the guest asks for, so pointing an allowed name's address at
another name fails), a query sent straight to another resolver such as 1.1.1.1 or 8.8.8.8 (the
rule that lets the guest use any public resolver is absent), and UDP and other ports to listed
hosts.

What it does **not** stop, and why you should still treat the computer as hostile:

- **Sending data to an allowed host.** A gist, an issue, a package upload or an API call to a host
  on the list carries whatever the agent can read. The list limits where data can go, not what is
  sent. A GitHub grant adds GitHub's hosts, which include places the agent can publish to.
- **A name lookup reaches the internet, so DNS is a way to send data out.** In open and allowlist
  modes the gateway's resolver answers any name by asking that name's authoritative server on the
  internet. A guest can therefore put data in the name it looks up (a long label under a domain
  whose server it or an attacker runs) and it arrives, whatever the list says. This is a usable
  channel, not a narrow one. A closed computer has no lookups at all.
- **With `strict` off, a host that shares an address with an allowed host may be reachable.**
  Hostname rules for hosts that no secret covers work only while the runtime's strict mode is off
  (under strict, a TLS connection the runtime does not intercept is refused when only a hostname
  rule admitted it, so allowed hosts would fail). An allowlist computer therefore runs with strict
  off. The connection is still checked against the name in the TLS request, but a service that
  answers on an allowed host's address for another name, or a name that resolves to the same
  address, is not distinguished by address alone. With strict off, traffic that carries no TLS
  server name (raw TCP, or TLS without one) is admitted to an address the resolver's cache ties to
  an allowed name, so an agent that can make an allowed name resolve to an address it controls
  can open any TCP stream to it. Open and closed computers run with strict on.
- **A secret's hosts are allowed hosts too.** The hosts a secret may be sent to (wildcards
  included, unless over a public suffix) are added to the list of each computer it is assigned to,
  and the approval for an agent adding a secret says so. A GitHub grant likewise adds GitHub's
  hosts at the version of Silo that is running.
- **The macOS gateway and containers** apply the same rules through their gateway's start-up
  request and take them when the gateway starts, so a running computer keeps its rules until it
  restarts; the answer to a change says when it applies. A Linux virtual machine changes by being
  restored in place from a checkpoint taken just before its next start.

The runtime cannot change a created computer's rules, so a change is applied by Silo restoring the
stopped computer in place with the new rules (`restore --net-strict`, the
`microsandbox-restore-net-strict` patch). A Linux host builds its runtime in CI; the allowlist has
not been run on one.

macOS computers that have finished setup run behind a per-computer gateway child,
`silo-macos-gateway` ([source](../app/SiloUI/src-tauri/macos-gateway/README.md)), which uses
the same MicroSandbox network stack. The VM's only network device is a file-handle attachment to
that child ([`engine.rs`, `start_machine_once`](../app/SiloUI/src-tauri/engine/src/macos_computers/engine.rs)),
and the child applies the `Public` profile when no other policy is given
([`network.rs#L147`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L147)).

Limits of this boundary:

- **In the default mode, open, there is no hostname or destination filter.** Any public host, any
  port, TCP and UDP (except UDP to port 443). The other two modes are described under
  [Network policy per computer](#network-policy-per-computer).
- **macOS computers: only a disk no agent can have written runs on Apple NAT.** A first
  install and the setup of a copy of a template run on `VZNATNetworkDeviceAttachment` with no
  Silo policy until their setup completes
  ([`network_plan`, `store.rs`](../app/SiloUI/src-tauri/engine/src/macos_computers/store.rs);
  [`engine::start`](../app/SiloUI/src-tauri/engine/src/macos_computers/engine.rs)). In that
  state the guest can reach the Mac and the LAN. The disk then holds only Apple's image and
  Silo's scripts, so that is the expected workload. A computer's record carries
  `agentMayHaveUsed`, set when its setup first completes and never cleared, and set at birth
  for every fork and import. A flagged computer never gets Apple's NAT: a boot that finishes
  its setup goes behind the gateway after Silo has replaced the network bootstrap program, its
  launch daemon and its configuration on the stopped disk, and is refused, with a message that
  says what to do, when the disk has no such files, a link stands where one should be, the
  build has no signed program, or the saved network identity does not match. Recovery gets no
  network card. The choice is made where the machine is built, so nothing the guest does
  changes it. One case remains: a computer made before the gateway existed is converted once,
  at launch, over Apple's NAT, because its setup never completed and so its record is not
  flagged. The details are in
  [`SiloUI-MACOS-COMPUTERS.md`](SiloUI-MACOS-COMPUTERS.md#network).
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

Linux virtual-machine computers share one interception CA, generated by the runtime and kept as a
`0600` file under the runtime home; it is not per computer and not in the Keychain. Only its
certificate is written to the computer's `/.msb/tls/`. Linux container computers
([below](#container-computers)) are the exception: like a macOS computer, each has its own CA,
whose key is kept in the engine's credential store, which on a Linux host is the credential file
store (one `0600` file per item under `0700` folders).

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

- **A personal access token is not scoped by Silo.** A personal token is a secret of kind
  `github-token`; you can keep several and choose one per computer. For a computer that uses one,
  the proxy sends that token for every request to the GitHub hosts (`profile.version == 2`,
  [patch line 394](../app/SiloUI/patches/microsandbox-silo-network-0.7.6.patch#L394)), so the agent
  has whatever permissions the token has: the scope is the one you gave it on GitHub, and Silo adds
  no repository limit. The README says so. A computer only ever receives the token it was given,
  never another one in the list, and a token secret is never delivered as an environment
  placeholder. Host push for such a computer also uses the full token it chose, which is not
  narrowed or revoked (`personal_push_credential` in
  [`github.rs`](../app/SiloUI/src-tauri/engine/src/github.rs)). A token GitHub rejects
  disconnects, and the computers using it lose GitHub access instead of falling back to another
  credential.
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

**Sharing a computer as a file.** `silo export` and the app's Export write a computer's disks into
one `.silo-backup` file, and `silo import` reads one. What the file holds, and does not:

- **The guest image is not embedded** when the computer uses the image this Silo pins. The file
  names it; the importing Silo prepares its own pinned copy the way it does for a new computer and
  refuses a file naming another image or a different digest of the same one. A computer on any
  other image still embeds it, and files made before this change keep theirs.
- **The fork-time record travels** when the exported computer is a fork: the list of its files as
  they were at the fork, which paths and digests, not contents. Import stores it as the new
  computer's record, with no source and no origin, so the receiver names the target
  (`silo fold <imported> --into main`).
- **A Linux computer's known login files are removed from the exported computer, and traces
  may remain in the file.** An export of the current state of a Linux virtual machine is made from a
  hidden, short-lived copy of it (`silo-export-<8 hex>`, never listed): forked with no secrets, no
  GitHub access and a deny-all network, started, stripped of the files Silo recognises as logins (Claude
  Code `~/.claude/.credentials.json` and `~/.claude.json`, Codex, Gemini CLI, GitHub CLI, cloud and registry
  credentials, SSH private keys and key files in `/etc`, `/root`, `/home`, `/usr/local`, `/opt`, `/srv`),
  stopped, captured under the computer's name and dropped
  ([`export_scrub.rs`](../app/SiloUI/src-tauri/engine/src/export_scrub.rs)). The list is the one the scan and
  the diff's "Not kept" use. The result names the files removed; `--include-logins` (or the dialog's Include
  logins) keeps them. What the removal does and does not give:
  - **The files are not in the imported computer.** The check in
    [`t14_export_scrub_check.sh`](direction/checks/t14_export_scrub_check.sh) imports a default export,
    starts it, and finds none of the login files and neither marker among its files
    ([`T14.txt`](direction/evidence/T14.txt)).
  - **Their bytes are still in the file.** The copy is a fork, and a fork's disks are layers over the
    computer's own. The layers it shares with the computer (everything up to the fork's checkpoint) are
    read-only and are written into the file whole, deleted files' blocks included. Discarding the copy's free
    blocks (`fstrim`) reaches only its own top layer, and the same check finds a long random marker, written to
    every known login path and then deleted, in the layers of the default export. Anyone with the file and the
    tools to read a disk layer can recover them. Neither the app nor the CLI says the file is free of them;
    they say traces may remain. Removing them for real needs the layers flattened, which the runtime's
    snapshot export does not offer.
  - **It is a list, not a guarantee.** A secret in a file Silo does not recognise (an `.env`, a token in
    a shell history, a key under `/workspace`) stays in the file; the listing walks the folders `silo diff`
    walks. A key-material file (`*.pem`, `*.key`, ...) anywhere in those folders is removed too, which can
    include a certificate an installed program needs; the removed list names every path.
  - **The computer's own fork-time record travels** (paths and digests, not contents).
  - **It does not apply to a checkpoint, a macOS computer or a container computer.** A checkpoint is
    another disk than the live one; a macOS computer's login files and Keychain would need an offline edit
    of a copied disk, and a fork's rekey already clears the login keychain, but nothing here has verified
    that the removed bytes leave the disk image; a container has nothing to discard. These exports keep the
    logins in the file and ask first, as before: Silo lists the logins it recognises, on the disk and in the
    fork-time record, and asks; the CLI refuses without a terminal unless `--yes` is passed, and the app asks
    in a dialog. When Silo could not check (a stopped computer, whose disk is not listed, an export of a
    checkpoint, or a check that failed) it asks too: "Silo could not check <name> for logins (<reason>).
    Share anyway?", and `--json` reports the reason as `unchecked`. The list is a warning, not a guarantee.
    The disk is listed with the walk `silo diff` does (`/etc`, `/root`, `/home`, `/usr/local`, `/opt`, `/srv` and
    the crontabs on Linux; the home folder, `/usr/local`, `/opt/homebrew/etc` and the launch agents and
    daemons on macOS), so a login anywhere else, such as under `/workspace`, is not named either.
  - **The copy has the computer's own CPUs and memory** while it exists, and holds a second copy of its
    writable disks until it is dropped. A copy that the engine could not drop (it stopped mid-export) stays
    hidden and is dropped by the next export.
- **No memory is in the file**, for any kind of computer: an export carries disks only, as a
  fork does. A running computer is captured from its disks: the guest writes its cached file data
  out and the disks are copied, with a macOS computer paused for the copy the way a fork point
  takes it ([`macos_computers.rs`](../app/SiloUI/src-tauri/engine/src/macos_computers.rs),
  `create_archive`). A macOS checkpoint that saved memory is exported from its disks; the saved
  memory stays in the checkpoint's folder. A Linux VM checkpoint taken while the computer ran is
  refused: MicroSandbox writes such a checkpoint whole, with its memory, and has no way to leave
  it out, so [`backup.rs`](../app/SiloUI/src-tauri/engine/src/backup.rs) reads the descriptor of
  every capture before it is written and stops at one that holds memory, saying what to do
  instead (fork the checkpoint and export the fork, or export the computer itself). A container
  computer has no memory checkpoints. A file written before this rule can hold memory; an import
  never uses it, because an imported computer always starts from its disks.
- **Secrets, published ports, the network policy and host-folder mounts are not carried**; an
  imported computer starts stopped with a closed network policy.
- **An export is a plain file.** Keep it where you would keep the secrets in it. Importing one
  from someone else adds a computer whose disk you did not make; it starts stopped and is as
  untrusted as any new agent computer.
- **An imported computer is untrusted until it is kept.** `silo fold <imported> --into main`
  writes the sender's files into the computer that holds your secrets and network policy, and
  that includes shell startup files, agent settings and instructions, and a crontab: whatever
  the sender's agent changed runs in your main afterwards. Read `silo diff <imported>` before
  you keep it, and do not keep what you would not run.

**Sharing a computer that lives on another device.** `silo --device mini export main -o FILE` and
`silo --device mini import FILE` move the file over Connections. The managed key may only run the
bridge, so `scp` and `sftp` do not reach the owner; Files' `sftp` only reaches guests. The owner
therefore keeps a *transfer area*, `<engine directory>/transfer/` (directory 0700; every file in it
0600: received files are created that way and an export is made private when it finishes),
and serves exactly one file of it per `exports.file` stream.

- **No new data access.** A controller already holds the owner's authority (boundary 7): it can
  export any computer to a folder on the owner, run commands in it and read its files. The area adds
  no path a controller could not already use; it exists because the key cannot copy a file.
- **Names are the owner's.** A client never sends a path. The owner picks every name (`x-<token>`
  for an export it wrote, `i-<token>` for a file it received), the pattern is fixed, and a name
  carries its role: an export name can only be read, an import name only imported from, so received
  bytes cannot be read back and an export cannot be overwritten. Each export name is read once
  (a marker file is created exclusively); a second open is refused. Links are not followed.
- **Owner disk is bounded by what is declared.** A write names its size and sha256 first and is
  refused when the size is zero, past the cap (16 GiB; the engine's own export has no smaller
  limit), or the volume cannot hold it with 64 MiB to spare after the writes already promised.
  Writes are not serialized (the engine's stream budget of 192 is the only bound on how many are
  open); instead each one reserves its declared size in a counter the check subtracts from the
  free space, under one lock, and gives it back when the file is kept or the write ends. The
  counter counts a write's full size even after part of it is on disk, so it errs on the side of
  refusing. An export stops once the computers written are past the cap, before the file is assembled,
  and a finished one that turns out larger is removed. A caller cannot choose a name in the area: an export aimed at
  the area's folder by path is refused. The bytes go to a `.part` file, exactly the declared size is read, and the file is renamed only when
  the size and hash match; any other end deletes the `.part`. A stalled write ends after 120 s.
  The copy has the Files time budget (45 s plus the size at 64 KiB/s) and the controller's job can
  be cancelled.
- **Import content is untrusted.** A received file is imported exactly like a local file of unknown
  origin: the archive validation is the only boundary, and the imported computer is untrusted until
  it is kept (boundary 6).
- **Integrity is size and hash over SSH.** SSH protects the bytes in transit; the hash catches a
  truncated or changed file. It does not authenticate the sender: the controller already has the
  owner's authority.
- **Leftovers are swept.** The controller asks the owner to remove the file after a successful copy
  (`exports.discard`, for names of the area only); for an import it does so once the import is
  reported finished, and leaves the file when it could only lose track of the job. The engine
  empties the area when it starts, before the Connections bridge accepts its first stream, and a
  periodic pass removes entries older than a day, so a crash or a lost connection leaves a file for
  at most a day. On the controller the copy is written beside the chosen path as `<path>.part` and
  takes the path only when the hash matched, without replacing a file that is there. While it lasts, the file is readable by the engine's account and root, like the
  rest of the engine directory.
- **One hop, and no old owners.** The controller copies from the owner it names; an owner never
  forwards. An owner that does not advertise `exports.file` in its handshake is refused with a
  plain message before anything is exported.

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
`restrict,command="exec ~/.local/bin/silo-remote --remote-bridge --key SHA256:..."`, the key's own
fingerprint ([`remote.rs#L49`](../app/SiloUI/src-tauri/engine/src/remote.rs#L49),
[`channel.rs#L106`](../app/SiloUI/host/src/channel.rs#L106)): no pty, forwarding or agent, and
only that command whatever the client asked for (`SSH_ORIGINAL_COMMAND` is never read). A
controller's key setup writes the line without `--key`, which a host with an earlier Silo also
runs ([`#L59`](../app/SiloUI/src-tauri/engine/src/remote.rs#L59)); a host with this version adds it at the first handshake
([`#L1655`](../app/SiloUI/src-tauri/engine/src/remote.rs#L1655)). The link names `silod` inside the app bundle. Started through it,
`silod` takes the `--remote-bridge` flag ([`lib.rs#L65`](../app/SiloUI/src-tauri/silod/src/lib.rs#L65))
and accepts no other argument than `--key` and a fingerprint
([`remote.rs#L32`](../app/SiloUI/src-tauri/engine/src/remote.rs#L32)), replaces itself with the file in its bundle
([`lib.rs#L75`](../app/SiloUI/src-tauri/silod/src/lib.rs#L75)), verifies that bundle's signature
as the engine does ([`app.rs#L36`](../app/SiloUI/src-tauri/silod/src/app.rs#L36),
[`service.rs#L24`](../app/SiloUI/src-tauri/silod/src/service.rs#L24)), reads one framed request
from its input, replaces any `authenticatedKey` in it with its own `--key`
([`remote.rs#L2785`](../app/SiloUI/src-tauri/engine/src/remote.rs#L2785)) and relays it to `~/.silo/desktop-remote/control.sock` (`0600` in
a `0700` directory, no peer-UID check; [`remote.rs#L3161`](../app/SiloUI/src-tauri/engine/src/remote.rs#L3161)). The owner refuses the
request unless Connections are on, the protocol version matches and, except for the handshake,
the request names this device's id
([`remote.rs#L3404`](../app/SiloUI/src-tauri/engine/src/remote.rs#L3404)); the id is not a secret
(the handshake returns it), so the key is the whole check. The owner then serves the methods in
`METHODS` and `OWNER_METHODS`
([`remote.rs#L328`](../app/SiloUI/src-tauri/engine/src/remote.rs#L328),
[`#L398`](../app/SiloUI/src-tauri/engine/src/remote.rs#L398)), including delete, checkpoint
restore, guest exec, file reads and writes, exports, and the `guest.ssh`, `exports.file` and macOS
display byte streams. Turning Connections off refuses every request but leaves installed keys in
`authorized_keys`; removing a device revokes its key. `authorized_keys` is the only place a
controller key is revoked, and keys do not expire. Through the bridge, `connections.keys.revoke`
is allowed only for the key the forced command names
([`remote.rs#L1078`](../app/SiloUI/src-tauri/engine/src/remote.rs#L1078), called from [`#L3527`](../app/SiloUI/src-tauri/engine/src/remote.rs#L3527)), so one connected controller
cannot revoke another's; a request through a line without `--key` (an earlier Silo's) is refused
while that key still has a bridge line. The key list (`connections.keys.list`, `silo host
status`) covers every line whose forced command is Silo's bridge, in any form; a line for the same
key without it is a login of its own and is not Silo's to list. On a host, `silo host init
--authorize` installs a controller's key out of band, with the same authority as a paired
device's key, through `connections.keys.authorize`, which `engine.sock` serves and the bridge
does not (it is in neither list), so a connected controller cannot let another key in; a
same-user process could, as it could edit `authorized_keys` itself. `silo host init` runs no
privileged step and never turns lingering on. On the controller, `silo devices add --host-key`
writes the host's key to `~/.ssh/known_hosts`; the fingerprint the person typed is the trust
root, and a different key already listed for the name is never replaced. A device that holds that key, or a
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
| Editor and terminal | The user's editor, and a terminal for a computer on another device, connect over SSH with no agent forwarding (`ForwardAgent no`, `IdentityAgent none`), a per-computer `known_hosts` and `StrictHostKeyChecking yes`; guest paths are URL-encoded and control characters refused. A local terminal for a Linux computer runs `msb exec --tty` through MicroSandbox's guest agent, not SSH (for a container computer, `silod --container-runtime exec --tty`, which runs `crun exec` on the user's terminal; its SSH is served by the engine's own server, see [Container computers](#container-computers)); a local terminal for a macOS computer runs `/usr/bin/ssh -t` with the same no-forwarding, pinned-host-key settings ([`ssh_alias.rs#L126`](../app/SiloUI/src-tauri/engine/src/macos_computers/ssh_alias.rs#L126), [`#L258`](../app/SiloUI/src-tauri/engine/src/macos_computers/ssh_alias.rs#L258)). The editor's own remote trust model still applies. | [`editor.rs`](../app/SiloUI/src-tauri/engine/src/editor.rs), [`terminal.rs`](../app/SiloUI/src-tauri/engine/src/terminal.rs) |
| Guest agent | In every Linux computer, MicroSandbox's `agentd` talks to the `msb` process on the Mac over virtio-serial (CBOR) for readiness, exec and heartbeats. Silo parses `msb exec` output (repository discovery, directory listings). Silo configures no virtio-vsock routes. | [`runtime/mod.rs`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs), [MicroSandbox `protocol/lib.rs`](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/protocol/lib/lib.rs) |
| Published ports | Guest services appear on Mac loopback; opening one in a browser runs guest content at a per-computer `.localhost` origin; a page there can still send cross-site requests to other loopback services ([`network.rs#L87`](../app/SiloUI/src-tauri/engine/src/network.rs#L87)). | [`network.rs`](../app/SiloUI/src-tauri/engine/src/network.rs) |

### 9. The in-guest client: a scoped token for a computer's own agent

Design and install steps: [the in-guest client](SiloUI-IN-GUEST-CLIENT.md). A Linux computer can be
given a token (`silo guest-client install`) that lets an agent in it call a short list of engine
methods through the `/.msb/silo-bridge` folder. The engine serves it from
[`guest_bridge.rs`](../app/SiloUI/src-tauri/silod/src/guest_bridge.rs) and checks each call in
[`guest_client/scope.rs`](../app/SiloUI/src-tauri/engine/src/guest_client/scope.rs).

What the token can do: fork its own computer (secrets only those the settings list for that agent,
none by default; the number of forks is not capped, but a fork, start or checkpoint is refused when
the device is short of memory or disk, see [the policy](SiloUI-POLICY.md#limits)), start, stop and diff the
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
ability to make forks of it and run them (a disk, CPU and memory cost; there is no count cap, but
the engine refuses a start, fork or checkpoint while available memory or free disk is below the
safety floor, and `fork_ram_budget` applies only to forks the host CLI makes) and to put review requests in front of
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
  (and root) reads it and no other guest user does. (In a container the folders carry real owners: see [Container computers](#container-computers).) The override format is taken from MicroSandbox
  0.7.6 source and is not yet checked in a running guest.
- **Forks and jobs.** A fork is the token's only after its job finished and the fork exists below
  the computer (recorded by id); forks the person made are never reachable, and a failed fork
  never claims a computer. A token holds at most 16 unacknowledged jobs; the number of forks
  and running forks is not capped (the safety floor on memory and disk applies).
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
| Which binary serves the socket | The bundle's code signature | Location and ownership: before it reads anything, `silod` refuses an engine directory, any directory above it, `silod`, any entry beside it, or anything under `resources/` that an account other than its own or root owns or can change, and links that leave the directory. Write access for the owner's user-private group does not count (Debian's `StrictModes` rule: the owner's primary group, no member but the owner; Silo also requires that the owner's `/etc/passwd` line and the group's single `/etc/group` line agree with the lookups and that no other `/etc/passwd` account has that primary group). Root-owned paths and gid 0 get no exemption, and write access for everyone and sticky directories are always refused ([`safe_directories.rs#L332`](../app/SiloUI/src-tauri/engine/src/safe_directories.rs#L332), [`#L359`](../app/SiloUI/src-tauri/engine/src/safe_directories.rs#L359), [`install_trust.rs#L51`](../app/SiloUI/src-tauri/silod/src/install_trust.rs#L51), called from [`linux.rs#L38`](../app/SiloUI/src-tauri/silod/src/linux.rs#L38)). Before the first `msb` launch the agent checks the MicroSandbox manifest against the runtime inputs compiled into the build and hashes `msb` and `libkrunfw` against it ([`dependencies.rs#L933`](../app/SiloUI/src-tauri/engine/src/dependencies.rs#L933), [`linux.rs#L103`](../app/SiloUI/src-tauri/silod/src/linux.rs#L103)); `libkrunfw`'s hash is compiled in, `msb`'s comes from that manifest, which the ownership check covers. Nothing is signed: signed archives are roadmap milestone 11. Residual risk of the private-group exemption: the check cannot see memberships a network directory (LDAP, SSSD, NIS) adds to a local group, groups `pam_group` grants at login, a group password in `/etc/gshadow` that lets `newgrp` join, or setgid programs of the owner; any of those gives another account write access the check treats as the owner's alone. |
| Credential values | Keychain items whose access list names the engine | One 0600 file per item under 0700 directories at `$XDG_DATA_HOME/<identifier>/credentials/<service>/<account>` ([`credential_file_store.rs#L110`](../app/SiloUI/src-tauri/engine/src/credential_file_store.rs#L110)). Every process of the engine's account and root can read them, and a backup of the home directory copies them. `SILO_CREDENTIAL_STORE=secret-service` is untested. |
| Client check | `getpeereid` | `SO_PEERCRED` ([`socket.rs#L76`](../app/SiloUI/src-tauri/silod/src/socket.rs#L76)); the same effective-UID comparison |
| Paths | `~/Library/...` | XDG data, cache and state directories ([`paths.rs#L36`](../app/SiloUI/src-tauri/silod/src/paths.rs#L36)); `~/.silo` and `~/.silo-dev` hold the socket on both |
| Management | The app, or Connections | Connections (the bridge of boundary 7), and the host-local `silo host` commands on `engine.sock` |

**The VM.** Run the engine as a dedicated account in the `kvm` group, never as root: `silod`
refuses euid 0 and `nobody` (uid 65534) for the agent, the bridge and the guest helper
([`install_trust.rs#L39`](../app/SiloUI/src-tauri/silod/src/install_trust.rs#L39)). A libkrun or virtio escape then
gives the attacker that account, as on a Mac, and on a dedicated account that is the engine's
data and credentials, not a person's files. The engine starts `msb` in its own process group
([`runtime/mod.rs#L1780`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L1780),
[`guest/process.rs#L27`](../app/SiloUI/src-tauri/engine/src/guest/process.rs#L27)), so a terminal's Ctrl-C reaches
`silod` alone and its session end stops the computers in order; a second SIGINT or SIGTERM exits at
once ([`agent.rs#L233`](../app/SiloUI/src-tauri/silod/src/agent.rs#L233)).

**Egress.** The computers get the same network profile and MicroSandbox policy as on a Mac
(boundary 2), now in front of the host's network. It has not been evaluated on KVM. The KVM CI job
(`.github/workflows/linux-host.yml`) is green, but it exercises the computer lifecycle (create, start,
exec, checkpoint, fork, drop) and the two-account Connections run, not an isolation scenario, and the
[evals](evals/README.md) have only run on a Mac. Running them on a KVM host is roadmap milestone 9;
until a scenario has run there, this document claims nothing stronger for a Linux host than the code
reading of boundary 2.

**Host folders a guest sees.** The same as boundary 6: the read-write runtime share at `/.msb`
(served from the runtime's per-computer directory,
[`guest_client/spool.rs#L123`](../app/SiloUI/src-tauri/engine/src/guest_client/spool.rs#L123) for the bridge folder in
it), the two empty read-only legacy folders on older computers, and `/workspace` as a disk image.
The credential directory is never a mount source or inside or above one; a unit test checks that
against the storage, share, guest-image and legacy paths
([`credential_file_store.rs#L942`](../app/SiloUI/src-tauri/engine/src/credential_file_store.rs#L942)).

**Secrets and GitHub.** The placeholder and token logic of boundaries 3 and 5 is unchanged; only
where the values rest differs. The file store opens every directory and file without following
links, through directory descriptors, checks every directory from the XDG data directory down
(the same ownership rule as the engine directory, owner's private group included), creates missing
ones through descriptors with fixed modes (0755 up to the XDG data directory, 0700 below) and only
inside a directory nobody else can change
([`safe_directories.rs#L408`](../app/SiloUI/src-tauri/engine/src/safe_directories.rs#L408)), and refuses a link, another account's file, or a file others can read
([`credential_file_store.rs#L147`](../app/SiloUI/src-tauri/engine/src/credential_file_store.rs#L147),
[`#L386`](../app/SiloUI/src-tauri/engine/src/credential_file_store.rs#L386)). A refusal is reported as
`credential_access_denied`, naming the file and never the value
([`credential_store.rs#L50`](../app/SiloUI/src-tauri/engine/src/credential_store.rs#L50)). The engine records the store
it used first and refuses a different `SILO_CREDENTIAL_STORE` later
([`linux.rs#L192`](../app/SiloUI/src-tauri/silod/src/linux.rs#L192)).

**Engine socket and in-guest client.** Boundaries 7 and 9 unchanged, with `SO_PEERCRED` in place of
`getpeereid`.

**Connections on the host.** A controller reaches the host through the host's sshd, as the engine's
account, with the Ed25519 key its own engine made. `silo host init --authorize <key>` has the engine
(`connections.keys.authorize`, [`hosts.rs#L1002`](../app/SiloUI/cli/src/hosts.rs#L1002)) write Silo's
line to `~/.ssh/authorized_keys`:
`restrict,command="exec ~/.local/bin/silo-remote[-dev] --remote-bridge --key SHA256:<fingerprint>"`,
the key's own fingerprint, and this channel's comment
([`remote.rs#L49`](../app/SiloUI/src-tauri/engine/src/remote.rs#L49),
[`#L1120`](../app/SiloUI/src-tauri/engine/src/remote.rs#L1120)). A controller's own key setup writes
the same line without `--key`, and the host's first handshake adds it. Only an Ed25519 key is taken
([`hosts.rs#L651`](../app/SiloUI/cli/src/hosts.rs#L651),
[`remote.rs#L1126`](../app/SiloUI/src-tauri/engine/src/remote.rs#L1126)). `restrict` turns off
forwarding, the pty and the agent, so the key runs only the bridge, which relays one framed request to
the engine's `control.sock` ([`linux.rs#L91`](../app/SiloUI/src-tauri/silod/src/linux.rs#L91),
[`remote.rs#L2754`](../app/SiloUI/src-tauri/engine/src/remote.rs#L2754)). The bridge accepts no argument
(a line from an earlier Silo) or `--key` and a fingerprint, else prints its usage and exits
([`#L32`](../app/SiloUI/src-tauri/engine/src/remote.rs#L32)), and replaces any `authenticatedKey` the
controller put in the request with that fingerprint
([`#L2785`](../app/SiloUI/src-tauri/engine/src/remote.rs#L2785)), so the host's `authorized_keys`
alone says which key connected. The owner uses that for one thing: a bridged
`connections.keys.revoke` is allowed for the caller's own key only, and refused with
`permission_denied` for another key, or through a line without `--key` while the key still has a
bridge line ([`#L1078`](../app/SiloUI/src-tauri/engine/src/remote.rs#L1078), called from
[`#L3527`](../app/SiloUI/src-tauri/engine/src/remote.rs#L3527)). `connections.keys.authorize`,
`connections.keys.list` and `connections.keys.own` are in neither `METHODS`
([`#L328`](../app/SiloUI/src-tauri/engine/src/remote.rs#L328)) nor `OWNER_METHODS`
([`#L398`](../app/SiloUI/src-tauri/engine/src/remote.rs#L398)), so the bridge refuses them and only a
client of `engine.sock` (boundary 7) adds or lists a key
([`access.rs#L2403`](../app/SiloUI/src-tauri/silod/src/methods/access.rs#L2403); the test is
[`remote.rs#L5994`](../app/SiloUI/src-tauri/engine/src/remote.rs#L5994)). Lines Silo did not write are
left alone: `authorize` and `revoke` refuse a symlinked or otherwise managed `authorized_keys` and
name the line to change there ([`#L1068`](../app/SiloUI/src-tauri/engine/src/remote.rs#L1068),
[`#L1119`](../app/SiloUI/src-tauri/engine/src/remote.rs#L1119)), `revoke` removes only lines whose
forced command is the bridge ([`#L1040`](../app/SiloUI/src-tauri/engine/src/remote.rs#L1040)), and
`silo host status` lists every such line and marks one that is not exactly Silo's or has no `--key`
([`hosts.rs#L719`](../app/SiloUI/cli/src/hosts.rs#L719)); a line for the same key with another forced
command or none (a personal login) is neither listed nor touched. What a controller's key gives is
everything the bridge serves, `METHODS` and `OWNER_METHODS` together: computers, checkpoints, guest
files and commands, secrets, GitHub, published ports and guest SSH on the host. Whoever holds a
controller's Silo private key, or runs as the controller's account (boundary 7 there), has that
authority over the host until `silo host revoke` on the host; the key does not expire. A revoke
only rewrites `authorized_keys`, which sshd reads when a connection starts: a bridge already
running, such as an open guest SSH stream, keeps going until it ends
([`#L1043`](../app/SiloUI/src-tauri/engine/src/remote.rs#L1043)). To cut open connections too,
turn Connections off, which ends open streams
([`#L4305`](../app/SiloUI/src-tauri/engine/src/remote.rs#L4305)), or restart the engine's service
after the revoke. It is less than a login: the forced command runs no shell, and the engine's account, not root, answers. Silo
does not configure sshd. The host's sshd must accept that account's keys: `StrictModes` ignores an
`authorized_keys` under a home or `~/.ssh` that others can write, which `init` and `status` report
([`hosts.rs#L862`](../app/SiloUI/cli/src/hosts.rs#L862)), and an account sshd does not let in cannot
be a host.

The other direction: a controller trusts the host it adds. Secrets and a GitHub personal token
saved for computers on a host are sent to that host and stored there
([`#L352`](../app/SiloUI/src-tauri/engine/src/remote.rs#L352)), and what the host returns (files,
command output, screenshots) is the host's word. The controller's only check of the host is the
pinned SSH host key of boundary 7, so a compromised host gets what is saved for it and can answer
anything, but it reaches nothing on the controller beyond what boundary 7 describes.

**Logs.** The unit sets no `StandardOutput`, so under systemd `silod`'s stdout and stderr go to the
journal ([`service_linux.rs#L225`](../app/SiloUI/src-tauri/silod/src/service_linux.rs#L225)). silod
uses no log framework; the journal holds what it prints: why it did not become the owner and what it
does on SIGTERM and SIGINT ([`agent.rs#L58`](../app/SiloUI/src-tauri/silod/src/agent.rs#L58),
[`#L243`](../app/SiloUI/src-tauri/silod/src/agent.rs#L243)), an engine directory without the runtime
([`linux.rs#L143`](../app/SiloUI/src-tauri/silod/src/linux.rs#L143)), why Connections are unavailable
and accept errors ([`remote.rs#L549`](../app/SiloUI/src-tauri/engine/src/remote.rs#L549),
[`#L3136`](../app/SiloUI/src-tauri/engine/src/remote.rs#L3136)), the `silo` link error
([`#L538`](../app/SiloUI/src-tauri/engine/src/remote.rs#L538)), image cache repair and update
recovery ([`startup.rs#L395`](../app/SiloUI/src-tauri/silod/src/startup.rs#L395),
[`#L447`](../app/SiloUI/src-tauri/silod/src/startup.rs#L447)), and the names of computers a session
end left running ([`process.rs#L47`](../app/SiloUI/src-tauri/silod/src/process.rs#L47)). None of
those lines carries a credential or token value: the file store's refusal names the file
([`credential_store.rs#L50`](../app/SiloUI/src-tauri/engine/src/credential_store.rs#L50)), a guest
client's rejected call is logged without its token
([`frames.rs#L220`](../app/SiloUI/src-tauri/silod/src/guest_bridge/frames.rs#L220)), and `msb`'s
output is captured by the engine or discarded, never inherited
([`runtime/mod.rs#L1846`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L1846),
[`#L1863`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L1863)). Error texts do carry paths,
computer and device names and addresses. journald lets root and, on typical distributions, the
`systemd-journal` or `adm` group read every account's journal, so these lines are not private to the
engine's account: a host administrator reads them without the account's credentials. Computer logs
under the data directory ([`runtime_logs.rs#L661`](../app/SiloUI/src-tauri/engine/src/runtime_logs.rs#L661))
and the XDG state log directory ([`paths.rs#L54`](../app/SiloUI/src-tauri/silod/src/paths.rs#L54)) are
the account's and root's.

**Lingering.** The unit is wanted by the user manager's `default.target`
([`service_linux.rs#L244`](../app/SiloUI/src-tauri/silod/src/service_linux.rs#L244)). With lingering
on (`loginctl enable-linger`, run by an administrator; silod only reports it,
[`#L564`](../app/SiloUI/src-tauri/silod/src/service_linux.rs#L564)), that manager starts at boot and
outlives every logout, so the engine, its socket, its computers and the bridge for every authorized
key are up whenever the host is, with nobody logged in. Without it the engine runs only between the
account's first login and last logout, and the manager's end kills what the session end left
([engine guide](SiloUI-ENGINE.md#silod-on-a-linux-host)). A lingering host answers every authorized
controller key at all times, which is what a host is for: revoke keys on the host rather than
counting on a logout.

Not defended on a Linux host:

- **Processes of the engine's account.** They have the engine's full authority (boundary 7) and can
  read every credential file. Nothing asks first, as the Keychain can on a Mac.
- **Root**, which can read and change everything above.
- **A writable install directory.** The ownership check makes `silod` refuse one, but only at
  start: an account that can change the directory later, or a writable directory the check does
  not cover, can still replace what the engine runs next. The check reads mode bits only, so
  write access granted by a POSIX ACL is not seen. A default ACL on a directory also overrides the
  umask a tool relies on: on the KVM runner `ssh-keygen` wrote the Connections key 0644, so the
  engine now sets a new key to 0600 itself
  ([`editor.rs#L295`](../app/SiloUI/src-tauri/engine/src/editor.rs#L295)); files the engine writes
  with explicit modes (the credential store, the unit, `authorized_keys`) are not affected. Nothing is
  signed.
- **A controller's key.** It is as good as the controller's engine: whoever holds it has the host
  engine's authority (above) until the host revokes it. Silo cannot tell a stolen key from its owner.
- **Other local users reaching loopback listeners.** Published ports, SSH access to a computer and
  host-push listen on the host's loopback, which every account on the host can reach. Use a
  single-tenant host.
- **The Secret Service backend**, which has never been exercised.

### Container computers

A container computer (Linux hosts only, opt-in, never the default) is **weaker than a virtual
machine**. It shares the host's kernel, so a kernel bug reachable from inside is a bug in the host
account, and boundary 1 does not hold for it: the hardware boundary is replaced by the baseline
below, which narrows the kernel surface and does not remove it. It is meant for a dev-only machine
with nothing sensitive on it, and the app, the CLI and `silo list` label it as less isolated.

- **Who the computer is on the host.** crun runs rootless inside a user namespace made with
  `newuidmap` and the account's `/etc/subuid` range, laid out like rootless Podman's
  `--userns=keep-id` ([`idmap.rs`](../app/SiloUI/src-tauri/engine/src/containers/idmap.rs)): the
  working account (`silo`, id 1001) **is the engine's own user and group**, and root and every
  other id are sub-ids that own no file of the account's. A process that escapes the container as
  the working account therefore lands as the account that owns the engine, and says plainly what
  that means: the engine socket, the engine's files and every other computer on the host (virtual
  machines and containers alike) are within its reach. The mapping gives the account no capability
  it did not have as the engine's user, and removes none of the engine's. This is a change from a container whose root was the engine's
  user: now a process that escapes as the *agent* lands as the engine, which is the process an
  agent runs as; an escape as container root (which Silo's own commands use, with seven file
  capabilities) lands as a sub-id and needs a second step. The mapping is what makes the
  in-guest client work (below) and why SSH sessions and files the agent writes are readable to
  the engine; it is not what keeps the agent from the engine. The namespaces do: the container's
  pid namespace shows no engine, gateway or holder process (nothing to signal or trace, `/proc`
  masked), its mount namespace holds none of the engine's files or sockets except one folder, the
  computer's own share bound at `/.msb` (its runtime folder, which holds the in-guest client's
  bridge folder: the account writes there, the engine reads and answers, and every other user of
  the container may only search the folder), its network namespace holds only the TAP, its user namespace gives it no capability over anything outside it, and the
  baseline below applies to the agent's commands as to any. The engine's user owns the computer's
  upper directory and what the agent writes in it; the upper directory's root, the unpacked image,
  and the init shim's layer belong to the container's root (a sub-id), so the agent cannot replace
  `/silo/init` or rename system directories out from under Silo's root commands. A container
  made by an earlier build, whose files were owned the other way, is refused at start with a
  message to recreate it.
- **The baseline.** No capabilities in the container's first process or in a user's commands (Silo's
  own root commands, such as creating the working account, get seven file and ownership
  capabilities inside the namespace); `no_new_privs` (so `sudo` and setuid programs do not gain
  privileges); a seccomp deny-list over allow-all (io_uring, `keyctl`, `bpf`, `perf_event_open`,
  `userfaultfd`, `setns` outright, and `clone` and `unshare` with a new user namespace, `clone3`
  answered with ENOSYS); Landlock applied by Silo's init shim, which is also the container's first
  process and reaps zombies, and which wraps every command run in the computer; a writable layer
  over a read-only image; `/proc` and `/sys` masked or read-only; cgroup v2 limits (memory and
  tasks always; CPU only when the user manager delegates `cpu`, otherwise the computer is not
  CPU-limited) on the computer's transient user scope. No host folder, no Docker or Podman socket and not the
  engine socket (boundary 7) are mounted; the engine's files are not in the container's mount
  namespace. The only host files the container sees are its own `resolv.conf`, `hosts` and the
  computer's gateway CA certificate (all bound read-only). The system trust bundle is the guest's
  own writable file: Silo runs `update-ca-certificates` as a root command at each start, so the
  guest's package manager can update it and a guest can add a CA of its own.
- **Network.** The container's network namespace holds one TAP device and a loopback, and nothing
  else; a gateway process (the same network stack as macOS computers, boundary 2) is the only way
  out, with the Public policy and the gateway as resolver. A computer made from an import starts
  with the deny-all policy instead and keeps it (the export section below).
  A command run in the computer enters
  that namespace too.
- **Secrets and GitHub access (boundary 3).** The same mechanism and rules as for a virtual
  machine: the guest's environment holds `$MSB_<name>`, and the gateway substitutes the value only
  in request headers, only toward the hosts the secret allows, after verifying the upstream's TLS
  certificate (TLS is intercepted only toward those hosts). The shared code validates the material
  and removes a secret from the running computer at once; a failed or unconfirmed update stops
  the gateway, so the computer loses its network instead of keeping a secret it was told to drop.
  The values live in the same place as for a Linux virtual machine: in the memory of a process of
  the engine's own account (there the runtime's process, here the gateway), and the engine's
  credential store is that account's too. What differs is the boundary between the guest and that
  account: a virtual machine's is a hypervisor; a container's is a user namespace, seccomp and
  Landlock on a shared kernel. A process that escapes the container lands as the engine's account
  and can read the gateway's memory, the engine's credential store and the GitHub tokens held for
  the computer; the account note above applies to every secret given to a container computer.
  Give one only secrets you would give the account. The usage events (names, hosts, methods; never values) are written under
  `sandboxes/<name>/logs/`, which the container does not see.
- **Differences from a Linux virtual machine's network.** Three, besides the boundary above.
  There are no passthrough hosts: a Linux virtual machine lets a placeholder sent to a host the
  secret does not allow go through unchanged (`passthrough=*`), where a container's gateway applies
  the violation action, as a macOS computer's does. The interception CA is per computer (see
  [boundary 2](#2-network-egress-public-addresses-only), where a Linux virtual machine's is shared), so the CA of
  one container computer does not vouch for another's. And the tools that trust the CA are the
  ones that read the system store or an environment variable Silo sets: OpenSSL, curl, wget, Go and
  Python's `ssl` through `/etc/ssl/certs`; Python `requests` and curl through
  `REQUESTS_CA_BUNDLE` and `CURL_CA_BUNDLE`; anything that reads `SSL_CERT_FILE`; Node through
  `NODE_EXTRA_CA_CERTS`; and Java's trust store when `ca-certificates-java` is installed, through
  the same `update-ca-certificates` run. Silo sets the variables for every command it runs in
  the computer, as a virtual machine's guest agent does. A program that ships its own bundle (a
  Python application using `certifi` without the variable, a browser with its own store) does not
  trust the CA in either kind of computer, and cannot reach a host a secret is allowed for through
  the gateway.
- **Published ports (boundary 8).** A guest TCP port is published on the host's IPv4 loopback by
  the gateway, as for a virtual machine: any process of any user on the host can connect to it,
  and the URL proxy serves it under `<port>.<computer>.silo.localhost`. The engine passes each
  request to the gateway over a per-computer control socket in a private directory (socket
  `0600`); a host port below 1024 is refused and at most 32 ports are published.
- **SSH, the editor and terminals (boundaries 7 and 8).** A container has no `sshd`, as a
  virtual machine has none: MicroSandbox's `msb ssh serve` is a host-side russh server that maps a
  shell, a command, a terminal and `sftp` onto its guest agent, and a container is answered by the
  same command line with a server of ours on the same library
  ([`ssh.rs`](../app/SiloUI/src-tauri/engine/src/containers/ssh.rs), russh 0.65, Apache-2.0, with
  its ring backend and no RSA keys). The engine's callers start it where they start `msb`
  (`silod --container-runtime ssh serve ...`): an editor's `ProxyCommand` over standard input and
  output, and the SSH access listener on the host's loopback (or the one chosen address) as the
  engine's child. It authenticates public keys only, from the authorized-keys file its caller names
  (read at every attempt, so removing a key ends access at once; a line with options is not
  accepted), for the working account only, and presents the computer's pinned host key; it runs
  at most 32 connections and 16 channels each. A session is `crun exec` as the working account
  through the engine's own exec path: the baseline above applies to it (no capabilities,
  `no_new_privs`, seccomp, Landlock), and a closed channel or connection ends the command's whole
  process group. The `sftp` subsystem runs `/usr/lib/openssh/sftp-server` (the guest image's
  `openssh-sftp-server`) as the working account, so file access over SFTP is exactly the account's
  own. A terminal comes from a pseudo-terminal `crun` makes inside the container; the SSH server
  (or the local terminal command) receives its controlling side over a socket in a folder only
  the account can enter (mode `0700`, whatever the umask of the process that serves the session:
  whoever connects to that socket first gives the session its terminal). Client environment
  variables are refused, as with an `sshd` that accepts none, and there is no agent, X11, remote
  (`-R`) or socket forwarding. A local forward (`direct-tcpip`: `ssh -L`, `ssh -D`, the editor's
  port forwarding, the desktop viewer) is answered only toward the container's own loopback, by a
  connection made inside the container's network namespace (`silo-ctr connect`); any other
  destination is refused, so a forward never reaches the host's network. The local terminal runs
  the same command attached to the user's terminal. To end or signal a command (a closed channel,
  a timeout, an SSH `signal` request) the engine signals the process group named in the command's
  pid file, and only while that pid is a process of the computer's own scope: the file outlives
  the command, and a number the host has since given to another process is never signalled. The
  loopback listener is reachable by every user on the host (see the loopback note under Other
  local users); the authorized key is the whole check, as for a virtual machine's. SSH sessions do
  not survive the engine stopping the container or the listener; a container that an engine
  restart leaves running is taken back by the next engine (see Checkpoints and forks below).
- **The in-guest client (boundary 9) in a container.** The per-computer share is the computer's
  runtime folder, bound at `/.msb` (read-write, no exec, no devices) and sealed by mode instead of
  by stat override: its parent `sandboxes/<name>` is `0700`, the runtime folder `0711`, and
  `silo-bridge`, `req/` and `resp/` keep the modes of a virtual machine's (`0755`, `1733`, `0711`).
  Because the working account is the engine's own user, the agent's `0600` request files are
  readable by the engine and the engine's `0600` answers by the agent; a file owned by any other
  id (another guest user, root) is not the account's, gets no owner, and its answer (if the
  engine could read the request at all) is the agent's alone. The agent owns the folders it
  writes in, so it can loosen their modes (a virtual machine's folders are root's to it): the
  worker restores the runtime folder's mode every pass, `req/` and `resp/` stay below it, and
  what the agent gains by loosening them is access for other guest users to its own requests,
  which it can already give them. The token, the allow-list and the folder parsing (links, sizes,
  counts) are the same code and the same limits.
- **No memory checkpoints.** The capability list in `containers/capability.rs` names one thing a
  container computer lacks, "memory snapshots": a container's checkpoints, and so its exports, save the disks only, whether the
  computer runs or not. No memory is ever carried into an export.
- **Export and import** ([`archive.rs`](../app/SiloUI/src-tauri/engine/src/containers/archive.rs),
  `silo-ctr pack` and `unpack` in [`pack.rs`](../app/SiloUI/container-shim/src/pack.rs);
  [Sharing a computer as a file](#6-the-workspace-disk-and-host-folders) applies unchanged: what the
  file holds, the login warning, nothing filtered, an imported computer untrusted until kept).
  A container's file is format 6, which a Silo without it refuses as newer, so an older device never
  tries to load it as a MicroSandbox snapshot. What differs from a virtual machine's:
  - *What is written.* The export is a checkpoint of the computer's disks (a running one is frozen
    for the copy, up to 30 s, as for any checkpoint) and each layer of its chain is written as it is,
    owners included, by `silo-ctr pack` in the container's own user namespace, so the engine's
    account never reads a layer outside it. A socket file is left out of the file (tar has no type
    for one); a
    device node other than a whiteout stops the export. The guest image is never in the file: the file names the config digest
    it needs and the importing Silo refuses any other than the one it pins or one it has kept
    unpacked (see *The guest image* below), which also bounds what
    the layers can sit on. Secrets, GitHub access, environment and layer ids are dropped
    from the record in the file, as they are for a VM, and the settings in the manifest carry the
    same deny-all network section a VM's do. An imported container starts with its network blocked
    (deny-all) and keeps it, as an imported VM does: the gateway starts it with no way out to
    any address, and no secret assignment (GitHub access among them) changes that. Ports the user
    publishes still answer on the host's loopback (the gateway's ingress is left as it is).
  - *What is read.* The file is an untrusted input. The payload's hash is checked as for a VM; the
    header is read with unknown fields refused, and its name, resources, labels and image are checked
    field by field and against the settings the manifest was validated for, so the file cannot make a
    larger computer than the manifest promised. A header carries no layer ids, secrets or paths.
    Each layer is a standard tar (PAX) stream that `silo-ctr unpack` reads as the container's root in
    a user namespace laid out like the container's, with the new empty layer as its root: the safety
    is the changed root, not the format. A path with `..`, an absolute path, an empty name or a hard
    link outside the layer is refused, and a symbolic link in the file can only name the layer's own
    files, so nothing outside the layer can be written. The overlay's own attributes other than
    `user.overlay.opaque` (`redirect`, `origin`, `metacopy`, ...) are refused, since they decide which
    files of the layers below show through. Memory is bounded: a folder's owner and attributes are
    applied as it is made, only the chain of open folders waits for its mode and time, and the
    process runs under a 1 GiB address-space limit, so a tar header that claims more ends in a failed
    allocation. The disk is measured, not estimated: before every chunk of a file, every 256
    entries, and whenever the names, link targets and attributes written since the last look add
    up to 16 MiB, `unpack` reads the free bytes and free inodes of the layer's file system and stops under
    the host disk margin (5 GiB by default; the inode margin is that over 16 KiB per inode), which
    counts block size, inodes, attributes and link targets as the file system does. Between two looks
    it can write at most one 16 MiB chunk of a file, or 16 MiB of names, link targets and attributes
    plus the file system's own cost of 256 entries. There is no limit on
    the number of entries: the inode margin is the limit. Sockets are not exported (tar has no
    type for them). The files it makes are what the container's own root could have written:
    setuid bits, file capabilities and device whiteouts are inert inside the container under the
    baseline (no new privileges, no capability, seccomp). On the host the account's own id is the
    container's agent user (`--keep-id`), so a file of that user with the setuid bit in an imported
    layer is a setuid file of the engine account on the host; the layer store (`containers/layers`)
    and the folder holding it are set to `0700` each time a layer is made, so no other account can
    reach or run it (a test checks the modes; this was set explicitly rather than found already
    so, because the folders' modes came from the umask and any default ACL).
  - *A device that cannot run containers* refuses an inspection or an import of such a file before
    anything is copied: on a Mac, "Silo cannot import a container computer on this device. Container
    computers run only on a Linux host."; on Linux the unmet requirements and their fixes, as
    `silo host init` gives them.
  - *What it does not cover.* An imported container is as untrusted as an imported VM: it runs
    whatever its sender's agent left, and keeping it into your main writes the sender's files there
    (read `silo diff` first). A malicious file can fill the free space down to the margin, as a
    container that writes without end can (see The disk below). The unpack stops within one chunk
    and 256 entries of the margin, not exactly at it, and another process writing at the same time
    can still take the disk below it (the host disk guard handles that, as for any container).
  - *The guest image.* The file does not carry it. An export accepts any image digest unpacked on
    the exporting device, and an import accepts the pinned one or one kept unpacked; otherwise it
    says the computer was made on an older guest image that this device does not have. An image
    is kept unpacked while a computer's record or a checkpoint names it, so an image an imported
    computer needs is kept before its first start too, when only its imported checkpoint names it.
    The pinned image is not unpacked by an import (that needs the image download): until its first
    start, a computer imported on it needs a Silo that still pins that image.
- **The desktop and Chrome.** A container computer has the desktop and computer use of a virtual
  machine: the image's own desktop service (Xvfb, PulseAudio, Xfce, Selkies) and Cua Driver, started
  by a root command after the container starts (the image's boot hook; the container has no init
  system and its PID 1 has no capability), with nothing installed by the desktop setup (it refuses
  to install packages in a container). The viewer reaches the streamer on the container's loopback
  through the computer's own SSH server, as for a virtual machine; no port is opened on the host.
  **Chrome runs without its sandbox.** Chrome's sandbox needs a nested user namespace, which the
  baseline's seccomp profile denies, and there is **no exception for Chrome**: allowing nested user
  namespaces for one program cannot be done safely, since they are a frequent kernel-bug entry
  point and the profile applies to the whole container. So the computer-use setup writes launchers
  for `google-chrome`, `google-chrome-stable`, `chromium` and `chromium-browser` in `/usr/local/bin`
  (and the desktop entries `google-chrome.desktop` and `chromium.desktop`) that add `--no-sandbox`. The
  consequence, plainly: **a web page that compromises the browser reaches everything in this
  computer** that the working account can reach, because the renderer is no longer isolated from the
  rest of the browser or the account: its files, its network as the gateway allows, and the authority
  the computer holds. **It can act with this computer's GitHub access and use its secrets** (the
  gateway swaps a secret's placeholder for the real value on the hosts that secret is allowed for), and
  **it can use the computer's in-guest `silo` client**, with what that client may do from this computer. It stays inside the container's baseline (no capability, seccomp, Landlock,
  only the TAP and the gateway), and the app, the CLI and `silo list` label the kind; the computer's
  page repeats the warning when it has a desktop. The launchers do not make a single browser safer:
  each runs the first installed of Google Chrome and the two Chromium paths, whatever name was called,
  and a launcher stays when the desktop is turned off. They cover those names only: **any
  Chromium-based program that an agent starts with `--no-sandbox`** (Playwright's and Puppeteer's
  browsers, an Electron application: their sandbox needs the same user namespace, so their callers
  pass the flag) **has the same exposure**, and one started without the flag fails to start. A virtual machine keeps Chrome's
  sandbox. With the desktop off no launcher is written until it is turned on (a program that starts
  Chrome by hand passes the flag itself). The approvals, the audit of computer-use calls and the
  one-driver-session rule are those of a virtual machine and run in the same engine code. Each command the engine runs in the
  container is in its own Landlock domain, which cannot read a process of another command (`/proc/<pid>/environ`, `exe`), so
  the desktop session of either kind leaves its display, bus and runtime variables in
  `/run/silo-desktop/user/session.env`, a file only the working account owns and root reads after checking
  that: it holds no secret, and the desktop stopping removes it. A virtual machine whose desktop service predates
  the file (Silo 0.13 or earlier; a computer keeps its service until its desktop is updated) has none, and the
  launcher and `silo-computer-use` then read the working account's `xfce4-session` process, as they did before.
  The viewer's credentials stay in
  `/var/lib/silo-desktop`, root's, and the streamer gets its password in its environment
  (`SELKIES_BASIC_AUTH_PASSWORD`), not in its arguments, which every process in the computer can read; its
  environment is readable by root and the working account's own processes, as in a virtual machine.
  `/run` is a tmpfs, as in a virtual machine, so the display's authority cookie and the session file are
  not part of the computer's files and do not travel into a checkpoint or a fork. The commands the engine
  runs itself as root (the boot command of the desktop, the gateway certificate update) take Silo's fixed
  environment only, never the computer's own variables (`PATH`, `LD_PRELOAD`, `HOME`), which would
  otherwise steer a program the account can replace.
- **Checkpoints and forks.** A computer's file system is an overlay: its writable layer over a
  chain of sealed layers, the shim layer and the image. Every layer is a directory of one shared
  store (`containers/layers/<id>`), a checkpoint is a list of layer ids with a copy of the
  computer's record, and a fork is a new record over the checkpoint's list with a writable layer
  of its own. Nothing a fork writes reaches the parent or another fork. The fork keeps its own
  reference to the layers, so deleting the parent or the checkpoint keeps them; a layer no record
  and no checkpoint names is removed, and nothing is removed when a record, a checkpoint or a
  folder that holds them cannot be read. A stopped computer's checkpoint seals its writable layer
  (no file moves). A running container cannot swap its writable layer, so its checkpoint is a
  copy of it, made by the container's own user namespace while the container's control group is
  frozen (a reflink copy where the file system has them; otherwise the time grows with the size
  of the layer). The processes are frozen for the copy itself and no longer than 30 seconds: a
  copy that takes longer is killed, the computer is resumed and the checkpoint fails with a plain
  message (a stopped computer's checkpoint takes no time). The copy is made before the layer
  store's lock is taken, which is held only to register the checkpoint, so a long copy never
  blocks another computer's start, restore or fork; the new layer is marked as being made, so
  collection leaves it alone until the checkpoint names it (its age would not protect it: a copy
  keeps its source's modification time). The freeze is separate
  from a user's pause: a checkpoint of a paused computer leaves it paused, `inspect` reports it
  as Paused, and an engine that takes the computer back after a restart does not thaw it (the
  pause is a file in the computer's state folder; a stop or a start removes it). A long chain has its oldest layers folded into one when the computer starts
  (`silo-ctr merge`, which keeps whiteouts and opaque directories); the folded layer is marked as being made in the same way. The layer files are owned by
  the container's sub-ids, so the engine reads, copies and removes them only through a user
  namespace laid out like the container's. Removing a checkpoint, a fork or a computer drops its
  references; the layers nothing names any more are removed by the next collection pass (at the
  engine's start, after a checkpoint is removed and after a computer is removed), not at once, and only once they are more
  than ten minutes old. A fork is quiet like a virtual machine's, but **later than a virtual
  machine's**: the quiet step (scheduled jobs parked, identity renewed) of a virtual machine runs
  first in its boot, before anything else starts. A container has no such hook, so the engine runs
  the step right after the fork has started, before the fork is reported started. For that
  moment the fork's processes (its image's services, scheduled jobs and anything the parent left
  to start with the computer) are already running with the parent's identity and its jobs, and
  the "starts nothing else first" promise of the virtual machine does not hold. Doing it before the
  start would mean editing the fork's writable layer from the host in the user namespace, which
  was left out. A container's host name and `/etc/hosts` are set by the runtime and are left alone.
- **An engine restart.** The holder, the container and its network device (which the namespace
  keeps) outlive the engine, as a virtual machine does. The next engine hands the device to a
  new gateway (`silo-ctr tap`), serves the published ports again and takes the container back; the
  GitHub profile is applied by the engine's next refresh. A container that cannot be taken back
  is stopped. `silo-ctr enter` and `silo-ctr tap` join the namespaces of the process in the computer's
  `outer.pid`; they do not check that it is the holder. The engine does, before it runs either
  (`is_holder`: a `silo-ctr` process in the computer's own systemd scope), and the folder is the
  engine account's own and private, so the check is the engine's and a hand-run `silo-ctr` with a
  folder someone else controls joins whatever that folder names, which the account could do
  anyway.
- **What Landlock covers.** The ruleset grants the container's root read and execute everywhere and
  write access to the directories a system normally writes (`/tmp`, `/var`, `/run`, `/home`,
  `/root`, `/workspace`, `/dev`, `/usr`, `/etc`, `/opt`, `/srv`, `/mnt`, and the share `/.msb`). It therefore mainly protects
  `/silo/init`, `/boot` and the root directory itself; the system directories stay writable to the
  container's root, which is the point of a computer you install software in. It is a second layer
  and **advisory where it is absent**: on a kernel without Landlock the holder logs one warning
  per start (shown by the engine) and the container runs without it. On a kernel with Landlock
  ABI 1 to 3 (older than ABI 4) the filesystem ruleset still applies and the holder warns that
  the kernel is older than the baseline expects; the namespaces, the missing capabilities,
  seccomp and the read-only masks apply in every case.
- **Account files.** The engine reads the container's `/etc/passwd` and `/etc/group` on the host to
  find a user's ids. Those files live in the writable layer, which the container controls, so they
  are opened without following links, must be regular files and are capped at 1 MiB; overlay
  whiteouts are honoured as inside the container.
- **The disk.** A container computer shares the host's disk: its files are a folder on the
  file system that holds the engine's home, and **its size (root disk and workspace disk
  together) is reported, not a bound**. The kernel offers no quota a rootless host can set on a
  folder (a project quota or a loop-mounted file system needs administrator rights, and a user
  namespace cannot mount a block file system). Making the container's root read-only past a limit
  was built and removed: it fails while a process has a file open for writing, has no way back
  after the engine restarts, is undone by a checkpoint and does not see a directory the engine
  cannot read. So a container that writes without end can fill the host's disk, as any program of
  the account can. What Silo does instead protects the host: below a margin of free space on the
  engine's file system (5 GiB by default; `SILO_CONTAINER_DISK_MARGIN_MIB` sets it, in MiB, in the
  environment of `silod`, because a Linux host has no settings file) the engine refuses to start a
  container computer, make a checkpoint of one or fork one, with a plain message, and stops (the
  ordinary graceful stop) the running container computers whose writable layer is larger than
  their stated size, the largest first, leaving the reason in the computer's status. A computer
  within its stated size is never stopped, and a stop frees nothing by itself: the files stay
  until their owner removes some. The guard reads the free space every 30 seconds and holds
  nothing about a computer between looks. Only while the host is below the margin does it measure
  layers (every minute), inside the user namespace so unreadable directories count (`du` there);
  **a layer that cannot be measured (the measurement timed out, or `du` itself failed) counts as over
  its size** and is stopped first, as soon as its own measurement ends, so a container cannot avoid the
  guard by making itself slow to measure. A failure of the tools around `du` (the user namespace cannot
  be set up, say) says nothing about the layer: it is logged and the computer is left alone, so a
  transient host error does not stop every container. Layers are
  measured in parallel (at most four at a time, each bounded to five minutes), so one slow
  container does not hold up the others; with more than four slow ones a look takes several bounds.
  What the guard does not give: its stop can land during a checkpoint's copy (it then ends the
  frozen control group after 15 seconds, and the copy, which runs outside the container,
  still completes and saves); it can act on a measurement taken minutes earlier, since a look
  measures first and stops afterwards; the sizes of several computers can add up to more than the
  disk, so each within its size and together enough to fill it; and a checkpoint freezes the
  processes for up to 10 seconds of waiting plus 30 seconds of copying. A program can write the
  whole margin between two looks, and the margin itself is the only protection against that. Sealed layers belong to
  checkpoints and forks and are not counted against a computer; the per-computer share
  (`/.msb`, the in-guest client's bridge folder, in the engine's `sandboxes` folder) is outside
  every size, the computer's and the guard's.
- **Over Connections** nothing is container-specific beyond what the earlier items say. A container
  is listed, run in and has its desktop switched through the same routed methods as a VM, answered
  by the owner's engine; its terminal and SSH are served by the owner's own `ssh serve` and
  `exec --tty` command, which `runtime::command_for` picks for a container as for a VM. `silo --device
  export` and `import` move the same format 6 file through the owner's transfer area (boundary 6,
  [Sharing a computer that lives on another device](#6-the-workspace-disk-and-host-folders)):
  the owner runs `pack` and `unpack` as its own account, the controller never reads a layer, and
  the controller needs no container support of its own to hold the file (it needs it only to import
  it). An owner that cannot run containers, or whose Silo predates format 6, refuses the file with
  its own message. A controller already holds the owner's authority (boundary 7), so it can export
  any container there, files and logins included.
- **Known gaps.** `/sys` is a fresh sysfs mounted read-only inside the container's own user and
  network namespaces, so it shows the container's own devices (the TAP and loopback) and the
  host's hardware, not the host's network interfaces. Chrome runs with `--no-sandbox` (see The
  desktop and Chrome above), so a web page that compromises the browser reaches
  everything in the computer. Ubuntu's `kernel.apparmor_restrict_unprivileged_userns` has to be 0
  on the host, which relaxes a host-wide guard; Silo checks and names it and never sets it. A
  container's gateway is the engine's child, so a restart replaces it (see An engine restart
  above); reading a computer's state never changes the computer. The evals of the isolation claims above have not been run against a
  container computer.

## CI runner (`silo-runner`)

Scope: the Go helper `silo-runner` ([CI runner](SiloUI-CI-RUNNER.md)) on a Mac or on a
[Linux host](#linux-hosts), giving every job of a private repository a fresh computer and deleting it
afterwards. The boundaries above apply to the computer; this section covers what the runner adds. The
last bullets cover a runner on a Linux host.

- **What a job can reach.** The job runs as the computer's working account, which is root-equivalent
  inside the computer: it has passwordless `sudo` on Linux
  ([`working-account.py#L276`](../app/SiloUI/src-tauri/guest/working-account.py#L276)) and on a Mac
  ([`offline_setup.rs#L1094`](../app/SiloUI/src-tauri/engine/src/macos_computers/offline_setup.rs#L1094)).
  A job can change anything in its computer, including the runner program and the recipe's tools. The
  wall is the virtual machine (boundary 1), not the account. The computer has its own network profile
  (boundary 2). Everything GitHub sends
  to a job is in the computer in the clear: `GITHUB_TOKEN`, repository and organization secrets the
  workflow uses, the repository contents and OIDC tokens it asks for. Silo's placeholder proxy
  (boundary 3) cannot protect those, because GitHub delivers them to the runner program itself; only
  secrets that Silo manages are placeholders. A workflow author who can push a workflow can read every
  secret that workflow can name, which is why the runner serves private repositories only.
- **Kept failed computers may hold credentials.** A failed job's computer is stopped, not deleted, so
  its disk keeps the repository, build output, the runner's work folder and any secret the job wrote
  to a file or printed into a log in the computer. It is deleted after `keepFailed.days` or beyond
  `keepFailed.count`, whichever comes first, but only while `silo-runner` is running: a stopped helper
  cleans nothing, so kept computers stay until it runs again. `keepFailed.count: 0` keeps nothing. Retention only deletes a computer that is already kept, never one whose runner is still being deregistered.
  Whoever can run `silo` on the Mac can start and read it
  (boundary 7). The runner never takes a checkpoint of a computer after a job ran in it, and a
  one-time runner registration is already spent by then.
- **The helper's GitHub credential.** `silo-runner` holds a GitHub App key or a personal access
  token with the right to register runners for the repository (or organization). That credential is
  on the Mac, not in any computer, and the one-time JIT configuration it hands each computer is
  scoped to a single runner and can be used once. The JIT configuration reaches the computer on the standard
  input of `silo exec --stream --stdin`: it is in no command line on the Mac, and in none in the
  computer. The engine does not store it: the `guest.exec.start` request is held in memory only to write
  the bytes to the command's input pipe
  ([`session.rs#L143`](../app/SiloUI/src-tauri/engine/src/guest/session.rs#L143)), and the job registry
  records only the job id, method, state, progress and the exit result, never the request
  ([`registry.rs#L217`](../app/SiloUI/src-tauri/engine/src/jobs/registry.rs#L217)). In the computer it is
  in the memory and environment of GitHub's runner program, so the job (same account) can read it; it is
  useless after the one job. Prefer a GitHub App over a token, and keep the key file `0600`: the guide
  says to, and the helper warns (at start and in `check`) when the group or others can read it; it does
  not set the mode or refuse to run.
- **Why public repositories are refused.** GitHub's guidance is that self-hosted runners "should
  almost never be used for public repositories" because anyone can open a pull request whose workflow
  runs on the runner. `silo-runner` refuses to start for a public repository, and for an organization
  when the runner group allows public repositories (`allows_public_repositories`), and when it cannot
  find out. Isolation by VM makes the Mac safer than a bare runner, but a job in a public repository
  can still use the network, abuse resources and read whatever the workflow gives it; the refusal
  stays.
- **A job can attack the next one only through the host.** Each job has its own computer, forked from
  the recipe's cached checkpoint, so nothing a job writes reaches another job's disk. The shared
  pieces are the recipe cache (written only by recipe builds and the runner refresh) and the Mac's
  CPU, memory, disk and network.
- **Runner updates.** The runner program updates itself never; the helper refreshes the recipe from
  GitHub's release digest within the 30 days GitHub allows. The refresh reads the latest release from
  the GitHub API without authentication, rewrites the pin in the recipe file under
  `<projectDir>/.silo/recipes` and runs `silo recipe build` unattended, with no one to approve the
  change. The version and the SHA-256 come from the same release, so a compromised release feed could
  swap the runner and its digest together; the only bound is the SHA-256 the recipe checks when it
  downloads the archive, which matches whatever that release publishes. Set `runnerRefresh.disabled` and
  update the pin by hand to remove this step.
- **The Linux budget.** On a Mac, `linuxBudget` (memory and count) defaults to unbounded: with it unset,
  only `maxRunners` and the free-disk guard limit how many Linux computers the helper starts, so a burst
  of jobs can use the Mac's memory. macOS is always limited to Apple's two running computers. On a Linux
  host an unset field defaults to the host's: three quarters of its total memory and one computer per
  CPU. Cgroup limits are not read, so a shared or limited host should set `linuxBudget`. That bounds a burst of jobs; it is a guard against running the host out of memory, not a
  security limit, and it does not bound CPU time, disk beyond the free-disk guard, or network use.
- **The helper only deletes what it made.** It records every computer in `<stateDir>/computers.json`:
  by name before `silo fork` (the engine has not made it yet), and by the engine's computer id once
  the fork answers. After the fork every
  engine call goes by that id, and the helper acts on a computer only while its id is in a record. A
  computer that merely has a `ci-*` name, one renamed since, and one that took the name of a computer
  that is gone are never touched. A fork the engine refuses because the name is taken
  (`Computer names must be unique.`) made nothing: the record is forgotten at once and nothing is
  deleted, even if a computer has that name. The one exception is a record whose fork did not answer
  or failed some other way, where the engine may have made the computer (a timeout, a killed command,
  unreadable output) or the run stopped between writing the name and learning the id: the record stays
  until a list that started afterwards shows a computer with exactly that name, which only the helper
  chose (`ci-<label>-<random>`, checked against the list and the record), and binds it, or shows none.
  A helper killed during a fork whose computer appears only after
  the next start's first list leaves that computer, named `ci-...`, for the owner to delete. A computer is
  forgotten, and its record removed, only when a fresh, successful `silo list` lacks its id, and it is
  deleted only after its runner is deregistered (or GitHub says it is gone) or once it is kept.
  Writes flush the file and its directory. If the ledger is lost or edited, the helper loses track of
  computers it made; it never gains control of others.
- **A job in flight across a helper restart is kept.** The restarted helper cannot know how the job
  ended, even when it ended while the helper was down, so when GitHub no longer has the runner it
  stops and keeps the computer with the result `unknown` (unless the job's result reaches it first). That computer holds the job's files and
  secrets like any kept failure and is subject to the same limits.
- **A runner host's GitHub credential is on that host, readable by the account.** On a Linux host the
  helper's configuration holds the path of a GitHub App private key or a personal access
  token (`github.auth.tokenEnv`, which under the service is read from `~/.config/silo-runner/env`). The
  guide makes both files `0600` and the helper warns when either is readable by its group or others;
  nothing sets or enforces the mode. Any process of the account can read them, and so can root; a backup of the home directory
  copies them; the token is also in the helper's environment (`/proc/<pid>/environ`, readable by the
  account and root). The account is the one that runs `silod`, so a libkrun or virtio escape from a
  job's computer (boundary 1) lands in the account that holds the credential, the engine's credential
  store and every kept computer. That credential can register runners for the repository (or
  organization): with it, someone can attach their own machine as a runner and receive the workflows'
  jobs and their secrets. Prefer a GitHub App with only the runner permission over a token, install it on
  the one repository, and do not share the account with anything else. Run the helper on a dedicated
  account, never root (`silo-runner` refuses euid 0, and the unit is a user unit).
- **The journal and who reads it.** Under systemd the helper's log goes to the user journal. It names
  computers, runner and job ids, repository, label, recipe versions and GitHub's error messages; never
  the credential or a JIT configuration (the R1 live check greps the log for the encoded JIT
  configuration). Root can read the journal, and so can members of `adm` and `systemd-journal`,
  depending on the distribution (the same exposure as the engine's journal, see the Linux hosts
  section): they learn which repository the host serves and when jobs ran, but not a credential. They
  can also read everything else the account can, so they are inside this boundary anyway. A job's own
  output goes to GitHub, not to the journal.
- **Root and the host's administrators.** They can read the credential files, the helper's memory and
  every kept computer's disk, and can start any computer. A Linux runner host is only as trustworthy as
  the people with root on it; do not run it on a machine whose administrators should not see the
  repository's secrets.
- **KVM and the engine's account.** The account needs `/dev/kvm` (the `kvm` group). The helper checks it
  at start and does not widen any permission. The unit sets `UMask=0077`, so files the helper creates
  (such as the computer record in `stateDir`) are private to the account.
- **Pins for both architectures.** The `silo-linux` recipe pins the runner's SHA-256 for `linux-arm64`
  and `linux-x64` and checks the one for the computer's architecture (`uname -m`) when it builds. The
  refresh reads both digests from the same release and rewrites the recipe only when both are there. It
  has the same trust as before: whoever controls the release feed controls both digests.
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
- **Personal access tokens.** A computer that uses one sends the whole token, unscoped, with its
  GitHub requests (boundary 5).
- **macOS computers during a first install.** Until their setup completes, a first install
  and the setup of a copy of a template run on Apple NAT and can reach the Mac and the LAN
  (boundary 2). A computer an agent may have used never does.
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
- **Egress filtering.** A new computer's policy is open: default-deny with one allow rule for
  every public address. The allowlist and closed modes narrow it per computer (see
  [Network policy per computer](#network-policy-per-computer)) but do not stop sending data to an
  allowed host, name lookups still reach the Mac's resolvers unfiltered, and there is no DNS
  query log yet. See [planned changes](#planned-changes-that-will-change-this-document).
- **Hostile apps that embed the engine.** An app that talks to `silod` is trusted with the same
  authority as Silo.app.

## Planned changes that will change this document

- A DNS query log and a filter for lookups of names outside an allowlist.
- Per-client socket authentication (macOS code signature, Linux `SO_PEERCRED`) and an audit log of
  client, method and computer.
- On a Linux host: signed engine archives (roadmap milestone 11), and the isolation evals run on a
  KVM host (roadmap milestone 9); the KVM CI job runs the computer lifecycle, not an eval.
- A security review of the read-write `/.msb` share (boundary 6).
- A public, re-runnable isolation eval in `docs/evals/` whose results back each "cannot" above.

Until the eval exists, the "cannot" column is supported by code reading, Silo's unit tests and
the manual checks recorded in [docs/SiloUI-SECRETS.md](SiloUI-SECRETS.md) and
[docs/SiloUI-NETWORK-PLAN.md](SiloUI-NETWORK-PLAN.md), not by an adversarial test suite. The `/.msb` eval result in boundary 6 is not yet recorded in the
repository.

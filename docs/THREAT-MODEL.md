# Silo threat model

This document says what Silo protects, against whom, by which mechanism, and what it does
not protect. Every mechanism cites the code that implements it, so a reader can check the claim
against the source. It describes `main` at the commit this file was last changed in; when the code
and this document disagree, the code is right and this document has a bug.

Scope: the Silo engine (`silod`, `silo-engine`, the bundled MicroSandbox runtime and the macOS
network gateway) on its only supported host today, an Apple Silicon Mac. Linux computers and
macOS computers both run as guests on that Mac. A Linux host daemon is not shipped, so nothing here
claims Linux-host behaviour.

## Summary

Silo assumes that whatever runs inside a computer is hostile: an agent that has read a malicious
issue, web page or dependency and now follows the attacker's instructions with root in the guest.
Silo tries to keep that agent from reaching the Mac, the Mac's network, other computers, raw
credentials and repositories the user did not grant. It does **not** try to keep the agent from
sending the data it can already see to the internet: outbound traffic to public addresses is
allowed, and there is no per-host egress filter yet.

| The agent in a computer can | The agent in a computer cannot (if the boundary holds) |
| --- | --- |
| Read and change everything in that computer, as root | Read or write the Mac's files, except two read-only folders lent to Linux computers and the runtime's own control folder at `/.msb` |
| Connect to any public internet address, any port | Connect to the Mac's loopback, private-range (LAN, VPN) addresses, link-local or cloud-metadata addresses, or another computer (macOS computers: only once setup is complete) |
| Use an assigned secret against that secret's allowed HTTPS hosts | Read the secret's value, or send it to any other host |
| Read granted repositories; write only those granted for changes | Read the user's GitHub sign-in or any GitHub token; touch ungranted repositories (OAuth mode) |
| Send the data it can read to any public host | Talk to the engine socket on the Mac |

## Attacker

**Primary attacker: a prompt-injected agent in a guest.** It controls every process in one computer
and has root there: Silo's working account `silo` has passwordless `sudo`
([`working-account.py#L276`](../app/SiloUI/src-tauri/guest/working-account.py#L276), and the same
for macOS computers in [`offline_setup.rs`](../app/SiloUI/src-tauri/engine/src/macos_computers/offline_setup.rs)).
It can install software, change the guest's trust store, kernel parameters and network settings,
and craft any packet, DNS query or HTTP request. It sees the placeholders for its secrets and the
read-only folders lent to it. It knows how Silo works, because Silo is open source.

**Secondary inputs it can shape**, which Silo's host side reads and must treat as untrusted:
guest web content shown in the desktop viewer, file names and sizes in `/workspace`, Git
repositories that the user pushes from the host, output relayed into logs, files the user downloads
from the computer, exported computer archives that are later imported, and the HTTP request line,
path and body of GitHub requests, which the proxy parses on the Mac to choose a token.

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
([`runtime/mod.rs#L6307`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L6307)) only pins
hostname-rule enforcement and has no effect while no hostname rule exists. The resulting
configuration, which exports compare against, is
[`github-network-default.json`](../app/SiloUI/src-tauri/guest/github-network-default.json):

- `default_egress: deny`, then allow DNS (port 53) to the gateway, and allow every protocol and port
  to the `public` destination group.
- `public` is decided by destination IP. Loopback, RFC 1918, CGNAT `100.64.0.0/10`, IPv6 ULA,
  link-local, `169.254.169.254`, multicast, the gateway itself and unspecified addresses are not
  public ([MicroSandbox `destination.rs`](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/network/lib/engine/policy/destination.rs)).
- DNS goes through the gateway, which forwards to the Mac's own resolvers (`nameservers: []`), with
  `rebind_protection: true`. Rebind protection filters answers; it does not stop connecting by IP.
  The guest can resolve LAN and VPN-internal names, and DNS queries themselves are an unfiltered
  outbound channel.

So the guest cannot open a connection to a service listening on the Mac's loopback or LAN address,
to a router admin page, to a Tailscale peer, or to another computer's guest address.

macOS computers that have finished setup run behind a per-computer gateway child,
`silo-macos-gateway` ([source](../app/SiloUI/src-tauri/macos-gateway/README.md)), which uses
the same MicroSandbox network stack. The VM's only network device is a file-handle attachment to
that child ([`engine.rs`, `start_machine_once`](../app/SiloUI/src-tauri/engine/src/macos_computers/engine.rs)),
and the child applies the `Public` profile when no other policy is given
([`network.rs#L147`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L147)).

Limits of this boundary:

- **No hostname or destination allowlist.** Any public host, any port, TCP and UDP. This is what
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
  reachable, as is a service on the Mac's own public address, if it has one.

**Ingress** (into the guest) is opened only by the user: published ports bind `127.0.0.1` on the Mac
([`microsandbox-live-public-ports-0.7.6.patch`](../app/SiloUI/patches/microsandbox-live-public-ports-0.7.6.patch)),
and opt-in SSH access binds `127.0.0.1` or one chosen interface, never a wildcard
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
  the value supplied by host environment reference, not inline. MicroSandbox's parser then sets
  substitution to **headers only** (no query, no body) and, on Linux computers, lets the unchanged
  placeholder go to any host, `passthrough=*` ([MicroSandbox `common.rs` `parse_secret`](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/cli/lib/commands/common.rs#L2893)).
- The real value is written only into requests whose verified TLS server identity matches one of
  the secret's allowed hosts (`require_tls_identity: true`, checked by
  [`secrets_runtime.rs`](../app/SiloUI/src-tauri/engine/src/secrets_runtime.rs) when it verifies the
  runtime configuration). Plain HTTP never receives it.
- Changing or removing a secret closes the computer's open proxy connections, so a connection made
  under the old policy cannot keep using it (`until_policy_change` in
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
[non-goal](#non-goals)). The `*` (any host) assignment exists, requires an explicit acknowledgement,
and removes this boundary for that secret.

### 4. TLS interception scope

To substitute a header, the proxy must terminate TLS, so it presents a certificate from the
computer's interception CA, which the guest trusts. Silo patches MicroSandbox so that only
connections whose SNI matches a **current secret's allowed hosts** are intercepted; every other TLS
connection is relayed with the server's own certificate
([`microsandbox-silo-network-0.7.6.patch#L1991`](../app/SiloUI/patches/microsandbox-silo-network-0.7.6.patch#L1991),
with its test `intercepts_only_current_secret_destinations`). Interception is limited to port 443
and blocks QUIC on intercepted destinations; the proxy verifies the upstream certificate
(`intercepted_ports`, `block_quic_on_intercept`, `verify_upstream` in
[`github-network-default.json`](../app/SiloUI/src-tauri/guest/github-network-default.json)).

Every Linux computer is created with the GitHub secret, so `github.com`, `api.github.com` and
`uploads.github.com` are always intercepted. A macOS computer with a GitHub profile has
`github.com` and `api.github.com` intercepted
([`macos-gateway/src/network.rs#L193`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L194)).
For macOS computers the CA is per computer: it is stored in the Mac's Keychain
(`engine.credentials.status` lists "one proxy CA per macOS computer",
[docs/SiloUI-ENGINE.md](SiloUI-ENGINE.md)) and written to a private temporary file for the gateway
process while the computer runs
([`macos-gateway/src/network.rs#L131`](../app/SiloUI/src-tauri/macos-gateway/src/network.rs#L132)).

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
  `issues`, `pull_requests`, `statuses` and `workflows` write and `actions` read
  ([`COMPUTER_WRITE`, `github_tokens.rs#L66`](../app/SiloUI/src-tauri/engine/src/github_tokens.rs#L66)).
  Admin and secret-bearing permissions are never requested.

These tokens are handed to the runtime as the `SILO_GITHUB` secret
([`runtime/mod.rs#L6303`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L6303)). The guest's
`GH_TOKEN` is the placeholder `$MSB_SILO_GITHUB`
([`runtime/mod.rs#L6309`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L6309)). For each
request to the GitHub hosts (three on Linux computers, `github.com` and `api.github.com` on macOS
computers) the patched proxy picks the owner and the read or write token
([`microsandbox-silo-network-0.7.6.patch#L385`](../app/SiloUI/patches/microsandbox-silo-network-0.7.6.patch#L385));
an unknown owner, a write without a write token, or an expired token is blocked. That selection is
routing, not the security boundary: **GitHub enforces the token's repositories and permissions**.
A guest that tricks the selector gets at most another token this computer already holds.

Host push is a separate, user-confirmed path: the Mac fetches the committed objects from the guest
over a private SSH transport, then pushes one confirmed commit to one branch. In OAuth mode the token is limited to
`contents: write` on that single repository and revoked when the push ends
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
[`runtime/mod.rs#L6292`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs#L6292)), and its root
filesystem is a separate disk. No host folder of the user's is shared into a computer. A Linux
computer sees three host folders.

The first is MicroSandbox's own per-computer runtime directory, mounted **read-write** at `/.msb`
(virtiofs tag `msb_runtime`). It is a control channel: the host writes scripts and TLS
certificates into it, and the guest writes a heartbeat file that the host reads. MicroSandbox
serves it from a passthrough filesystem rooted at that directory, refuses a symlinked root, and
caps guest writes at 16 MiB
([MicroSandbox `vm.rs`](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/runtime/lib/runner/vm.rs#L2014),
[`protocol/lib.rs`](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/protocol/lib/lib.rs#L79)). An eval run on a Mac
found the share read-write from the guest and could not reach a host canary file through it.
**This share has not had a security review.** The guest can change what the host later reads
there, and any escape from the passthrough root would be a host file write.

The other two are Silo-managed, **read-only** shares for built-in computer use: the downloaded ChatGPT for Linux package at `/opt/silo/chatgpt` and the verified LCU archive at
`/opt/silo/lcu` ([`computer_use.rs#L663`](../app/SiloUI/src-tauri/engine/src/computer_use.rs#L663),
[`#L708`](../app/SiloUI/src-tauri/engine/src/computer_use.rs#L708)).

Host features read the disk only through the guest: directory listings run a bounded script in the
guest without following links ([`files.rs`](../app/SiloUI/src-tauri/engine/src/files.rs)), and
file transfers use `sftp` as the working account with sizes bounded on the host
([`transfer.rs`](../app/SiloUI/src-tauri/engine/src/transfer.rs)). The two read-only shares are
served by the `msb` process on the Mac (virtiofs), so that process parses the guest's filesystem
requests; read-only is enforced on the host side.

Imported Linux computer archives are parsed on the Mac: a zstd-compressed tar is streamed through
caps on entry types, paths, entry count and size
([`backup.rs#L3552`](../app/SiloUI/src-tauri/engine/src/backup.rs#L3552)).

Exception: **macOS computer disks are attached and mounted on the Mac** with `hdiutil` while Silo
edits the Data volume, during first setup and when an exported macOS computer is imported
([`offline_setup.rs`](../app/SiloUI/src-tauri/engine/src/macos_computers/offline_setup.rs);
import at [`macos_computers.rs#L3230`](../app/SiloUI/src-tauri/engine/src/macos_computers.rs#L3230)).
An imported archive from a computer an agent controlled is therefore parsed by the Mac's APFS
driver. Import only archives you trust.

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
open the guest over SSH. There is no per-client identity and no audit log of which client asked
for what. The guest is not such a process: the socket is on the Mac's filesystem, which the guest
cannot see.

At start, `silod` verifies its own bundle signature against `SiloEngineCodeRequirement`, and a
production engine refuses to run without one
([`code_signature.rs`](../app/SiloUI/src-tauri/silod/src/code_signature.rs)). That protects which
binary serves the socket, not who connects to it.

### 8. Host surfaces that read guest-controlled data

| Surface | Control | Code |
| --- | --- | --- |
| Linux desktop viewer | Guest web content loads in a child webview that matches no Tauri capability; only the local `desktop-shell-*` webview has desktop permissions. The loopback desktop proxy checks Host and Origin, uses an HttpOnly cookie, strips client credentials, and reaches the guest through a Unix socket in a private directory. | [`capabilities/desktop-viewer.json`](../app/SiloUI/src-tauri/capabilities/desktop-viewer.json), [`desktop_viewer.rs`](../app/SiloUI/src-tauri/engine/src/desktop_viewer.rs), [`desktop_proxy.rs`](../app/SiloUI/src-tauri/engine/src/desktop_proxy.rs) |
| macOS screen, same Mac | Native `VZVirtualMachineView`, no webview. | [docs/SiloUI-MACOS-COMPUTERS.md](SiloUI-MACOS-COMPUTERS.md) |
| macOS screen, another Mac | A noVNC page in a Tauri webview speaks RFB to the guest's Screen Sharing through a loopback WebSocket endpoint (random per-window token, Host and Origin checks, one connection, message size limit) spliced to the owner over the SSH bridge. The webview parses guest-controlled RFB data. | [`macos_display_gateway.rs`](../app/SiloUI/src-tauri/engine/src/macos_display_gateway.rs), [display path](SiloUI-MACOS-COMPUTERS.md#on-another-mac) |
| Clipboard | The desktop viewer copies in either direction only on an explicit user action (shortcut, menu item or button). Engine clients can also read or write a macOS computer's clipboard on request. There is no background sync. | [`viewer_clipboard.rs`](../app/SiloUI/src-tauri/engine/src/viewer_clipboard.rs), [`macos_computers.rs`, `macos_clipboard_read`](../app/SiloUI/src-tauri/engine/src/macos_computers.rs#L2489) |
| Host push | Host Git uses a sanitized configuration and fixed executable; the guest supplies objects through standard protocols, not hooks or configuration. | [`host_push.rs`](../app/SiloUI/src-tauri/engine/src/host_push.rs), [`host_push_transport.rs`](../app/SiloUI/src-tauri/engine/src/host_push_transport.rs) |
| Editor and terminal | The user's editor connects over SSH with no agent forwarding and a pinned guest host key; guest paths are URL-encoded and control characters refused. The editor's own remote trust model still applies. | [`editor.rs`](../app/SiloUI/src-tauri/engine/src/editor.rs) |
| Published ports | Guest services appear on Mac loopback; opening one in a browser runs guest content at a per-computer `.localhost` origin. | [`network.rs`](../app/SiloUI/src-tauri/engine/src/network.rs) |

The 2026-10-02 security review checked these surfaces in source
([report](SiloUI-CODE-REVIEW-PASS-3-SECURITY-2026-10-02.md)); it was not a live exploit campaign.

## What Silo does not protect

These follow from the design today, not from bugs:

- **Data the agent can read can leave.** Repository contents, files in `/workspace`, and responses
  from services the agent can call can be sent to any public host, including through an allowed
  host (for example a gist or an issue on a repository it can write).
- **Credentials can be used, not just stolen.** A scoped secret or GitHub token works for the agent
  exactly as for the user, within its scope.
- **Guest integrity.** The agent is root in the guest. Anything inside a computer, including tools
  Silo installed there, can be changed by it. Roll back with checkpoints.
- **Resource abuse.** CPU, memory and disk are capped per computer; outbound bandwidth and the
  number of connections are not.

## Non-goals

- **Malware running as the same user on the Mac.** It can already read the user's files, connect to
  the engine socket (boundary 7) and drive the Keychain prompts the user approves. Silo does not
  defend against it.
- **Hypervisor and kernel 0-days.** A VM escape in libkrun, Hypervisor.framework,
  Virtualization.framework or macOS is out of scope; Silo tracks upstream fixes but adds no
  defence-in-depth layer today.
- **Reflected-secret exfiltration.** If an allowed host returns the request's headers (debug echo
  endpoints, some error pages), the guest sees the real value. Restrict secrets to hosts you trust.
- **Egress filtering.** There is no per-computer domain allowlist, default-deny mode or DNS log yet.
  Planned as "per-computer egress policy" (ship list S13).
- **Hostile apps that embed the engine.** An app that talks to `silod` is trusted with the same
  authority as Silo.app.

## Planned changes that will change this document

- Per-computer egress policy (default-deny or allowlist, with DNS log).
- Per-client socket authentication (macOS code signature, Linux `SO_PEERCRED`) and an audit log of
  client, method and computer.
- A Linux host daemon, which will need its own boundary review.
- A public, re-runnable isolation eval in `docs/evals/` whose results back each "cannot" above.

Until the eval exists, the "cannot" column is supported by code reading, Silo's unit tests and
the manual checks recorded in [docs/SiloUI-SECRETS.md](SiloUI-SECRETS.md) and
[docs/SiloUI-NETWORK-PLAN.md](SiloUI-NETWORK-PLAN.md), not by an adversarial test suite.


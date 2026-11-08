# Network gateway helper source (`silo-macos-gateway`)

The helper runs MicroSandbox's network stack as a host process for a computer that has no
in-process stack: a macOS VM, and a container computer on a Linux host. It keeps the name it
was built under (renaming it would touch the macOS bundle, its signing and the engine's lookups
for no gain). It builds for `aarch64-apple-darwin` and for `x86_64`/`aarch64-unknown-linux-gnu`.

This source is copied into the pinned, patched MicroSandbox checkout as a helper binary. The
`Cargo.toml` is a build template for that location: its MicroSandbox paths expect this directory
to sit beside `crates/` and `packages/`. The production Silo build must use the staged patched
checkout, not a direct Git dependency from this template.

The helper uses public APIs from `microsandbox_network::network`:

- `SmoltcpNetwork` and `SecretsHandle`
- `generate_intercept_ca_pair`
- Silo patch additions `SecretsHandle::{put_secret_and_wait, rotate_value_and_wait,
  remove_and_wait, set_allowed_hosts_and_wait, set_github_profile_and_wait}`
- Silo patch additions `SmoltcpNetwork::start_and_wait_for_ports` and
  `InterfaceOverrides::ipv6_enabled`

`start_and_wait_for_ports(handle, timeout)` returns
`Result<Vec<PublishedPortBinding>, _>`; each binding exposes the actual `host_port`,
`guest_port`, and `protocol` from the listener that remains open. This is the readiness boundary
used by the helper.

Each update method must wait for old-generation intercepted and bypassed sessions to close. A
timeout returns an error, shuts down frame fd 3, and exits. The `Ready` response omits the CA key;
only `--generate-ca` writes `{cert_pem,key_pem}` to stdout, prefixed by a four-byte big-endian
length. That mode does not access inherited frame or control descriptors.

## Inherited descriptors and control messages

- fd 3 carries raw Ethernet frames. For a macOS VM (init `profile` absent or `"vm"`) it is a
  connected `AF_UNIX/SOCK_DGRAM` socket. For a container (`"profile":"container"`, Linux only)
  it is a TAP device opened with `IFF_TAP | IFF_NO_PI` inside the container's own user and
  network namespace, so the gateway is the only way out of that namespace; the helper checks
  with `TUNGETIFF` that the descriptor is such a TAP (no packet information, no virtio-net
  header) and refuses anything else with `invalid_frame_carrier`. A TAP is read with `readv` and
  written with `write` (`recvmsg` and `send` do not work on it); a frame longer than the frame
  buffer is counted as truncated and dropped.
- fd 4 is a connected `AF_UNIX/SOCK_STREAM` control socket.
- Each control message is a four-byte big-endian JSON length followed by the JSON body; the
  maximum body size is 1 MiB.

The parent sends one `init` message with `version`, `slot`, `guest_ipv4`, `guest_mac`, required
`dns_nameservers`, `loopback_ports`, and `ca:{cert_pem,key_pem}`; `mtu`, strict `policy`,
`intercepted_ports`, initial `secrets`, and `github_profile_json` are optional. An empty
`dns_nameservers` list leaves DNS to MicroSandbox's host resolver table: the primary resolver,
each service's supplemental match domains, and `/etc/resolver/*`. A name goes to the resolver with
the longest matching domain, otherwise to the primary one, and the table is re-read whenever the
host's DNS configuration changes while the gateway runs. Only queries addressed to the gateway
use it, so the guest still cannot choose its upstream. A UDP answer is limited to the EDNS buffer
the guest advertised and to one datagram of `mtu`; a larger one is truncated (TC=1) so the
guest retries over TCP. Empty credentials are
valid when an adapter will apply them later.

`loopback_ports` for a macOS VM contains exactly two TCP entries: `{host_port:0,guest_port:22}`
and `{host_port:0,guest_port:5900}`. For a container it is the computer's own ports (up to 32,
each guest port once per protocol, possibly none). Port zero asks MicroSandbox to bind an
ephemeral loopback port; the helper never binds and releases a port itself. Ready returns the
actual ports reported by the listeners that remain bound.

`github_hosts` (container profile only; a VM keeps the fixed pair) names the hosts the GitHub
credential covers: `github.com` and `api.github.com` plus, optionally, `uploads.github.com`,
which is the full set the credential selection accepts. A later `set_allowed_hosts` for
`SILO_GITHUB` must name exactly the set fixed at init.

The container profile's CA is stored by the engine in its credential store: the file store on a
Linux host (`engine/src/credential_file_store.rs`, one 0600 file per credential), the Keychain
on a Mac. The helper itself never stores it.

After MicroSandbox reports all requested loopback listeners bound, the helper returns
`type:"ready"` with `slot`, guest and gateway IPv4/MAC, `prefix_len`, `dns_ipv4`, `mtu`,
`ca_cert_pem`, and the confirmed `loopback_ports`.

Revisioned update operations are `put_secret`, `rotate_secret`, `remove_secret`,
`set_allowed_hosts`, and `set_github_profile`; the GitHub operation carries
`profile_json:string|null`. The helper ACKs only after the close barrier completes, with
`closed_previous_generation:true`. Invalid requests return generic error codes without values.

## Trust-probe mode

`--issue-trust-probe` reads one length-prefixed `{cert_pem,key_pem}` CA bundle on stdin and
writes one length-prefixed `{leaf_pem}` response: a certificate for `TRUST_PROBE_HOST` signed by
that CA, valid for a year, whose key is discarded. The guest bootstrap evaluates it with the
system's trust settings, so readiness proves the CA is trusted, not merely present. Like
`--generate-ca`, this mode does not access the inherited frame or control descriptors.

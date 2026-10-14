# macOS proxy network bootstrap

`com.silo.network-bootstrap.plist` runs `silo-network-bootstrap` as a root
LaunchDaemon. The helper uses SystemConfiguration preferences and the active
Dynamic Store. It selects the network interface by the expected MAC address,
creates a service in the active set when needed, puts that service first, and
configures the guest's `/30` address, router, and DNS server together. It
disables the service's IPv6 protocol, commits the preference changes, and
applies them.

The helper waits for active interface, route, resolver, and IPv6 state. It then
requires explicit `dig @server`, plain `dig`, `getaddrinfo`, `dscacheutil`, and
system-trust HTTPS checks to pass. Readiness never comes from saved preferences
alone. It reads `/etc/resolv.conf` and `scutil --dns`; it does not change either
file or flush resolver caches. Failure writes a public receipt with check names
and a stable error code, then exits nonzero so launchd retries after its
throttle interval. The config, receipt, and logs contain no credential values.

SystemConfiguration's preference commit and apply operations are both required
because saved network preferences and the active configuration are distinct.
The helper follows Apple's `SCPreferencesCommitChanges` and
`SCPreferencesApplyChanges` contract, then checks the active Dynamic Store and
the selected kernel routes before it reports readiness.

## Host integration contract

Before the guest's first proxy-backed boot, the macOS computer finalizer must
install the helper at
`/Library/PrivilegedHelperTools/silo-network-bootstrap`, install this plist at
`/Library/LaunchDaemons/com.silo.network-bootstrap.plist`, and install a
root-owned config at
`/Library/Application Support/Silo/NetworkBootstrap/config.json`. The config
must be mode `0644` or stricter, owned by root, and contain the computer ID,
stable guest MAC, guest IPv4 address, `/30` prefix, gateway/DNS address, and a
public HTTPS probe URL. Set the helper executable bit and keep the config and
plist non-writable by group and other users. The guest must already trust the
proxy CA before the first helper run. Load the daemon with
`launchctl bootstrap system /Library/LaunchDaemons/com.silo.network-bootstrap.plist`;
when replacing an already loaded daemon, boot it out by label before
bootstrapping it again. Each computer clone needs its own config and a cleared
stale receipt before its first proxy-backed boot.

The host must keep the FileHandle gateway unavailable to normal computer
traffic until it reads a `status: "ready"` receipt over the guest management
channel and verifies all checks are true. A failed receipt or missing receipt
is a setup failure; the host must not fall back to NAT.

The source is compiled for Apple Silicon macOS 14+ by
`build-network-bootstrap.sh OUTPUT`. `test-network-bootstrap-config.sh` builds
the helper, validates config parsing, and runs the inherited-CA cleanup script
against a synthetic keychain fixture. It does not modify user Keychains, guest
network preferences, or exercise a VM.

References: [SCNetworkConfiguration](https://developer.apple.com/documentation/systemconfiguration/scnetworkconfiguration),
[SCPreferencesApplyChanges](https://developer.apple.com/documentation/systemconfiguration/scpreferencesapplychanges(_:)),
[ISC `dig`](https://bind9.readthedocs.io/en/stable/manpages.html#dig-dns-lookup-utility).

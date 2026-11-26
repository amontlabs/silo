# gvproxy's changes, ready for upstream (25 and 26 November 2026)

An agent's work, for the open decision `network-stack`. Nothing here is a feature polarzero took,
and nothing was sent upstream: sending is public, so it is polarzero's call. It continues #380 (a
330-line patch to gvproxy for Silo, with a suite), #395 (a one-line fix of gvproxy's
`-ec2-metadata-access`) and #363. Everything ran on Linux, with network namespaces standing in for
the computer. Nothing ran on a Mac.

## What was found

- **#380's patch is now ten patches, nine of them written for upstream.** Each is one pull request
  with its commit message as the description: three bug fixes, five options and one small hook.
  The tenth, the `-sealed` and `-closed` shorthands, stays in Silo's fork. If the nine merge, Silo
  runs upstream gvproxy with its own flags and keeps no fork at all.
- **Upstream is already building half of it.** PR #750, "Add a dial filter and a DNS answer
  observer for embedders" (approved, not merged), adds the one place every forwarder asks before it
  dials. #687, #609 and #631 add allow lists by name on top of it, and issue #630 asks for a
  deny-all policy. So the deny list and the hand-off are rebuilt on #750's filter instead of #380's
  own hook. Upstream reviewers will see a first user of their API, not a competing one.
- **Three bugs of today's gvproxy, each shown failing first.** All three are in the suite's `stock`
  columns, and two also have unit tests that fail on main ([`evidence/`](evidence/)):
  1. `-ec2-metadata-access` does nothing, and has not since the configuration file came in (#484,
     v0.8.8). The flag sets a field nothing reads. #395 found this. The fix is #395's line, and also
     covers the configuration file's top-level key.
  2. Ping goes to 169.254.0.0/16, which TCP and UDP refuse without `-ec2-metadata-access`. The
     ICMP forwarder (#597, 0.9.0) has no such check.
  3. **New: a connection to a port of the gateway where gvproxy serves nothing leaves the virtual
     network.** `192.168.127.1:7790` is dialled as `192.168.127.1:7790` on the Mac's own network,
     where a machine of a home or office network on 192.168.127.0/24 answers. With the gateway API
     turned off, port 80 behaves the same way, so `-no-gateway-api` alone would have opened a path
     to such a machine. #380's `-sealed` hid it, because its deny list refuses private addresses.
     The fix refuses those connections unless NAT translates the gateway (Lima's setting).
- **Five of the patches need nothing else and apply to upstream `main` today** (`patches/on-main/`):
  the two older bug fixes, `-no-gateway-api`, `-no-host-loopback` and `-dns-upstream`. The other
  four need #750.
- **`-dns-upstream` is rebuilt to fit upstream.** #380 answered names through a path of its own.
  Now the configured servers answer through the same code as the Mac's resolver, so answers keep
  their shape and #750's observer sees them. `none` still refuses every name.
- **The hand-off is now a hook plus a flag.** Upstream gets a general `DialTCP` hook next to #750's
  filter. `-outbound-handoff` is the PROXY v2 dialer built on that hook, and it lives in a new
  package `outbound` with the deny list.

## The suite

[`suite/`](suite/) is #380's, with three additions: a metadata service at `169.254.169.254` on the
local network, a ping to it, and a machine of the local network that holds `192.168.127.1`.
[`suite/expected.txt`](suite/expected.txt) now holds what a correct gvproxy does, so a result of
stock gvproxy that differs from it is a bug of stock gvproxy. Results are in
[`suite/results/`](suite/results/), and the whole table is in
[`verdict.md`](suite/results/verdict.md).

| Setting | gvproxy and flags | Cases that differ from expected |
|---|---|---|
| `stock` | upstream, no flags | 2: the gateway's address leaves, ping reaches 169.254.169.254 |
| `stock-ec2` | upstream, `-ec2-metadata-access` | 2: the gateway's address leaves, metadata over TCP stays blocked |
| `ec2` | the series, `-ec2-metadata-access` | 0 |
| `on-main` | the five patches on main, `-no-gateway-api -no-host-loopback` | 2: the gateway's address leaves, as expected without the fix that needs #750 |
| `sealed` | the series, `-sealed` | 0 |
| `explicit` | the series, the upstream flags `-sealed` stands for | 0, the same as `sealed` |
| `closed` | the series, `-closed` | 0 |
| `boundary` | the series, hand-off to #380's stand-in boundary | 0 |
| `boundary-metadata` | #395's setting: hand-off, deny only the Mac's addresses and networks, `-ec2-metadata-access` | 0: metadata reaches the boundary, the local network does not, from gvproxy's list alone |
| `boundary-down` | the series, hand-off with no boundary running | 0: nothing leaves |

`check.sh` applies each set to upstream one patch at a time. After every patch it builds for Linux,
macOS and Windows and runs the Go tests. It then runs upstream's `make lint`. All of that passed,
with 0 lint issues on either set.

## How to run it

```sh
./check.sh                         # every patch, built and tested in turn, then upstream's lint
./build.sh /root/gvbin             # stock, on-main and series builds, gvforwarder, the stand-in
sudo suite/runall.sh /root/gvbin   # all ten settings; needs iproute2, curl, ping, dhclient
```

## What to send upstream

[`upstream.md`](upstream.md) holds the issue that opens the conversation, the order of the nine
pull requests, and a note to add on each. The order is: the two older bug fixes first, then the
issue with the three options that need nothing, then the four that need #750, once it merges.

## What this changes in the plan

- `network-stack`: an agent's note added. With these patches, the stack Silo needs is upstream
  gvproxy with flags, plus no fork if upstream merges them. Its answer does not change.
- `network-modes`, `network-before-rules`: the settings `closed`, `explicit` and `boundary` give
  the three modes with only upstream flags, plus #750. The gateway fix (bug 3) is needed for the
  "never your local network" promise whenever the deny list does not hold `private`, as in
  #395's metadata setting.
- #395's conflict 2 (hand-off against #379's metadata server) is settled: with patch 1 and
  `-outbound-deny host,host-networks`, metadata reaches the boundary and the local network does
  not.

## Not verified

- Nothing ran on a Mac: the macOS build compiles, nothing more.
- Whether upstream wants these options, or under these names, was not asked.
- #750 may change before it merges. `check.sh` shows on the day whether the patches still apply.
- `plan check` reports 23 errors with or without this change, on Node 22 (the repository asks for
  Node 26), so that run shows nothing about this change.
- The ICMP handler leaks a packet reference on early returns that were there before (and in
  #750's filter path). Patch 2 releases it on its own path only.

## For you to rule

1. **Send upstream, and under whose name.** The DCO sign-off has to be yours, and so does the
   decision to name the gateway-address bug in public.
2. **Bug 3 privately first?** It can lead a guest to a machine of the user's own network that holds
   192.168.127.1, which is uncommon. Upstream's policy sends vulnerabilities to
   `security@lists.podman.io`. Recommended: send bug 3 there first with the patch, and the rest in
   public. It is a small, rare path, but an embargo costs little.

The research at a glance: #380's gvproxy patch is now nine upstream-ready pull requests built on
upstream's own dial filter (#750), plus one fork-only shorthand. Three bugs of today's gvproxy are
shown failing and fixed, including a new one: a connection to the gateway's own address leaves for
the host's network. The extended suite passes in all seven Silo settings.

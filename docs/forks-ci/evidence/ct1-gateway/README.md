# CT1: the gateway as a container's network, on the GitHub KVM runner

Phase CT1 of [CONTAINERS-PLAN.md](../../CONTAINERS-PLAN.md). Pull request
[#92](https://github.com/amontlabs/silo/pull/92); workflow run
[42468047145](https://github.com/amontlabs/silo/actions/runs/42468047145), job `Gateway as a container's
network` (commit `56af53e8`, `ubuntu-24.04`, kernel 7.0.0-1017-azure, x86_64, curl 8.5.0). Raw results:
[`script/results.json`](script/results.json) and [`engine/results.json`](engine/results.json).

**Question.** Can the gateway that already runs macOS VMs (`silo-macos-gateway`: Public egress policy, header
substitution of secrets, per-computer CA, DNS, published ports) serve a Linux container whose only network
interface is a TAP device, with no root step in the path?

**Result.** Yes. 22 of 22 checks passed, twice: once with the check script driving the gateway's control
protocol, once with the gateway started by the engine's own code (`silo_engine::computer_gateway`, through
the `gateway_tap_harness` example). The gateway crate's own tests also passed on the runner
(`scripts/test-gateway.mjs`).

## How it ran

[`scripts/gateway-live-check.py`](../../../../app/SiloUI/scripts/gateway-live-check.py), as the runner's
ordinary user:

1. `unshare --user --map-root-user --net --mount` makes a user and network namespace. Inside it the script
   creates a TAP (`IFF_TAP | IFF_NO_PI`), sends the descriptor to the host side over a socket and closes its
   own copy, so the gateway holds the only descriptor of the device.
2. The gateway runs in the host's own namespaces with the TAP as fd 3 and the control socket as fd 4, as
   the supervisor will hand them over. Init uses the `container` profile, one published port, and one fake
   secret (`CT1_FAKE_TOKEN`, a random `ghp_`-shaped value made for the run, allowed for `api.github.com`).
3. Inside the namespace the script gives the TAP the address and MAC from `ready`, a default route via the
   gateway, and `/etc/resolv.conf` pointing at the gateway (a bind mount in the private mount namespace).
   The namespace holds only `lo` and the TAP.

No step runs as root. `sudo` was used only to install packages and to set
`kernel.apparmor_restrict_unprivileged_userns=0` (Ubuntu 24.04's host-wide guard on unprivileged user
namespaces). Nothing sets or checks it yet: CT2 adds it to `silo host init`.

## What was checked

| Check | Result |
|---|---|
| The namespace holds only `lo` and `tap0` | passed |
| Public HTTPS host (`example.com`) answers through the gateway; public plain HTTP answers | passed (200) |
| `10.255.255.1`, `192.168.0.1`, `169.254.169.254`, the host's own address and the gateway's own address (`gateway_ipv4`; plain and over TLS for the last two) are refused | RESULT_PRIVATE |
| Nothing from the namespace reached a listener bound on all of the host's addresses | passed (0 connections) |
| A TUN device (`IFF_TUN`) handed over as the frame descriptor | the gateway answers `invalid_frame_carrier` and exits non-zero |
| `GET https://api.github.com/user` with `Authorization: Bearer $MSB_CT1_FAKE_TOKEN` | 401 "Bad credentials": the placeholder was substituted (see below) |
| The same request without a token | 401 "Requires authentication" |
| DNS query to the gateway for `example.com` | answered (rcode 0) |
| Three names that resolve publicly to `127.0.0.1` (`localtest.me`, `127.0.0.1.nip.io`, `127.0.0.1.sslip.io`) | all answered NXDOMAIN: the private answer is blocked |
| A port the computer publishes (8080 in the namespace) answers on the host's loopback | passed |
| TLS interception scope | `example.com` keeps its public certificate (issuer SSL Corporation); `api.github.com`'s certificate is refused by the system store and accepted with the gateway CA (issuer `microsandbox`) |
| Credential-use log | parsed as JSON lines (none malformed): a `used` event with the secret name, `api.github.com` and `GET`; the value is not in the file; no `used` event for any other host or secret |
| Frames | 112 guest to engine and 78 engine to guest in the engine run; no MAC or IP spoof, no malformed frame, no write error |
| `shutdown` | the gateway answered `stopped` and exited 0 |
| The check ran without root | euid not 0 |

**Reading two rows.** On the GitHub runner the "host's own address" is a `10.x` address, so that check
repeats the private-range check; it is not the threat model's known limit, a service listening on the host's
public address. And substitution is shown by GitHub's "Bad credentials" answer to the fake token (a request
without a token gets "Requires authentication"), not by an echo of the value, which never leaves the gateway;
the credential-use log's `used` event is the gateway's own record of it.

The TLS interception row matches the existing rule (MicroSandbox patch: "intercept only hosts allowed to
receive a current secret"; other TLS keeps its public certificate), now seen on the TAP carrier.

## Not covered

- Only x86_64 was run live. The aarch64 gateway builds on the same code path and was not run on a runner.
- One published TCP port; UDP ports and many ports are covered by unit tests of the validation only.
- No container runtime: the namespace is a bare one made by `unshare`. CT2 adds the runtime and CT3 repeats
  these checks from inside a container.
- The first run of this job failed on two checks of the script itself (it listed the host's interfaces
  because `/sys` still belongs to the original namespace, and it expected every host to be intercepted); the
  gateway's behaviour was as designed and the checks were corrected, then rerun to the result above.

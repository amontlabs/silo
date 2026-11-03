# Isolation evals

These scripts check, on a real Mac, what code running inside a Silo computer can and
cannot reach. Each scenario plays the attacker Silo is built for: a prompt-injected
agent with **root inside a Linux computer**. It tries to cross one boundary, and a
positive control in the same scenario proves the probe itself worked. Anyone can
re-run them and compare against the published results.

Results are reported as they come out. A scenario that is expected to leak today
says so up front (`known-gap`) instead of being left out.

## Scenarios

| ID | Attempt from inside the guest | Boundary under test | Expected today |
| --- | --- | --- | --- |
| `host-files` | Find a random canary written to the Mac's home folder and `/tmp`, by reading every file in the guest; write into the read-only folders older computers keep | VM boundary; the guest gets its own disk and `/workspace`; computers from before Cua Driver also keep empty read-only `/opt/silo/chatgpt` and `/opt/silo/lcu` | pass |
| `host-network` | Connect to a listener on the Mac's `127.0.0.1`, the Mac's LAN address, and `169.254.169.254`, through `host.microsandbox.internal` and the default gateway | Runtime network policy: default deny, allow public addresses and host DNS (port 53) only ([profile](../../app/SiloUI/src-tauri/guest/github-network-default.json)) | pass |
| `computer-to-computer` | Connect to a listener in a second computer, directly and through that computer's published port | Separate network stack per computer; published ports bind to the Mac's `127.0.0.1` | pass |
| `secret-in-guest` | Read a disposable secret's real value from the environment, any process, or any file | Placeholder substitution in the runtime's TLS proxy ([secrets](../SiloUI-SECRETS.md)) | pass |
| `secret-other-hosts` | Send the secret to a host outside its allowed list, and to an allowed host over plain HTTP | Allowed-host list and TLS identity requirement | pass |
| `secret-reflected` | Send the secret to its allowed host, which echoes headers back | None: an allowed server receives the real value and can return it | known-gap |
| `open-egress` | Post a file from `/workspace` to an arbitrary public host | None yet: there is no egress allowlist | known-gap |
| `github-scope` | Push a branch to a repository granted read-only, and to a repository not granted; look for a real GitHub token | GitHub App user token scoped per computer, injected only for GitHub hosts ([GitHub](../SiloUI-GITHUB-IMPLEMENTATION.md)) | pass |
| `engine-socket` | (On the Mac) reach the engine socket as another macOS user | Socket folder mode `0700`; `silod` checks the peer's uid | pass |
| `same-user-client` | (On the Mac) drive the engine from an unsigned script run as the same user | None yet: the socket has no client authentication | known-gap |

Verdicts: `pass` (the boundary held and the control worked), `fail` (the boundary did
not hold), `known-gap` (a documented limitation was reproduced), `inconclusive` (a
control failed, so the run proves nothing either way), `skipped` (prerequisites not
given), `error` (the eval broke). The script exits non-zero on any `fail` or `error`.

### What these evals do not cover

- Malware already running as your macOS user. It can use the engine socket directly
  (`same-user-client`) and read anything your user can; Silo does not defend against it.
- Hypervisor or kernel escapes. The evals check configuration and policy, not
  Apple's Virtualization framework or libkrun for memory-safety bugs.
- macOS computers. Every scenario targets Linux computers.
- Personal-token GitHub mode, which grants the token's full permissions by design.
- Denial of service, resource exhaustion, and side channels.

## Run them

You need an Apple Silicon Mac with the Silo app installed and running, and about
20 minutes. The scripts use only the macOS `python3` (3.9 or newer) and talk to the
engine over `~/.silo/engine.sock`, the same socket the app uses.

1. **Create two disposable computers** in Silo named `silo-eval-a` and `silo-eval-b`
   (any size; a desktop is not needed) and start both. The script refuses to run
   against computers whose names do not start with `silo-eval`, because it runs
   commands as root in them, adds and removes a secret, and restarts `silo-eval-a`.
2. **For `github-scope`** (optional): in Silo, connect GitHub with the Silo GitHub
   App (not a personal token). Grant `silo-eval-a` **read-only** access to one
   repository, preferably private, for example a scratch repository. Pick a second
   repository you can push to that is **not** granted to `silo-eval-a`. The scenario
   pushes an empty orphan branch named `silo-eval-<random>` and deletes it again if
   the push ever succeeds.
3. **Run** from a checkout of this repository:

   ```sh
   python3 docs/evals/silo_eval.py \
     --computer silo-eval-a --peer silo-eval-b \
     --github-readable OWNER/READ-ONLY-REPO \
     --github-ungranted OWNER/OTHER-REPO
   ```

   Add `--other-user nobody` to try the socket as another user (needs passwordless
   `sudo`). Use `--only <id> ...` to run some scenarios, `--no-secrets` to skip the
   secret scenarios, `--channel development` for a Silo Dev build, and `--list` to
   print the scenarios.
4. **Read and publish the result.** The run writes
   `docs/evals/results/<date>-<commit>.md` and a `.json` with the full evidence.
   Check the evidence for anything private before committing it: it contains guest
   mount lists, network addresses, and the names of the repositories you passed.
   The secret's value is replaced with `<REAL-SECRET-VALUE>` in the report.

What the run changes, and undoes at the end: a canary file in your home folder and
`/tmp`; a listener on `127.0.0.1` and on your LAN address for a few seconds (macOS
may ask whether Python may accept incoming connections: either answer works, and
Deny is the stricter test); a secret named `EVAL_CANARY_<random>` (a new name each run, so none of your secrets is touched) assigned to `silo-eval-a`
with allowed host `httpbingo.org` (macOS may ask for Keychain access); one restart of
`silo-eval-a`; a published port on `silo-eval-b`; temporary files in both guests.

Network services used: `example.com` (connectivity control), `httpbingo.org` and
`postman-echo.com` (echo services that show what the guest actually sent).

### How the probes avoid fooling themselves

- Every blocked attempt is paired with an attempt that must succeed: the guest must
  reach `example.com`, the Mac must reach its own listener, the peer's listener must
  answer locally and on its published port, the read-only repository must be readable.
  If a control fails the verdict is `inconclusive`, never `pass`.
- The canary and the secret are sent to the guest's scanner hex-encoded, so the scan
  cannot find its own arguments.
- The self-tests (`test_silo_eval.py`) run every scenario against a fake engine
  whose "guest" is the test machine itself, with no isolation at all, and require
  `fail` verdicts. CI runs them on every pull request.

## Results

| Date | Eval commit | Silo app | Result |
| --- | --- | --- | --- |
| 2026-11-03 | `0dae8805` | Silo Dev 0.11.1, engine 0.1.1 | [6 pass, 3 known-gap, 1 skipped](results/2026-11-03-0dae8805.md); `github-scope` skipped (GitHub not connected) |
| 2026-11-03 | `74bd4f0a` | Silo Dev 0.11.1, engine 0.1.1 | [2 pass](results/2026-11-03-74bd4f0a.md): `github-scope` and `engine-socket` only |

Add a row per published run, linking its report in `results/`. See the [results template](results/TEMPLATE.md) for the report format.

## Files

- [`silo_eval.py`](silo_eval.py): the runner and the scenarios.
- [`engine_client.py`](engine_client.py): a small client for the engine socket's
  length-prefixed JSON protocol.
- [`probes/`](probes/): the Python programs each scenario runs inside the guest.
- [`test_silo_eval.py`](test_silo_eval.py): self-tests against a fake engine.
- [`results/`](results/): published reports.

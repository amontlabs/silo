# R1 silo-runner: live check against a private repository (7-8 Nov 2026)

Repository: `0xpolarzero/silo-runner-e2e` (private, confirmed with `gh api repos/... --jq .visibility`).
Script: [`checks/r1_runner_check.sh`](../../checks/r1_runner_check.sh). Data: live (the Dev engine of this Mac,
GitHub's real service). Build: integration of `claude/forks-phase-3` and `claude/runner-phase-1` on `origin/main`
(merge `f67e2694`), release-profile Dev bundle; `silo-runner` built with `go build` from this branch. Authentication
was the output of `gh auth token` (an OAuth token with the `repo` scope, not a fine-grained token) in an environment
variable: it is in no file, log or command line (checked with a search of every file here for the token and for
`gho_`, `ghp_`, `github_pat_` and `eyJ`). The Dev engine was started by hand from the bundle because launchd rejects
the new ad-hoc-signed engine (`needs LWCR update`).

## Result: Linux recipe (`silo-linux`, label `silo-linux-r1`): 19 of 19 checks pass

[run-2.txt](run-2.txt) and [runner-2.log](runner-2.log) are the final run. Highlights:

- **Fresh computer per job, named after the runner shown on the job page.** The success job ran on
  `ci-silo-linux-r1-d15ffebc` (`hostname` and `$RUNNER_NAME` both printed that name, `uname -a` showed
  `Linux ... aarch64`, a C program built and ran: [github-job-success.log](github-job-success.log)); the failing job
  ran on `ci-silo-linux-r1-dce55915` ([github-job-failure.log](github-job-failure.log)). Job request to "runner
  ready": about 11 s; success job to computer deleted: 3 s (`runner-2.log` shape, same in run 1).
- **Success:** the computer was deleted (gone from `silo list --all`) and its runner is gone from GitHub (0 runners).
- **Failure:** the computer is kept, stopped (`ci-silo-linux-r1-dce55915  stopped  linux  4 cpu, 8 GiB`), its runner
  gone from GitHub; log line `kept a computer after a job that did not succeed ... result=failed`.
- **No JIT configuration on any command line.** A sampler polled `ps` every 0.5 s for the whole job window and
  looked only at silo, silo-runner and runner-program processes; it saw the `exec --stdin` runner command 26 times and
  no `eyJ`. The log has none either.
- **Restart reconcile:** after stopping and starting silo-runner the kept computer was untouched, still stopped, and
  no other `ci-*` computer appeared.
- **Nothing else on the Dev engine changed:** the list of non-`ci-*` computers (with their state) is identical
  before and after.
- A second silo-runner on the same state is refused. The stale queued job from an earlier attempt shows up as
  `job completed runner="" result=canceled`, harmless.

[run-1.txt](run-1.txt) / [runner-1.log](runner-1.log) is the previous run: 17 pass, 1 fail (the over-broad `eyJ` scan
of every host process matched macOS system extensions such as `WallpaperImageExtension`; replaced by the sampler).

## Result: macOS recipe (`silo-macos`, label `silo-macos-r1`): 18 of 19 checks pass (8 Nov 2026, run 3)

[run-3-macos.txt](run-3-macos.txt), [runner-3-macos.log](runner-3-macos.log), [config-macos.yaml](config-macos.yaml); job pages:
[success](github-job-macos-success.log), [failure](github-job-macos-failure.log). Run with `R1_OS=macos` of the same script, on the
integration build `412e329c` (this branch at `5f3771ac` with the forks work), Dev engine started by hand from the bundle, no macOS
computer running before (limit of two), `maxRunners: 1`. The token was the output of `gh auth token` in `SILO_RUNNER_TOKEN`; a search of
the run output, the log and the config for it, for `eyJ`, `gho_`, `ghp_` and `github_pat` found nothing.

- **Same verifications as Linux, all pass:** the success job ran on `ci-silo-macos-r1-c69eb916` (`hostname`/`$RUNNER_NAME` and `uname -a`
  say `Darwin ci-silo-macos-r1-c69eb916 25.6.0`, `sw_vers` ran); the failing job on `ci-silo-macos-r1-b8d9ee73`, a different
  computer; the success computer was deleted and its runner gone from GitHub; the failure's computer was kept, stopped, its runner
  gone (`ci-silo-macos-r1-b8d9ee73  stopped  macos  4 cpu, 8 GiB`); the sampler saw the `exec --stdin` command 37 times and no
  `eyJ` on any command line; the log has none; the kept computer survived a restart of silo-runner and no other `ci-*` computer
  appeared; a second silo-runner on the same state is refused; at the end 0 runners were registered.
- **Timing:** recipe build 113 s (a rebuild: the build version changed); request to "runner ready" about 42 to 56 s (a macOS fork and
  start); the success computer left `silo list` about 50 s after the job (`cannot delete the computer` once, then `a computer is gone`,
  as in the earlier note on `Stop the computer first`).
- **One check failed, a script artifact:** "no computer other than ci-* changed" compared the list before the run with the list after it,
  and the recipe's own builder (`silo-build-silo-macos-9e3ce756`, made by the script's `recipe build`) is new. The comparison also
  included the header, whose column widths follow the longest name. The script now leaves out both. The fixed script was not run again.
- The first computer of the run, `ci-silo-macos-r1-aab9cf2f`, took a job left queued by the attempt before (it completed at once
  with `result=succeeded`); it was deleted like any other.

### Why the earlier run was blocked, and the fix

The earlier run below failed with `silo-run-jit: No such file`. Cause (found 8 Nov 2026): the recipe's setup ran fine, but the
build **checkpoints the running builder**, and a computer forked from that checkpoint boots from the copied disk without the
builder's memory. A macOS guest still held its last writes in memory: the checkpoint had `silo-run-jit`, written last, missing,
and `/Users/Shared/actions-runner/bin` empty (`ls -la` in a fork showed `bin` with no entries and only `config.sh`, `env.sh`,
`run.sh` and templates), so the runner could not have worked either. Linux checkpoints are consistent by themselves; the capture of a
fork point already runs `/bin/sync` in a running macOS guest first (`capture_held`), but the recipe build did not. A local run of the
recipe's setup script (stubbed download) creates the file, which ruled out the YAML and the `bash -c "$A_CMD"` path.
Fix: a macOS build ends with a `/bin/sync` step ("Writing the disk") and the build version moved to `silo-recipe-build-3`, so
builds made without it are rebuilt. Test: `a_macos_build_writes_the_guest_disk_out_last_and_a_linux_build_does_not` (and one for
the version). Commit `Recipes: a macOS build writes the guest disk out before it saves the build`.

### Earlier run (7 Nov 2026), before the fix: blocked

- `silo recipe build silo-macos` succeeded in 102 s ([macos-build.txt](macos-build.txt)), and a macOS fork from it
  starts in about 36 s: the earlier "did not accept SSH logins in time" did **not** occur here.
- The runner then never comes up: `silo: cannot run /Users/Shared/actions-runner/silo-run-jit: No such file or
  directory` (exit 127), on every fork ([runner-macos.log](runner-macos.log), [macos-try.txt](macos-try.txt):
  the job stayed queued for 30 minutes). In a computer forked from the recipe `/Users/Shared/actions-runner` holds
  GitHub's runner, but no `silo-run-jit`; running the recipe's own setup script by hand with `silo exec ... sh -x`
  creates it ([macos-setup.sh](macos-setup.sh)). So the recipe build reports success without the file; the cause
  was not found (the build ran the setup, the runner was extracted). Not fixed: this is the recipe/engine, not the
  helper. The helper tried four forks with its own backoff (1, 2, 4, 8 min) before the run was stopped.
- [macos-try-0-stale-binary.txt](macos-try-0-stale-binary.txt) is an invalid first attempt: it used a `silo-runner`
  built before the fork fix below.

## Fixes made

1. **`silo-runner` could not read the id from `silo fork <name> --recipe <recipe> --json`.** The engine prints the
   computer itself (`{"id": ..., "name": ..., "source": ..., "state": ...}`); the helper (and its unit-test fake)
   expected `{"forks": [{"id": ...}]}`, so every fork failed with "no computer id in the output", was deleted and
   retried with the growing backoff, and the job waited for over an hour. `silo.go` now reads the flat form and still
   accepts `forks[]`; the CLI test uses the real shape (`go vet`, `go test ./...` pass). Separate commit
   `silo-runner: read the computer id from the flat JSON that silo fork --recipe prints`.
2. **Script:** the workflow upload failed on an empty repository (the 404 body of the sha lookup went into the
   argument list); the "second runner is refused" check was inverted; added the per-job assertions above (runner name
   on the job page, success computer and runner gone, failure kept stopped, nothing else changed, restart reconcile)
   and the `ps` sampler; the success job now also runs `uname -a` and a small C build.

## Observations

- On a macOS fork whose runner did not start, the first delete printed `silo: Stop the computer first.` and the
  helper's retry succeeded about 45 s later (`cannot delete the computer`, then `a computer is gone`).
- The script uses the label `silo-linux-r1`, not `silo-linux`.

## Cleaned up

silo-runner stopped; 0 runners registered; all `ci-*` computers, the `e2e-macos-probe` computer and both recipe
builds (`silo recipe rm`) dropped; the workflow files removed from the repository (the runs stay; the repository
stays, private).

Run 3 (macOS, 8 Nov 2026): silo-runner stopped; 0 runners registered; the `ci-silo-macos-r1-*` computers, the `silo-macos` recipe build and the
test workflows removed from the repository (the runs stay).

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

## Result: macOS recipe (`silo-macos`, label `silo-macos-r1`): blocked

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

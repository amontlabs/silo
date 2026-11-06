# Silo fork and review

A composite GitHub Action that forks a Silo computer on a self-hosted Mac runner, runs a command in the fork, writes the change summary to the job summary, and opens a pull request (or records a review request). It is written as `action.yml` and a POSIX shell script (`run.sh`); there is no build step. It is published as `amontlabs/silo-action`.

The runner needs Silo installed and its engine running (the action checks `silo list` first), and `python3` to render the summary. Forks get none of the credentials Silo manages unless you name them in `secrets`. Files on disk are copied.

## Example

```yaml
name: Silo
on:
  pull_request:

permissions:
  contents: write
  pull-requests: write

jobs:
  agent:
    runs-on: [self-hosted, macOS]
    steps:
      - id: silo
        uses: amontlabs/silo-action@v1
        with:
          recipe: macos-dev
          run: claude -p "fix the failing tests"
          secrets: ANTHROPIC_API_KEY
          fold: pr
      - run: echo "${{ steps.silo.outputs.pr-url }}"
```

## Inputs

| Input | Default | |
| --- | --- | --- |
| `recipe` | | Run `silo up --recipe <recipe>` first, then fork `main`. |
| `from` | `main` | The computer to fork when `recipe` is empty. |
| `run` | required | The command, run with `sh -c` in the fork. |
| `secrets` | | Secret names to give the fork (spaces, commas or newlines). |
| `fold` | `pr` | `pr`: `silo fold --pr`. `request`: `silo fold --request`. `none`: review only. |
| `keep-fork` | `false` | Leave the fork in place after the job. |
| `silo-path` | `silo` | The silo executable. |

Outputs: `fork`, `exit-code`, `pr-url`, `request-id`.

## What it does

1. Checks that `silo` exists and that `silo list` answers.
2. Forks as `gha-<run id>-<attempt>` and runs the command in it (`silo fork <source> --name N [--secret S]... -- sh -c "<run>"`).
3. Runs `silo diff --json` and writes the buckets, and the credentials line when the fork touched credentials, to `$GITHUB_STEP_SUMMARY`. This happens even when the command failed.
4. If the command succeeded, folds: `silo fold --pr` uses the job's `GITHUB_TOKEN` and `SILO_GITHUB_API_URL` (set from `github.api_url`), or `silo fold --request`.
5. Drops the fork unless `keep-fork` is `true`.
6. Fails the job with the command's exit code (or the fold's, if only the fold failed).

`silo fold --pr` exists from phase 4d. If the Silo on your runner lacks it, `fold: pr` fails after the review; use `fold: request` or `none`. The script calls it regardless, and the tests use a fake `silo`.

## Tests

```sh
sh silo-action/test/run.sh
```

The tests put a fake `silo` (`test/silo`, which logs its arguments and prints canned JSON) in front of `run.sh` and check success with a pull request, a failing command, request mode, `keep-fork`, a missing `silo` and a stopped engine. CI runs them in the frontend scripts job.

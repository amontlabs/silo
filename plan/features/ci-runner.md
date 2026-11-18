## Decided

- 2026-11-10: the CI runner is the GitHub-specific case of tasks from outside that start a computer; the general form comes first.

## From the old code

The old code had it built and driven end to end, about 7,600 lines, and it needs reworking.

Lessons:
- GitHub sends no branch or SHA before giving a job, so one label must mean one recipe.
- GitHub's own secrets and GITHUB_TOKEN go straight into the computer; Silo's placeholders do not protect them, so allow private repositories only.
- Count capacity from a fresh list plus what is still forking; on a Mac every macOS VM counts toward Apple's two.
- Runners are refused for public repositories: anyone's pull request could run a workflow on your Mac.
- GitHub delivers its own secrets and GITHUB_TOKEN straight into the job, so Silo's placeholders do not protect them.
- Lifecycle bugs (delete by name, retention before deregister) ended only with one owner of all state and a ledger by id, never by name.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-CI-RUNNER.md`
- `app/SiloUI/runner/scaler.go`
- `docs/forks-ci/evidence/r1-runner/README.md`
- `app/SiloUI/runner/recipes/silo-linux.yaml`
- `docs/forks-ci/BUILD_LOG.md#L97`
- `docs/forks-ci/evidence/r1-runner`
- `docs/forks-ci/PLAN.md`
- `docs/SiloUI-CLI.md#commands`

Calls: `silo-runner run`, `silo fork --recipe`, `silo exec --stream --stdin`, `silo-runner`.

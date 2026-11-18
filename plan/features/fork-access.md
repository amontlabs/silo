## Proposed, not confirmed by the owner

### For whoever builds it

- Property: The simulator checks that a fork made by a computer never holds more access than its maker; a fork the owner made by hand may hold more than its parent.
- Detail: The call that makes a fork can also name a subset of the parent's network rules, beside the secrets it hands over, so each subagent gets a narrower computer.
- The docs say that credential files and signed-in command-line tools on the disk travel into a fork, whatever secrets it is given, until `logins-stay-put` settles it.

## Notes

- The old Linux fork copied secret and GitHub assignments while macOS did not. Name what a fork gets at fork time (`--secret github`) or in settings; GitHub is read-only in forks by default.

## From the old code

The old code had it built and driven end to end, about 400 lines, and its approach can be reused.

Lessons:
- Silo cannot narrow a personal token, so a fork can never inherit one.
- Updating policy after the guest resumes is too late; it must be applied before activation.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/engine/src/policy/access.rs`
- `docs/SiloUI-CHECKPOINTS-PLAN.md#L132`
- `docs/SiloUI-POLICY.md#L176`
- `docs/SiloUI-SECRETS.md#L268`
- `app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs`

Calls: `checkpoints.fork`, `checkpoints.restore`.

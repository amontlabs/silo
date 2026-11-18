## Proposed, not confirmed by the owner

### For the owner to settle

- Does this feature pick a winner (score the forks, or keep the first whose check passes and drop the rest), or only start them and return their results?
  - Recommendation: Only start and return, because the agent that made the forks can score them and drop the losers itself.

### For whoever builds it

- Detail: The one call that starts the forks and runs a command in each also collects every fork's result and returns them together.
- Detail: Each fork of one task can run a different agent program or model.
- Detail: An ambiguous task can go to several forks, each given a different reading of it.
- Rule: The normal case to size for is 3 to 15 concurrent forks per task.
- To find out: How many forks does one Mac hold when each runs an end-to-end suite with Docker and a dev server?
- Test: A large fan-out, as for research or an audit, is tested separately from the normal case.

## Notes

- On macOS, asking for more forks than free macOS slots (two macOS computers running per Mac, by Apple's licence) is refused with a plain reason.

## From the old code

The old code had it built, about 500 lines, and its approach can be reused.

Lessons:
- macOS allows only two macOS VMs running at once; check slots before making forks.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/cli/src/fanout.rs`
- `app/SiloUI/cli/tests/fanout.rs`
- `docs/SiloUI-CLI.md#several-forks-and-commands`

Calls: `silo fork -n`, `silo fork --`.

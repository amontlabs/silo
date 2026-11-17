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

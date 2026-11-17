## From the old code

The old code had it only as an idea and how much can be reused is not known.

Lessons:
- Rebuilding a recipe is equivalent, not identical, because apt, brew and npm install whatever is current.
- A host /nix/store shared between computers was ruled out: it breaks disk-only forks and whole-computer export.

Where it lived, at the tag legacy-2026-11-16:
- `docs/ROADMAP.md#L285`
- `docs/ROADMAP.md#L516`

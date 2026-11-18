## Proposed, not confirmed by the owner

### For the owner to settle

- Are computer images built with Nix and no OCI images, or does the Ubuntu image stay, with Nix only inside recipes?
  - Recommendation: The Ubuntu image stays, because guest-image is in the first stage and exact rebuilds are still only an idea.

### For whoever builds it

- Rule: If recipes rest on Nix, users never need to understand Nix.
- To find out: If recipes rest on Nix, how reliably do the models users run write it?
- To find out: Would devenv 2.0 work as the base for recipes?
- Detail: A computer can build its dependencies from source, pinned, for users who refuse package registries.

## From the old code

The old code had it only as an idea and how much can be reused is not known.

Lessons:
- Rebuilding a recipe is equivalent, not identical, because apt, brew and npm install whatever is current.
- A host /nix/store shared between computers was ruled out: it breaks disk-only forks and whole-computer export.

Where it lived, at the tag legacy-2026-11-16:
- `docs/ROADMAP.md#L285`
- `docs/ROADMAP.md#L516`

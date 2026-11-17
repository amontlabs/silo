## Proposed, not confirmed by the owner

- Each rule states its worst case when it is set, one view shows what a hostile agent could do from a computer, and access is closed by default with guided presets.

## Open points

- Needs `ports` and `network-policy`, which come later in the order.

## From the old code

The old code had it built, about 300 lines, and its approach can be reused.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/cli/src/access.rs`
- `docs/SiloUI-CLI.md#commands`

Calls: `silo access`, `computers.access`.

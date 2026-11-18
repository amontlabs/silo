## Proposed

- Each rule states its worst case when it is set, one view shows what a hostile agent could do from a computer, and access is closed by default with guided presets.

- Each property shows a verdict read at a glance, holds or does not hold, with the detailed worst case one click behind it, from any computer or fork.
- Each statement of the worst case links to the rules it derives from, so the summary can be checked.
- The worst case lists every outbound channel the computer has, messaging included.
- The view shows which credential each agent program in a computer will use, a subscription sign-in or an API key in the environment.
- What an agent will bill to is shown before it starts, because an API key that is present takes precedence without notice.
- The view lists the scheduled jobs of a main that stays on.
- The view says which device the computer runs on.

## Open

- Does the view state what a hostile agent could do from a computer, as a verdict per property, or only list what the computer holds?
  - Suggested: State it, as the notes propose, because `security-framing` says rules exist to make an attack predictable.
- Does this feature offer guided presets, such as a private repository without workflows, package registries only, and no network, or do presets wait for the new entry `rule-presets`?
  - Suggested: Wait for `rule-presets`, since this feature is a view and presets are a way to set rules.
- Needs `ports` and `network-policy`, which come later in the order.

## From the old code

The old code had it built, about 300 lines, and its approach can be reused.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/cli/src/access.rs`
- `docs/SiloUI-CLI.md#commands`

Calls: `silo access`, `computers.access`.

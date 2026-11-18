## Proposed

- The search also finds computers, forks, recipes, changed files and events.
- Results and autocomplete appear as you type, under a shortcut in the style of cmd+K or cmd+P.

## Open

- Does the palette also start work, with "fork: <prompt>" starting a fork in the background, or does it only find and jump?
  - Suggested: Start work too, once starting a task from a prompt exists (one-call-task, or the new entry quick-entry), so that the palette only calls it.

## From the old code

The old code had it built and driven end to end, about 300 lines, and its approach can be reused.

Lessons:
- Schema-valid model output does not mean correct intent or safe execution.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src/features/application/components/application-commands.ts`
- `app/SiloUI/src/features/application/components/application-command-menu.tsx`
- `docs/SiloUI-NATIVE-MENUS.md#L59`
- `docs/SiloUI-JEV-RESEARCH.md`

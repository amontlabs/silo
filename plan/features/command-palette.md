## Proposed, not confirmed by the owner

### For the owner to settle

- Does the palette also start work, with "fork: <prompt>" starting a fork in the background, or does it only find and jump?
  - Recommendation: Start work too, once starting a task from a prompt exists (one-call-task, or the new entry quick-entry), so that the palette only calls it.

### For whoever builds it

- Detail: The search also finds computers, forks, recipes, changed files and events.
- Detail: Results and autocomplete appear as you type, under a shortcut in the style of cmd+K or cmd+P.

## From the old code

The old code had it built and driven end to end, about 300 lines, and its approach can be reused.

Lessons:
- Schema-valid model output does not mean correct intent or safe execution.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src/features/application/components/application-commands.ts`
- `app/SiloUI/src/features/application/components/application-command-menu.tsx`
- `docs/SiloUI-NATIVE-MENUS.md#L59`
- `docs/SiloUI-JEV-RESEARCH.md`

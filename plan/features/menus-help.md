## Proposed, not confirmed by the owner

### For the owner to settle

- Are key bindings vim-style only, or vim-style and ordinary ones both?
  - Recommendation: Both, because a Mac app owes its users the ordinary shortcuts and vim-style keys can sit beside them.

### For whoever builds it

- Property: Everything in the app can be done from the keyboard.
- Rule: No shortcut does something destructive with keys that a neighbouring app uses for something harmless, such as ctrl+W.
- Detail: Shortcuts switch between computers, tabs and panes, split them, hide the sidebar and move a tab to a new window.
- The docs ship as plain files that an agent can read locally.
- Detail: A command or a skill starts a guided session in which the agent in a computer teaches Silo to the user.

## From the old code

The old code had it built and driven end to end, about 1,200 lines, and its approach can be reused.

Lessons:
- Actions start disabled until the window's listener is ready, so an early menu click cannot be lost.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/src/app_menu.rs`
- `app/SiloUI/src/desktop/app-menu.ts`
- `docs/SiloUI-NATIVE-MENUS.md#L28`
- `app/SiloUI/docs/silo-help.html`
- `docs/SiloUI-NATIVE-MENUS.md#L23`

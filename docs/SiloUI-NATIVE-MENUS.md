# Native application menus

Silo's native menus dispatch `silo://menu-command` to the main window. React
routes each request through existing navigation, forms and update controls.
Only the main window can publish command availability. Actions start disabled
until the listener is ready; installation disables navigation and mutation
commands. Individual busy states disable update checks and export or import.
The status panel has its own menu; the application menu belongs to the main window.

Native menus own Command-B/Control-B for the sidebar and Command-K/Control-K for the command palette once connected, avoiding a second DOM shortcut handler. Repeated creation requests preserve unfinished forms. Consumed computer
requests are cleared by their owner so returning to a remounted overview cannot
open another form. Sidebar text follows the existing collapse state.

Category shortcuts follow sidebar order in the Go menu: Command-1 Computers,
2 Files, 3 Logs, 4 Network, 5 Activity, 6 GitHub, 7 Secrets. Linux desktop builds
use Control instead of Command and dispatch through the same guarded app actions.
The File menu has New Computer… (Command-N) and Import Computer…, which opens the
export-file picker; Export… lives in each local computer's own menu.
The shared shortcut badge displays these bindings at the right of expanded
sidebar rows on hover/focus, or inside tooltips when collapsed. Existing toolbar
tooltips include their actual bindings; controls without a shortcut have no badge.

Documentation opens the bundled `docs/silo-help.html`, which covers the current
app rather than the legacy root README. Issues and release notes use fixed
project URLs. Standard Edit, Window and application commands use Tauri's native
predefined items. About reports the built app version.

## Verification

- Full frontend suite: 648 tests passed before the final draft-preservation
  refinements; the final 18 menu-request and 26 backup tests passed afterward.
- Two native action-availability tests passed. TypeScript and lint passed.
- Rebuilt using `npm run desktop:build:debug -- --config
  /private/tmp/silo-local-update-test-version.json`; the version-only override
  leaves the local app at 0.1.0 for the owner's update test. Production updater
  endpoint/key remain unchanged.
- Actual macOS menu clicks: Settings opened General; Check for Updates displayed
  progress then version 0.1.1 available; Create Backup opened computer selection;
  Restore Backup opened the archive picker, which was cancelled without restoring.
  Those two items belonged to the since-removed Backup page. File → Import
  Computer… replaced Restore Backup and Export… moved to each computer's menu;
  this record does not cover them.
- Command-N opened the existing VM form, cancelled without creating a VM.
  Command-K opened and closed one palette. Hide Sidebar collapsed the existing
  sidebar and became Show Sidebar; the sidebar was restored.
- Command-comma is configured natively; the UI driver's comma chord did not
  activate it, so that particular physical-key interaction is not claimed tested.
- Documentation opened the bundled guide in the browser with its current-app
  content. Final bundle build and deep/strict signature verification passed.
- The normal app remains open. No release was installed during these menu tests.

Build logs: `/private/tmp/silo-native-menu-rebuild.log`.
Frontend log: `/private/tmp/silo-menu-frontend-tests.log`.
Native log: `/private/tmp/silo-app-menu-native-tests.log`.

## Primary references

- [Tauri native menus](https://v2.tauri.app/learn/window-menu/)
- [Apple menu conventions](https://developer.apple.com/design/human-interface-guidelines/menus)
- [Apple Settings menu](https://developer.apple.com/documentation/foundation/adding-a-settings-interface-to-your-app)

Shortcut badge verification (2026-09-11): the rebuilt macOS app showed the
GitHub badge at the right of its hovered expanded row and the Secrets badge
inside its collapsed tooltip. Command-6 opened GitHub; Command-B collapsed and
expanded the sidebar. Type checking, lint, focused navigation tests, and bundle
signature verification passed. Linux keyboard dispatch is covered by hook tests;
this check did not run a Linux desktop session.

## Linux menu and update commands

Linux uses a native window menubar, initially hidden, with a visible Menu button
in both application and setup toolbars. Bare Alt and F10 reveal and focus the
menu; Escape restores prior keyboard focus. AltGr and modifier chords do not
activate the menu. The native side owns menu construction, fixed help destinations,
and window/quit actions; the main webview receives guarded navigation commands.
Only the main window is allowed to invoke the reveal command.

The command palette derives update actions from the current updater snapshot.
Checks, downloads, retries, installation, and manual installer links use the
existing updater backend. Commands open General settings so progress and errors
are visible. Installation shares the card's running-computer confirmation,
including cancellation, and never implies permission to stop computers merely because
an update command was selected. Busy or blocked operations are unavailable.

Primary implementation references:

- [Tauri Menu](https://docs.rs/tauri/latest/tauri/menu/struct.Menu.html): Linux menus attach to application windows; macOS menus are global.
- [GTK menu selection](https://docs.gtk.org/gtk3/method.MenuShell.select_first.html): native keyboard menu selection.
- [GTK menubar implementation](https://github.com/GNOME/gtk/blob/gtk-3-24/gtk/gtkmenubar.c): keyboard entry activates the first menu item, establishing normal popup behavior.
- [GTK widget implementation](https://github.com/GNOME/gtk/blob/gtk-3-24/gtk/gtkwidget.c): `can-activate-accel` normally rejects hidden ancestors. Silo overrides this for menu items while retaining sensitivity checks, so hidden menus keep their shortcuts.
- [GTK menu-shell implementation](https://github.com/GNOME/gtk/blob/gtk-3-24/gtk/gtkmenushell.c): `MENU_SHELL_TIMEOUT` suppresses the first release within 500 ms of activation. Outside-click verification must allow that native activation interval to pass.

Earlier verification above describes prior macOS builds. New Linux verification
is recorded separately; it does not establish live computer health.

### Linux verification (2026-09-14)

Tested the rebuilt ARM64 native executable
`/home/polarzero/silo-parity/SiloUI/src-tauri/target/debug/silo-ui` on the
`svvy-e2e` Ubuntu machine with an isolated XDG profile, empty sandbox state,
synthetic GitHub configuration, Xvfb, and the final embedded frontend build.
The visible Menu button and physical F10/Alt keys opened the actual GTK menu.
One Escape restored the previously focused webview input. Arrow keys and Enter
opened Settings; AltGr, Ctrl+Alt, and Alt chords did not reveal the menu.
Physical Ctrl+K opened exactly one palette, and selecting Check for updates
completed against the production release feed. The native update action also
completed. Searching `download update` exposed the manual installer action
without opening an external browser. Physical Menu-button, F10, and Alt opening
followed by outside-click dismissal passed separate pointer probes. The keyboard
probe waited 500 ms after the menu became visible before clicking outside.

The final frontend suite passed 757 tests across 83 files, including palette
search synonyms, pending-install guards, and main-window command permissions.
Five native menu policy tests, TypeScript checking, lint,
the frontend production build, and the Linux native build passed.

Screenshots and automation output are local ignored evidence under
`app/SiloUI/src-tauri/target/verification/native-menu/`, including
`app-final-pointer-separated/` for eight passing core stages,
`pointer-positive-control/` for mouse opening/dismissal, and
`keyboard-outside-control-final/` for F10 and Alt dismissal. Earlier failed runs are
retained, including immediate outside-click failures and harness errors. GTK's
500 ms activation timeout explains the immediate-click behavior; the final
probes wait beyond that native interval.
These checks do not establish live VM health or installer behavior. Release preparation records these checks for Silo 0.4.0.

## Desktop viewer clipboard items

On macOS the Edit menu ends with Paste into Computer and Copy from Computer. They have no accelerator of their own: Command+V and Command+C in a desktop viewer are consumed by a native key monitor scoped to viewer windows (`viewer_shortcuts.rs`), and every other window keeps the standard Cut, Copy and Paste items. The two items are enabled only while a desktop viewer has focus and a device clipboard is available, which is not yet the case, so they stay disabled. They are not part of `MenuState` and send nothing to the main window. Linux viewer windows have no menu bar; Ctrl+Shift+V and Ctrl+Shift+C are handled by a GTK key handler on the viewer window. See [the desktop notes](SiloUI-DESKTOP.md#clipboard-and-the-host-bridge-selkies).

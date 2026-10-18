# SiloUI settings persistence

SiloUI saves preferences and unfinished onboarding input. Layout, navigation,
and existing Save/Cancel/Finish actions stay unchanged. The approved application
menus now show installed choices, their icons, `System default`, and a native
`Choose…` action.

The verification sections preserve dated results from the earlier prototype,
including removed native fixture flags and old bundle names. Use the
[current Dev verification guide](SiloUI-DEPENDENCIES-BACKUP-TESTING.md) for new
checks; those historical commands are not current isolation instructions.

## Login and notification authorization

The Launch Silo at login switch now reports the operating system's effective
state. Loading settings, regaining focus, reopening the status panel, migrating
an old document, or changing another preference never registers or unregisters
Silo. Only an explicit switch action changes OS state, and the UI reads the
result back before displaying or saving it. A saved `launchAtLogin` value is a
record of the last explicit verified action, not authority. External OS changes
therefore appear on focus and relaunch even when the saved value differs.

On macOS 13 and later, the signed app bundle uses
[SMAppService.mainApp](https://developer.apple.com/documentation/servicemanagement/smappservice/mainapp)
and its `status`, `register`, and `unregister` APIs. Apple's
[SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice)
contract requires code signing. Status values remain distinct: enabled,
not registered, requires approval, and not found. Requires approval stays off
and exposes a deliberate Open System Settings action through Apple's
`openSystemSettingsLoginItems`; it never reveals the startup child controls.
Older macOS versions report the feature unavailable without raising the app's
deployment floor. Registration applies to subsequent logins and does not
relaunch the current process. The implementation uses the official SDK header
at `ServiceManagement.framework/Headers/SMAppService.h` through
`objc2-service-management` 0.3.2.

On Linux, the user entry is
`$XDG_CONFIG_HOME/autostart/org.silo.preview.desktop` for production or
`org.silo.dev.desktop` for Silo Dev, falling back to `~/.config/autostart`.
The entry name comes from [the build channel](SiloUI-BUILD-CHANNELS.md). The reader follows `$XDG_CONFIG_HOME` then
`$XDG_CONFIG_DIRS` precedence and evaluates the effective entry rather than file
presence. It honors `Hidden`, `OnlyShowIn`, `NotShowIn`, `TryExec`, executable
availability, and the common `X-GNOME-Autostart-enabled` key. Disabling writes a
user `Hidden=true` override, which also suppresses an enabled system entry.
Writes use the same temporary-file, sync, atomic-replace, and directory-sync
sequence as settings. The generated `Exec` uses `/usr/bin/env --` as a fixed
executable and quotes the Silo executable as one argument, escapes reserved
characters and literal percent signs, and uses the available `APPIMAGE` target
when present. The fixed first token is required because GIO validates the
unexpanded executable before it expands `%%` to a literal percent in the target
path. GLib
[KeyFile](https://docs.gtk.org/glib/struct.KeyFile.html) parses and emits desktop
entries, and GIO resolves their executable. These rules come from the
[Desktop Application Autostart Specification](https://specifications.freedesktop.org/autostart/latest/)
and the [Desktop Entry Exec rules](https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html).

The Tauri autostart plugin is not used as the source of truth. Its documented
[desktop plugin](https://v2.tauri.app/plugin/autostart/) delegates to
`auto-launch`; the referenced 0.5 implementation checks file existence on Linux,
uses a fixed home configuration path, emits an unquoted `Exec`, and uses a
LaunchAgent existence check on macOS. Those checks cannot establish the required
effective OS state.

On macOS, the notification switch reads
`UNUserNotificationCenter.getNotificationSettings`. Every explicit enable first
reads fresh settings instead of trusting rendered state. It requests alert,
sound, and badge authorization only while status is not determined, then reads
settings again. The authorization worker waits for the user's callback without
using the bounded settings-read timeout; the app stays responsive and suppresses
duplicate requests while the system prompt is open. Denial stays off and exposes
Open System Settings. Turning
Silo notifications off changes only the Silo preference; it does not claim to
revoke macOS authorization. This follows Apple's
[notification authorization guidance](https://developer.apple.com/documentation/usernotifications/asking-permission-to-use-notifications)
and the SDK declarations in
`UserNotifications.framework/Headers/UNUserNotificationCenter.h`.

Linux has no standard notification permission prompt. Silo enables its own
preference only when the session owns the `org.freedesktop.Notifications` D-Bus
service. The [Desktop Notifications Specification](https://specifications.freedesktop.org/notification/latest-single/)
defines the service protocol, not a user authorization API; service availability
does not guarantee that a desktop will display every notification. Producing
real computer notification events is described in
[SiloUI-NOTIFICATIONS.md](SiloUI-NOTIFICATIONS.md). Category choices
remain saved and hidden or disabled while the parent preference lacks verified
authorization.

Login and notification IO runs on Tauri blocking workers. Both integrations use
main-window-only commands and narrow capabilities; the status window has no
mutation permission. An OS failure triggers a readback, retains the real state,
and shows one native error dialog for the explicit action. Initial read failures
show one dialog; focus refresh failures only log, so refocusing cannot create a
dialog loop. A verified OS change survives a later settings-save failure and the
dialog reports that save failure without claiming rollback. OS actions never
enter the ordinary optimistic settings retry queue.

## Installed application choices

The Terminal, Code editor, and Browser menus use installed applications in the
native app. Their existing layout stays the same. Each menu adds `Choose…`, which
opens a native file picker at `/Applications` on macOS or `/usr/share/applications`
on Linux. Cancel leaves the selection unchanged. Choosing an application does
not launch it or implement the computer handoff it will eventually configure.

On macOS, [NSWorkspace](https://developer.apple.com/documentation/appkit/nsworkspace)
provides application handlers and default associations. Known terminal IDs keep
source editors out of the terminal list. Known editor IDs supplement registered
source-code handlers; other editors must declare source-editing support, so
generic viewers and media handlers are excluded. Known browser IDs exclude
chat apps, download managers, terminals, and VM helpers that also register HTTPS.
Other browsers remain available through `Choose…`. Private empty `.command`,
`.swift`, and `.rs` files supply existing URLs for the handler queries and are
deleted automatically. The newer enumeration API is guarded; older macOS uses
default handlers and known installed apps without raising the deployment target.
Every result must be an existing application bundle with
an executable. On Linux, [GIO desktop entries](https://docs.gtk.org/gio-unix/class.DesktopAppInfo.html)
provide categories, visibility, executable checks, and browser/editor defaults.
Browser suggestions require the `WebBrowser` category; handling HTTP or HTTPS
alone does not make an application a browser.
The [Desktop Entry specification](https://specifications.freedesktop.org/desktop-entry/latest/recognized-keys.html)
defines `TryExec`, `Exec`, `Categories`, `Hidden`, and `NoDisplay` behavior.

Icons appear beside application names in menu options and selected values, at
16 CSS pixels inside the existing controls. macOS reads the native
[NSWorkspace icon](https://developer.apple.com/documentation/appkit/nsworkspace/icon(forfile:))
and renders a bounded 64×64 PNG on the discovery worker. Linux resolves the
desktop entry's icon through the current
[GTK icon theme](https://docs.gtk.org/gtk3/method.IconTheme.lookup_by_gicon.html)
on the GTK main thread, then decodes and scales it to a 32px PNG with
[GdkPixbuf](https://docs.gtk.org/gdk-pixbuf/ctor.Pixbuf.new_from_stream_at_scale.html).
Missing or unreadable icons use a category symbol. Icons are decorative, never
saved in settings, and never loaded from the device for fixtures.

Lists refresh when a menu opens and when the main window regains focus. An
explicit custom choice joins its selected category while it remains available.
macOS accepts executable `.app` bundles; Linux accepts valid `.desktop` entries
and executable files, including AppImages. A removed saved application stays
saved and appears as a disabled `unavailable` selection.

Each menu starts with `System default (app name)` and the current default's icon.
The selected control shows `app name (default)` to keep the name visible in the
existing width. Application menus fit their labels within the available screen
width. If no default is reported, that option says `System default
(not set)` and is disabled. Both platforms include a valid current browser
default even when it is outside the suggested categories or curated list;
merely registering HTTPS does not add other helper apps. Linux has no standard
terminal-default association in GIO.

The existing `terminal`, `editor`, and `browser` labels remain readable. New
`terminalPath`, `editorPath`, and `browserPath` fields save the selected location
alongside its label in the same atomic settings change. Missing/null paths mean
an older label-only choice. Bundle filenames also match legacy names such as
`iTerm` versus bundle metadata `iTerm2`; loading never rewrites a saved choice.
Discovered defaults apply only when the corresponding setting is absent, and
retain exact paths so identical application names do not select another copy.
The boolean fields `terminalUseSystemDefault`, `editorUseSystemDefault`, and
`browserUseSystemDefault` record whether to follow those defaults. A saved label
or path without the flag remains an explicit choice. With neither a saved choice
nor a flag, the control follows the system. Switching to system mode saves only
the flag and retains the old label/path; resolved labels and paths follow the
current catalog in both windows. Selecting an app saves its label/path together
with a false flag. Refreshing discovery never writes settings.

Both production webviews initialize application discovery before rendering and
refresh it on window focus. Resolved defaults live in each webview's settings
store; `settings:changed` carries saved choices, not the discovered catalog.
Skipping discovery in the status entry therefore leaves its fallback labels
(for example, Visual Studio Code) even when the main window resolves Zed.
The shared resolved preferences feed the main window, command palette, status
shortcuts, native computer menu, and folder-picker label.

Regression verification (2026-09-09): `application-startup.test.tsx` imports the
actual production entry as the status window. It reproduced the VS Code fallback
before the fix and now checks Zed/Zen discovery, editor refresh on focus, retained
terminal overrides, and no settings writes. The focused application/preferences
suite passed 170 tests; status/settings/onboarding follow-up checks passed 33
tests, including native menu and open folder-picker updates. Typecheck and lint
passed. These are mocked discovery and UI checks, not native app launch evidence.
That run verified preference propagation. The current native
[`computer_action` handler](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs) dispatches
`open-editor` and `open-terminal` to their launchers; website actions use
[`open_network_port`](../app/SiloUI/src-tauri/engine/src/network.rs), which checks the
endpoint before invoking the chosen browser. See
[editor and browser handoff](SiloUI-EDITOR-HANDOFF.md) and
[terminal handoff](SiloUI-TERMINAL-HANDOFF.md) for implementation and verification.

Browser previews and tests supply a fixed catalog. The native application uses
device discovery and the native picker. Both native windows read the catalog to resolve the same
installed defaults; only the main window can open the application picker.
The picker uses the official [Tauri dialog plugin](https://v2.tauri.app/plugin/dialog/)
behind those commands; the frontend receives no general filesystem permission.

### Application-choice verification

Completed on macOS arm64, 2026-09-07. `npm test` plus focused reruns passed all
335 frontend tests; `npm run typecheck`, `npm run lint`,
`cargo test --offline --manifest-path app/SiloUI/src-tauri/Cargo.toml` (33 tests),
and `npm run desktop:build -- --debug --bundles app` passed. The desktop build
retains Vite's existing chunk-size advisory. Focused tests cover category
filtering, available/custom paths, legacy labels, cancellation, icon conversion,
unavailable apps, default-mode persistence, and both windows following changed
defaults without writes. Linux classifier checks passed using an extracted
Rust test harness; full Linux GIO/GTK compilation, icon tests, and GUI checks
remain unverified on this Mac.

Native checks used the bundled `Silo Preview.app` with an isolated
`SILO_SETTINGS_DIR`, without an explicit fixture selector. Accessibility and
screenshots confirmed:

- Terminal choices: `System default (Terminal)`, Ghostty, iTerm2, Terminal, Warp,
  and `Choose…`, with native icons.
- Code editor choices: Cursor, Xcode, and Zed with their icons. Browser choices:
  Arc, Brave Browser, Dia, Google Chrome, Safari, Tor Browser, and Zen. Chat,
  terminal, download, and VM helper apps were absent from browser suggestions.
- `Choose…` opened the `open-panel` sheet with `Where: Applications` and title
  `Choose a terminal`. Selecting `/Applications/Ghostty.app` updated the Terminal
  control and icon; the label and exact path were present in JSON before Finish.
- Choosing Cursor from the editor menu saved its label/path. Both Ghostty and
  Cursor survived Quit and relaunch during unfinished onboarding.
- Selecting `System default (Terminal)` displayed `Terminal (default)` and
  saved `terminalUseSystemDefault: true` while retaining Ghostty's saved
  label/path. After another Quit/relaunch, Terminal remained in default mode,
  Cursor remained explicit, and the untouched Browser showed `Zen (default)`.
- Reopening `Choose…` and pressing `Cancel` retained default mode. The final
  menu displayed the full system-default name. The closed control dimensions
  and surrounding onboarding layout remained unchanged.

The final test instance quit through `Quit Silo Preview`; a process check found
no remaining Silo instance. Native stdout/stderr logs were empty. Local logs and
the isolated settings snapshot are under the ignored
`app/SiloUI/src-tauri/target/applications-verification/` directory. No host default
associations or production settings were changed. Native status-panel visuals
were not reverified; shared default resolution is covered by main/status tests.
Workspace and integration states remain deterministic scaffold values.

To repeat the focused UI check, build and launch with a fresh isolated settings
directory as described below, then open Dependencies > Applications. Inspect all
three lists and icons, choose and cancel a native application, select a named
app, quit before Finish, and relaunch using the same directory. Switch to System
default and repeat; confirm the current default name and retained explicit
choice in the JSON. On Linux, also exercise `.desktop`, executable, and AppImage
choices, GTK theme icons, and the disabled default option where no association
exists.

## Saved values

| JSON field | Existing control | Default when absent |
| --- | --- | --- |
| `theme` | Theme | System |
| `launchAtLogin` | Last explicitly verified Launch Silo at login result | On; never applied to the OS automatically |
| `startComputersAtLaunch` | Start computers at launch | Off |
| `startupComputerIds` | Startup computers | Existing initial `dev` selection, otherwise first computer |
| `terminal` | Terminal | Terminal |
| `editor` | Code editor | Visual Studio Code |
| `browser` | Browser | Safari |
| `reduceMotion` | Reduce motion | Off |
| `notificationsEnabled` | Silo notification preference | On; effective only with current OS/service authorization |
| `notifyFailures` | Failures: something you started, or background work, failed | On |
| `notifyChanges` | Changes: a computer changed state without a Silo operation causing it | On |
| `notifyCompletions` | Completions: work that took 3 seconds or longer finished | On |

The legacy `notifyHealth` (read as Changes) and `notifyActions` / `notifyBackup`
(both must be on for Failures) are read only as a fallback until the new keys are
saved. See [SiloUI-NOTIFICATIONS.md](SiloUI-NOTIFICATIONS.md).

Defaults are applied in TypeScript and are not written on mount. The startup
default follows the current computer list until the user enables startup or saves
a selection. Enabling startup saves the displayed IDs and the switch value in
one preference patch, both in General settings and on the setup completion page. An explicit
`false` or empty startup selection overrides its default. Startup selections use
stable computer IDs; temporarily missing computers do not delete saved IDs.

Older settings can contain `startComputersAtLaunch: true` without
`startupComputerIds`: the switch used to save only its boolean while the UI
displayed an unsaved default. Native startup resolves that missing field from
the saved local computer configuration (`dev`, otherwise the first local computer). It does
this only after onboarding is complete and before excluding recovered explicit
stops. An explicit empty array still starts nothing; remote computers
are never inferred as launch targets. This compatibility default does not rewrite
settings on startup.

The 2026-09-22 investigation confirmed the missing-field state in local saved
preferences. The regression tests in
[`startup.rs`](../app/SiloUI/src-tauri/engine/src/startup.rs),
[`general-page.test.tsx`](../app/SiloUI/src/features/application/pages/general-page.test.tsx),
and [`setup-complete.test.tsx`](../app/SiloUI/src/features/onboarding/components/setup-complete.test.tsx)
cover native default resolution and persistence when enabling the setting.

Verification used live preferences only for read-only diagnosis. The running
executable was `app/SiloUI/src-tauri/target/release/bundle/macos/Silo.app/Contents/MacOS/silo-ui`
under the repository root. This investigation did not rebuild or relaunch that
bundle or exercise startup against live VMs. Behavioral checks used temporary
computer configuration files, a stub start callback, and in-memory frontend
settings backends:

- `npm --prefix app/SiloUI test -- src/features/application/pages/general-page.test.tsx src/features/onboarding/components/setup-complete.test.tsx`: all 9 component tests passed after both new default-selection cases failed before the fix.
- `npm --prefix app/SiloUI test -- src/features/application/application-settings.test.tsx --testTimeout 20000`: all 7 passed; the initial concurrent run hit the default 5-second limit in one existing interaction test.
- `npm --prefix app/SiloUI run typecheck` and `npm --prefix app/SiloUI run lint`: passed.
- `cargo +1.94.0 test --manifest-path app/SiloUI/src-tauri/Cargo.toml --bin silo-ui startup::tests`: all 7 passed after the missing-selection regression failed before the fix. Native runs used explicit synthetic GitHub build configuration as described in the release guide.
- `cargo +1.94.0 test --manifest-path app/SiloUI/src-tauri/Cargo.toml -- --test-threads=1`: 473 passed, 12 ignored outside the execution sandbox. The restricted parallel run hit socket/process permissions; an unrestricted parallel run left two storage lock failures. All 16 non-live storage tests passed separately, and the full serial run passed. Opt-in live tests remained disabled.

K-18 subsequently qualified ten consecutive full runs with the default parallel
thread count. The historical serial result above remains a record of that earlier
verification. Current checks use `cargo test --locked`; see
[native test support](SiloUI-RUST-TEST-SUPPORT.md) for isolation and timings.

The existing valid `silo-theme` browser value is imported only when the native
document lacks a theme, and its original key is retained.

The document is `settings.json` under Tauri's `app_config_dir`, keyed by the
[build channel](SiloUI-BUILD-CHANNELS.md): `org.silo.preview` for production
and `org.silo.dev` for Silo Dev. Production uses
`~/Library/Application Support/org.silo.preview` on macOS and
`$XDG_CONFIG_HOME/org.silo.preview` (normally `~/.config/org.silo.preview`) on
Linux. Dev uses the corresponding `org.silo.dev` directory. See the official [Tauri path reference](https://docs.rs/tauri/latest/tauri/path/struct.PathResolver.html#method.app_config_dir).

```json
{
  "schemaVersion": 1,
  "settings": {},
  "onboardingDraft": null
}
```

`settings` contains explicit changes, not a complete default snapshot.
Unknown document and preference fields survive later writes. Revision numbers
and save errors belong to the running session and are not written to disk.

An onboarding draft contains the current step, saved computer configurations,
unfinished computer editor, repository selections and push choices, and Git
identity input. Edits save immediately, including incomplete text. Application
choices use shared preferences. Save still validates configuration; Cancel
discards the current editor draft; successful completion clears recovery data.
Drafts never claim that installation, authentication, or integration work has
completed. Credentials and operation results are not accepted draft fields.

## Shared state and failures

The main window initializes native storage before rendering the application.
The [native settings owner](../app/SiloUI/src-tauri/engine/src/settings.rs) always
resolves `settings.json` through Tauri's app configuration directory. Status
reads wait for initialization on a worker thread.

One Rust mutex serializes individual preference patches and complete onboarding
draft updates. Both windows subscribe before reading; numbered snapshots prevent
old replies from replacing newer state. Focus and status-panel opening refresh
the snapshot. React uses [useSyncExternalStore](https://react.dev/reference/react/useSyncExternalStore).
This follows Tauri's [managed-state](https://v2.tauri.app/develop/state-management/)
and [event](https://v2.tauri.app/develop/calling-frontend/#event-system) patterns.
Main-only mutation permissions and caller checks protect writes; status snapshots
omit onboarding data. See [Tauri capabilities](https://v2.tauri.app/security/capabilities/).

No new frontend state library is needed. [TanStack Store](https://tanstack.com/store/latest/docs/overview)
provides reactive state; native disk writes and cross-window delivery still need
the Tauri boundary. [Tauri Store](https://v2.tauri.app/plugin/store/) supports
saved key/value settings, but this implementation needs explicit atomic writes,
protected invalid files, and a Quit flush. A small Rust owner and React's existing
subscription API keep those rules in one place.

Writes create a private temporary file in the destination directory, write and
sync it, atomically replace the destination, and sync the directory.
[`tempfile::NamedTempFile::persist`](https://docs.rs/tempfile/latest/tempfile/struct.NamedTempFile.html#method.persist)
provides replacement; explicit sync calls provide the durability steps.
Failures before replacement preserve the old file. A directory-sync failure
after replacement leaves a complete new file but reports uncertain durability.

Ordinary session edits remain usable and synchronized after a save failure. The next
change or flush retries them. Errors are returned in state and logged, with no
new dialog or banner. The system-integration error dialogs described above are
the narrow exception. Malformed, unreadable, unsupported-version, or invalid
saved documents are protected from all writes for that session; valid known
preferences can still be displayed. Drafts must match the existing computer
configuration contracts and allow unfinished editor input. Documents are capped at 1 MiB,
drafts at 256 KiB, and startup selections at 256 IDs.

All native Quit paths intercept Tauri's
[`ExitRequested`](https://docs.rs/tauri/latest/tauri/enum.RunEvent.html), request a
main-window flush, wait for queued edits, and then exit. The frontend acknowledges
before draining its queue. A two-second fallback applies only when no
acknowledgment arrives; an acknowledged flush can take as long as needed. A
frontend that crashes after acknowledging can require force-quit. A crash cannot
save input that has not reached Rust. An unfinished temporary file is never
loaded as the settings document.

## Fixture isolation

Browser previews and tests use independent memory stores. Native unit tests
construct stores with temporary paths or no path; they do not use the user's
settings. The [application entry point](../app/SiloUI/src/main.tsx) uses native
production adapters and has no fixture-query selection. `SILO_SETTINGS_MEMORY`
and `SILO_SETTINGS_DIR` no longer select native storage modes.

For UI checks use deterministic frontend fixtures. For native checks use the
Dev channel and disposable state; follow [the current verification guide](SiloUI-DEPENDENCIES-BACKUP-TESTING.md#build-and-launch).
Do not use the removed prototype flags as an isolation boundary.

### Real completed-onboarding verification

Historical prototype verification, September 2026. The fixture progress,
presentation switch, native storage overrides and `Silo Preview.app` commands
below were removed. They record the original run and are not current launch
instructions. The current setup adapter waits for real computer and GitHub
acknowledgements; see [setup verification](SiloUI-DEPENDENCIES-BACKUP-TESTING.md#continue-and-the-setup-queue).

On a normal launch, onboarding uses completed computer fixture progress, so
Review → Finish opens the permission switches without an environment override.
Explicit loading and failure scenarios remain selectable for UI checks. This
default changes only onboarding progress; native settings and OS permissions
remain real. Computer readiness is still scaffold data, not live verification.

A debug-only presentation switch opens the real signed bundle directly on the
completed onboarding panel while retaining real settings and system-integration
adapters. It does not set a fixture query parameter and is ignored in fixture or
`SILO_SETTINGS_MEMORY=1` mode. The debug build script supplies an ad-hoc signing
identity without changing release-signing configuration. Build and verify the
complete app bundle, then run from the repository root:

```sh
npm --prefix app/SiloUI run desktop:build:debug
codesign --verify --deep --strict \
  'app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Preview.app'
otool -l \
  'app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Preview.app/Contents/MacOS/silo-preview' \
  | grep -B2 UserNotifications
SILO_SETTINGS_TEST_DIR=$(mktemp -d /tmp/silo-system-ui.XXXXXX)
open -n 'app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Preview.app' \
  --env "SILO_SETTINGS_DIR=$SILO_SETTINGS_TEST_DIR" \
  --env "SILO_NATIVE_ONBOARDING_COMPLETE=1"
```

Use the two switches on the Stay informed card. For login, confirm the switch
and startup children appear only after macOS reports enabled; if approval is
required, use the presented System Settings action, approve Silo, then refocus
the app. For notifications, accept or deny the first macOS prompt and confirm
the parent switch and category rows match the resulting authorization. Disable
notifications and confirm macOS authorization remains unchanged. Change each
state externally in System Settings and refocus Silo to confirm readback. Quit
Silo Preview, relaunch with the same two environment variables, and confirm OS
state wins over the isolated saved flags. Do not use `scenario=complete` for this
check because that selector intentionally activates fixture isolation.

## Verification

The results and manual procedures below are historical records. They include
removed native fixture modes and old bundle names. Use [current Dev verification
instructions](SiloUI-DEPENDENCIES-BACKUP-TESTING.md) for a new run; these records
do not establish behavior at current HEAD.

Completed on macOS arm64, 2026-09-07. Frontend commands run from `app/SiloUI`:

```sh
npm test
npm run typecheck
npm run lint
npm run desktop:build:debug
```

All passed: 357 frontend tests, type checking, lint, and the macOS debug bundle.
The desktop build also runs the TypeScript and Vite production build. Tests use
four Vitest workers: unrestricted workers caused CPU contention and five-second
test timeouts on this host. Test timeouts and assertions were retained.

Native commands run from the repository root:

```sh
cargo test --offline --manifest-path app/SiloUI/src-tauri/Cargo.toml
cargo build --offline --manifest-path app/SiloUI/src-tauri/Cargo.toml
git diff --check -- app/SiloUI/src-tauri
```

All passed on macOS: 37 Rust tests. Settings coverage includes every field, false/empty selections,
legacy import, unknown fields, invalid saved computers, corrupt/future documents,
failed-write retry, incomplete temporary files, concurrent patches, onboarding
recovery, fixture isolation, status initialization ordering, and both Quit
acknowledgment paths. Focused integration coverage includes verified on/off,
approval-required, external disable, denied notifications, saved/default flags
never invoking OS mutation, failed OS intents never entering settings retries,
verified OS results surviving save errors, fixture authority, main-window guards,
and initial-error dialog suppression on later refresh. Focused frontend tests
also cover stale focus-read invalidation, a fresh notification read on every
explicit enable, failed-read unknown authority, and runtime adapter selection for
native fixture and memory storage. Linux-only tests cover XDG precedence, Hidden
and `X-GNOME-Autostart-enabled=false` overrides, desktop restrictions,
unavailable TryExec, a GIO round-trip and harmless launch through a path containing
spaces, percent, quotes, backslash, dollar, and backtick characters, AppImage
selection, and persistence errors. Final Linux verification passed all 43 native
tests in a disposable Debian bookworm container with Rust 1.91.1, GTK 3, and
WebKitGTK 4.1. The repository was mounted read-only and copied into the container
for compilation. The test binary ran as ordinary UID 1000, because root bypasses
the existing filesystem-permission failure test. GIO returns quoted executable
strings on this GLib version; the state check parses the full command with
`glib::shell_parse_argv` before checking the executable. This parses arguments
without executing a shell. Build and test logs are retained under
`app/SiloUI/src-tauri/target/settings-verification/`.

`codesign --verify --deep --strict` passed for the generated bundle. Its identity
is `org.silo.preview`, its signature is ad hoc with sealed resources, and its
`LSMinimumSystemVersion` remains 10.13. `otool -l` reports
`LC_LOAD_WEAK_DYLIB` for UserNotifications, while runtime calls remain guarded to
macOS 10.14 or newer. This prevents the 10.13 deployment floor from gaining a
strong load-time dependency on the newer framework.

General, Notifications, and onboarding browser screenshots at 1160×820 were
compared with committed HEAD: zero changed pixels on all three surfaces. Tests
also exercise all 12 preferences, retained values after source replacement and
remount, native event ordering, missed-event refresh, Quit queue draining,
unfinished computer input, Git input, and completion/Cancel behavior.

The macOS bundle used for native checks was
`app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Preview.app`. It ran with
`SILO_SETTINGS_DIR` pointing to a new temporary test directory, without explicit
scenario selectors. Observed native controls after Quit and relaunch, and again
after intentional SIGTERM and relaunch:

- `GitHub` remained the selected setup tab.
- `Git name for dev` retained `Recovery Check`.
- `Allow pushes for acme/silo` remained checked.
- On `Dependencies`, `Terminal` retained `iTerm` and `Code editor` retained `Cursor`.

The JSON file contained edits before Finish. Native Quit exited cleanly; a
process check confirmed no remaining `silo-preview` instance after cleanup.
The native launch logs were empty. Local screenshots and logs are kept in the
ignored `app/SiloUI/src-tauri/target/settings-verification/` directory.

For this integration change, `npm run desktop:build -- --debug --bundles app`
created the same arm64 bundle. The complete bundle was then ad-hoc signed for
local SMAppService verification. `codesign --verify --deep --strict` passed, the signed identifier is
`org.silo.preview`, and the existing minimum system version remains 10.13.
The executable links Apple's ServiceManagement and UserNotifications frameworks.
The signed bundle was opened with `SILO_NATIVE_ONBOARDING_COMPLETE=1`, using the
normal persisted settings and real OS adapters. Native accessibility checks
observed both parent switches off on entry. Enabling `Launch Silo at login`
returned a checked switch and revealed `Startup preferences`, the saved
`Start computers at launch` choice, and the saved computer selection. Disabling
login registration returned an unchecked switch and hid those children. The
original off state was restored; child preferences were not edited.

Clicking `Enable notifications` entered the pending state: unchecked and disabled,
with notification categories hidden. Grant/denial verification is still manual:
computer-use access to Apple's `UserNotificationCenter` authorization-dialog app
is blocked. The app was left open for the user to answer the OS request. No
logout/login test was performed. The completed computer presentation remains a
scaffold; these checks do not prove computer startup or live VM state.

The complete native checklist below is **not** claimed as passed. Accessing the
macOS status item through SystemUIServer timed out in automation; native panel
appearance, folder picker, focus, Escape, and outside-click dismissal remain
unverified for this change. The then-default running setup fixture blocked computer
editing, so native persistence checks used editable Git inputs; computer-editor
recovery is covered by component and native storage tests.

For repeatable manual checks, build first, then launch from the repository root:

```sh
SILO_SETTINGS_TEST_DIR=$(mktemp -d /tmp/silo-settings-ui.XXXXXX)
open -n 'app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Preview.app' \
  --env "SILO_SETTINGS_DIR=$SILO_SETTINGS_TEST_DIR"
```

1. Capture General, Notifications, onboarding application choices, and the native
   status panel at the same dimensions as the baseline.
2. In General change Theme, both startup switches, startup selection, Terminal,
   Code editor, Browser, and Reduce motion. In Notifications change the master
   switch and each category. Navigate away and back; verify each value.
3. Clear Startup computers, disable/re-enable Start computers at launch, and
   confirm the empty selection survives. Disable/re-enable notifications and
   confirm category choices survive.
4. Open the native panel. Verify matching appearance, motion, and application
   labels. Exercise its folder picker, keyboard focus, nested-menu Escape, and
   outside-click dismissal.
5. Change onboarding application preferences; verify General and the panel agree.
   Leave repository selections, push choices, and Git identity input. Quit without
   Finish, relaunch with the same test directory, and verify recovery. Separately,
   in the complete browser fixture (`?view=onboarding&scenario=complete`), exercise
   computer editing, Cancel, and Finish. Do not treat a fixture reload as a disk
   recovery check: explicit fixtures deliberately use memory.
6. Use Quit Silo immediately after another edit, confirm process exit, relaunch
   with the same directory, and verify all preference values. Repeat System,
   Light, and Dark appearance checks and compare baseline screenshots.
7. Relaunch in memory/explicit fixture mode; confirm saved production/test values
   do not appear or change. Repeat the native sequence on Linux when a desktop
   session is available.

Linux compilation and all 43 native tests passed in the disposable Docker
environment described above. A Linux desktop session, notification delivery, and
actual launch after logout/login remain manual checks; the container tests do
not establish those behaviors.

# Compact app rows

Use `ListCard`, `ListRow`, and `ListRowIcon` from `src/components/list-row.tsx`
for records and settings with a left icon, title, caption, and right action or
metadata. Use `ListRowDetails` for expanded review, progress, or result content.

Let titles and captions inherit their type size from `ListRow`: 13px medium
titles and 11px captions, both with 16px line height. The icon tile is 28px;
text buttons use `size="xs"`. Keep semantic headings, list roles, status
announcements, and useful truncation or wrapping at the call site. Badges,
timestamps, and progress percentages can use smaller metadata text.

When changing this pattern, inspect every consumer and its running, failure,
and expanded states. Avoid adding page-specific title or caption sizes.

## Sandbox help

Bundled help in `docs/silo-help.html` follows the sandbox page: Overview,
Checkpoints, Storage, and SSH access, with unsupported tabs hidden. Explain
settings, secrets, and ports where users find them. Describe Duplicate settings
as a new empty sandbox with the same settings, and Fork as a new sandbox with
a copy of its files. Use Export and Import for export files and Restore only
for checkpoints. Check every emphasized control name against its visible label
or accessible name when updating help. Document available controls; logs load
more records by scrolling, and Details appears only where diagnostics exist.

Keep development examples aligned with the `/preview.html` fixture entry
and its `view` and `scenario` parameters (A-27/K-22).

## App surface audit

| Surface | Shared rows |
| --- | --- |
| Sandboxes | Sandbox and SSH host records through `SandboxListRow`; configuration status uses the same icon tile. |
| Files | Repository records; push controls and feedback remain below the header. |
| Activity | Every event, including progress, success, warning, and failure. |
| GitHub | Connected, connecting, and disconnected account cards. |
| Secrets | Secret records, sandbox badges, and restart notices. |
| Sandbox page | Checkpoint history and its empty state on the Checkpoints tab, and the Overview tab's secret and port rows. Export and import progress and results are notifications, not rows. |
| General | Startup, polling, application preferences, accessibility settings, and the Storage row for the pre-upgrade backup (present only while one exists). |
| Notifications | Main toggle and alert categories. |
| System issue | Repair header and expanded details; ordered repair steps share the icon tile. |
| Status bar | Sandbox rows, status labels, secret-change tooltips, repair row, and inline lifecycle confirmation use the same app components. |

## Specialized layouts reviewed

Logs, Network, and GitHub repository permissions need aligned table columns.
The file tree needs hierarchy and indentation. Navigation and disclosure
headings are single-line controls. Configuration editors are forms. Empty
states are centered explanations using `EmptyState` from
`src/components/empty-state.tsx`. Inline operation feedback and temporary
repair notices remain compact status messages within their owning surface.
These do not need a two-line record card.

Onboarding uses the same compact account and application preference rows.

## Screen transitions

Keep transitions limited to their intended properties. Progress indicators animate
their transform; buttons and tabs use the standard visual transition properties.
Never include `all` or `visibility`: onboarding retains hidden panels to preserve
drafts, expanded details, and scroll positions. A descendant that animates inherited
visibility can remain painted after its panel is hidden, as described in
[MDN’s visibility interpolation](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Properties/visibility#interpolation).

`src/test/transition-styles.test.ts` builds every stylesheet that application
modules import through the production Vite and Tailwind pipeline, without
bundling the application, and checks the compiled CSS for this rule, including
shared components, variants, and custom styles.

## Tooltips

Use `restoreFocus` when dismissing a surface returns focus to its trigger.
The shared tooltip trigger suppresses only that synchronous focus event; later
hover and keyboard focus still open it. This uses
[Radix's focus handler](https://github.com/radix-ui/primitives/blob/main/packages/react/tooltip/src/tooltip.tsx),
which skips opening when the supplied handler prevents the event's default.

Pass the surface's reduced-motion preference to its outer `TooltipProvider`.
Nested providers inherit it, and tooltip content receives it through
[React context across portals](https://react.dev/reference/react-dom/createPortal).
Tooltips disable entry and exit animations for either the app preference or
the system's `prefers-reduced-motion` setting. Popovers, selects and menus also
render in portals outside `.silo-window`; they read the same context through
`components/ui/reduce-motion.ts`, carry the `silo-portal` class and
`data-reduce-motion`, and `index.css` turns off their animations and transitions.

## Fixture preview

`npm --prefix app/SiloUI run dev` serves the browser preview. `/` is the
production entry and only shows "Open Silo in the desktop app." outside the
desktop app. Open `/preview.html` instead: it renders deterministic fixtures,
never Silo services, and is a development-only entry that `vite build` does not
bundle (the build input is `index.html` alone). URL parameters choose the
surface and its fixtures; there is no on-page selector, so edit the URL.

| Parameter | Values | Source |
| --- | --- | --- |
| `view` | `onboarding` (default), `app`, `status-bar`, `desktop`, `migration` (the screen shown after a migration that kept a pre-upgrade backup, then the app) | `src/fixtures/surfaces.ts` |
| `scenario` | `running`, `complete`, `dependency-failure`, `bootstrap-failure`, `stress-running`; the app and status bar default to `running`, onboarding to `complete` | `src/fixtures/scenarios.ts` |
| `github` | `disconnected`, `connecting`, `connected` | `src/fixtures/scenarios.ts` |
| `status-bar` | `stale`, `empty`, `long-list` | `src/fixtures/status-bar-scenarios.ts` |
| `sandbox-state`, `sandbox-change`, `system-issue`, `repository-push`, `github-operation` | See each `…FixtureModes` list | `src/fixtures/application-scenarios.ts` |
| `activity` | See `activityFixtureModes` | `src/fixtures/application-activity.ts` |
| `backup-operation` | See `backupFixtureModes` | `src/fixtures/application-backup.ts` |
| `pre-upgrade-backup` | `present`, `no-date`, `delete-fails`, `read-fails`; `view=migration` defaults to `present`, other views show none | `src/fixtures/pre-upgrade-backup.ts` |
| `unseen-result` | `interrupted-import`, `interrupted-export`, `set-aside`: an export or import result Silo has not shown yet (see below) | `src/fixtures/transfer-result-notice.ts` |
| `resource-notice` | `create-storage`, `start-memory` | `src/fixtures/application-resources.ts` |
| `operations` | `running`, `stuck` | `src/fixtures/operation-queue.ts` |
| `computer-use` | Built-in computer use: `unavailable`, `installing`, `ready`, `failed`, `auto`, `unknown-approval`, `approval-pending`, `approval-failed`, `approval-partial`, `pre-v4` (the approval ones show the chosen mode, "Applying…" or the failed/partial result with the warning that some agents may still act without asking; the switch applies like the native one, pending then applied). Enables the Computer use section (sandbox page, `dev`) and the `view=desktop` header. There is no consent notice anywhere | `src/fixtures/computer-use.ts` |
| `approvals` | `none`, `waiting`: what the agents inside computers ask for. `waiting` (the default in the browser preview) lists several kinds of requests waiting, some answered and failed, a merge with conflicts, a fast-forward merge, and audit rows under Activity, Who did what | `src/fixtures/approvals.ts` |
| `agents` | `mixed` (default in the browser preview: Claude Code ready, Codex not installed), `ready`, `missing`, `outdated`, `installing`, `unknown`, `install-fails`: the agents of every computer. Installing walks through its steps in about a second and a half; `install-fails` fails after it starts. Also drives the Your agent step of `view=onboarding` (choose one, Continue, and the last screen shows the install and Retry) and the agent actions of `view=status-bar` | `src/fixtures/agents.ts` |
| `appearance` | `light`, `dark`; otherwise the fixture's theme preference | `src/fixtures/preview.tsx` |

The app view also lists the macOS fixture computers (`src/fixtures/macos-computers.ts`) beside the Linux
ones, whatever the scenario: `daily` is running, `agent-box` is being set up, `xcode-build` is stopped with
checkpoints. Each opens the same computer page as a Linux computer and appears on the Files, Logs, Network,
Secrets and GitHub pages.

For example, `/preview.html?view=status-bar&status-bar=long-list` or
`/preview.html?view=app&scenario=complete&system-issue=needed`. Opening Silo
from finished onboarding or choosing **Open Silo** in the status bar switches
the URL to `view=app`. `glass.html` is a separate material study
(`docs/SiloUI-GLASS-STUDY.md`) that always shows the complete app scenario.

### Unseen export and import results

An export or import result present when Silo opens is from an earlier session and
stays silent, except one the runtime marks unseen (`BackupState.resultUnseen`): a
result an upgrade produced for an operation it interrupted, or the notice that an
unreadable export or import record was set aside. After an upgrade the screen about
the pre-upgrade backup shows it as a callout beside the backup (title, then message
and detail), and **Open Silo** acknowledges it before the application opens, so
the application does not show it again. Wherever that screen does not appear (no
backup, or its notice was already shown), the application shows the result as an
ordinary export and import notification that stays until it is dismissed, which is
also what acknowledges it there: the window may be hidden at launch, so showing it
acknowledges nothing.

`/preview.html?view=migration&unseen-result=interrupted-import` shows the callout
on the post-upgrade screen, and **Open Silo** then opens the application without
the notification. `/preview.html?view=app&unseen-result=set-aside` opens the
application with the set-aside notice as a notification, as when no backup notice is
pending. `interrupted-export` is the export wording. The parameter applies to
`view=app` and `view=migration`.

## Status bar preview

Open `/preview.html?view=status-bar`. The `scenario` and `system-issue`
parameters reuse the app's fixture snapshots; `status-bar` adds stale status,
an empty list, or a long list. Repair and error notices and the footer remain
visible while only the sandbox list scrolls.

Repositories with outgoing commits add one compact line under their sandbox,
with the repository's full path (hidden characters revealed) and a **Push N commits** action. Progress and the brief
success message reuse the Files page's push feedback. Failures remain pinned
above the list with Details and Retry. Push actions require a fresh, running
sandbox; the preview simulates completion and carries updated counts into Silo.

The menu bar keeps the Silo mark in every state. Loading adds a corner spinner;
errors use red and a circled alert; warnings use amber and a triangle. An empty
or stopped list uses a muted mark. Errors take precedence over warnings and
loading, and the spinner respects reduced motion.

On macOS, native tray images use template rendering in every state so AppKit
keeps them readable against the menu bar. Each image replacement uses
[Tauri’s atomic image-and-template update](https://docs.rs/tauri/2.11.5/tauri/tray/struct.TrayIcon.html#method.set_icon_with_as_template);
the plain image setter clears template rendering, and restoring it in a separate
call introduces a redraw flash.

The status panel presents aggregate health,
one runtime-repair action, sandbox shortcuts and overflow actions, folder and site
selection, Open Silo, and Quit. TypeScript components and tokens own the design.
Freshness, busy state, and repair state gate quick actions. Repair appears once
above the list. The editor picker browses sandbox folders, not host folders.

Each sandbox's "…" menu is built once as data
(`features/status-bar/workspace-menu-items.ts`): the desktop panel renders it as a
native menu and the preview (`status-bar-popover.tsx`) with Radix, so both offer the
same items. Browser fixtures simulate lifecycle progress and host handoffs. In the desktop
app, Rust commands perform terminal, editor, site, and Quit actions. Open Silo
opens the main window and carries the selected sandbox.

## User-facing copy

Use the [owner-approved glossary](../../docs/research/codebase-review-2026-09-29.md#owner-decisions-settled-2026-09-29): sandbox, SSH host, computer, checkpoint, Restore, Fork, Duplicate settings, Export, Import, and export file. Reserve workspace for /workspace and its disk. Say “this computer” only for the local computer; name remote computers. Use Updating… and Offline · last known status for remote state.

Use “Could not” in failure titles, sentence case, and a final period for complete messages and helper sentences. Action labels have no final period. Add an ellipsis only when a dialog or confirmation follows. Errors state what failed and the next action; put technical diagnostics behind Details. Use CPUs, Memory, and Disk for resources, and GiB/MiB for binary sizes. Keep essential explanations inline or behind a labelled disclosure; tooltips carry supplementary details.

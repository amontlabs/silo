# Retained logs

Silo searches the retained history on the device that owns each computer.
The 200-record page size limits one response, not the search range. Application
state refreshes no longer fetch log tails for every computer.

## Retention

The bundled runtime retains execution, runtime and kernel output for at most
seven days with independent 125 MiB budgets for host-written execution records
and guest console output per computer (250 MiB total). Runtime and kernel console
segments share the console budget. It removes the oldest segments within each
budget when either its size limit or the age limit is reached. Segments rotate
at 10 MiB or one day.
Expiration uses the segment's first-write time, so newer records in an old
segment can expire up to a day early. The limits apply to raw runtime files;
they do not establish a minimum guaranteed investigation window.

Running computers enforce retention in their runtime. Silo also cleans up logs
for computers confirmed stopped. Cleanup does not start a computer. An old runtime
process must restart onto the updated bundled runtime to use the new writer.
Idle running computers check expiration every minute.

Logs belong to the computer's runtime directory. An exported file is a separate
user-owned copy and is not removed by log retention. Lifecycle, setup and secret
Activity journals remain separate from diagnostic logs and retain their existing
count limits.

## Search and investigation

Select computers in Logs, enter text, and add optional source or date filters.
No date range applies by default. Filter chips can be removed individually or
reset with Clear; opening or cancelling the date editor leaves the search unchanged. Search runs against retained files, including rotated segments and stopped
computers. The view reports matching record counts. Older
records load automatically as the viewport approaches the end of the list.
Rows render only around the viewport, with overscan, and short pages fill the
available space. Follow refreshes the latest results; pause before browsing
older pages. The disclosure at the end of each row expands the complete log
directly below that row. Expanding and collapsing preserve the search and filters
and make no additional log requests.
Activity entries can open the associated time window.

The first request shows skeleton rows inside the table. Refreshing keeps the
previous rows visible until the new response arrives. Returning to Logs restores
the loaded pages, expanded rows and vertical scroll position from an in-memory session cache;
use Refresh or Follow to fetch the latest records. Cache keys include the data
source, device/computer identities, search and filters. Inactive views expire
after ten minutes and share limits of eight views and 8 MiB of estimated log text.
Each active view retains at most 5,000 records and 8 MiB of estimated record text
and metadata. Paging advances this window toward older records and removes newer
loaded records, adjusting the scroll position and discarding their expansion
state. Refresh returns to the latest records. The view reports when records
leave the window; Save logs still exports every match. A response whose records
exceed the window budget can omit records from the list; narrow the search or
export to read them. These limits bound retained frontend history, not native
indexes or responses in flight. Cached log text is never written to browser storage.

The computer badge preserves its state dot and adds the existing remote-computer server
icon when its owner is another device. Hovering or focusing the badge shows
the device name; local badges show "This device". Device ownership has no
separate column, leaving more room for the log message. Source remains a separate
column. Narrow windows scroll horizontally instead of cropping metadata. Copy
and the rotating disclosure chevron occupy the trailing actions column. Expanded
rows use the shared collapsible animation and measured heights; virtual scroll
offsets include their full height. Loading another page preserves the
scroll anchor, deduplicates concurrent requests, and stops on an error or a
non-advancing cursor. Retry repeats failed page requests; Refresh starts a new
snapshot.

Each record preserves device and computer identity, timestamp, source and
execution session when available. Legacy runtime and kernel lines without
timestamps use the file timestamp and are marked as estimated. Unavailable
devices and expired pagination snapshots produce explicit errors, not invented
log entries. New writes do not shift an existing page sequence.
Search snapshots expire after 30 minutes without use and can be evicted under
memory pressure. Refresh to begin a new snapshot. Search indexes store record
offsets rather than log bodies, with a shared 128 MiB index budget and at most
16 snapshots; an oversized query returns an explicit request to narrow its time
range or search text.

Follow (review finding F-07) continues the previous first page's snapshot
instead of starting a new search every three seconds. Each snapshot records, per
retained file, the offset after its last newline-terminated record. A follow
refresh reads each surviving file only from that offset (an unterminated final
record is read again once complete), reads new files in full, drops records of
expired files, and replaces its predecessor in the cache. It skips the runtime
inspection and retention cleanup that a new search performs. A file that shrank
in place (retention truncation), a changed boot failure record, or an expired
snapshot rebuilds the index from scratch. Rotation and a previously unfinished
record also rebuild the index so PEM state follows the retained record order.
Carried records keep the estimated time they were indexed with. Refresh and export
begin new searches. Pagination reads the cached snapshot selected by its cursor;
it does not start a new search or perform Follow. The
[query adapter](../app/SiloUI/src-tauri/engine/src/runtime_logs.rs) returns cached pages
before indexing new records, and its pagination regression preserves the original
record set across append and rotation. Devices running an older Silo ignore the
follow token and run a full query.

A malformed execution record, an unreadable boot failure, or any record over
1 MiB no longer fails every query and export for the computer (review finding
F-08). Malformed records appear as placeholders with the file time, marked
estimated; console records over 1 MiB keep their first 64 KiB and end with
"[record over 1 MiB truncated]"; execution records over 1 MiB become a
placeholder. The view and export coverage report `unreadableRecords`. Times
parsed from kernel console text are chosen by the guest, so those records carry
`guestTimestamp` and the view labels them "time reported by the computer".
Guest console floods cannot evict host-written execution records: each group
has its own 125 MiB retention budget. Execution floods cannot evict console
records either. Retention runs in the pinned runtime patch, whose
`logging_retention.rs` matches Silo's `log_retention.rs` byte for byte, including
regressions for floods in both directions and shared runtime/kernel eviction.
Age-marker filenames append `.started` to the native
[`Path::as_os_str`](https://doc.rust-lang.org/std/path/struct.Path.html#method.as_os_str)
bytes, preserving non-UTF-8 runtime directories instead of replacing bytes through
display text. The shared module tests this boundary on Unix and actual expiry in
such a directory on Linux.

Copy copies the records currently fetched, with identifying context.
Export… saves all matching pages through the native save dialog as JSON
Lines. It includes coverage metadata and complete record identities. Export
queries each computer as a separate snapshot. Cancellation or a failed page leaves
the selected destination untouched and removes partial output.
If a selected remote device cannot serve logs, export asks you to update Silo
on that device and leaves the destination untouched.

The display and export use marker-based sensitive-output filtering. PEM block
state crosses records within each stream and execution session. Search scans
rotated segments oldest first, and stores each matching record's initial PEM
state beside its offset; paging, context, Follow and export therefore hide body
lines even when the requested page starts inside a block. An unterminated block
remains hidden through the retained end of that session. Other execution
sessions keep their ordinary multiline output.

Lines containing URL user information are hidden before search results, context,
pagination, lifecycle diagnostics, and exports reach the UI. The existing
`reqwest::Url` parser identifies [usernames](https://docs.rs/url/latest/url/struct.Url.html#method.username)
and [passwords](https://docs.rs/url/latest/url/struct.Url.html#method.password),
including token-only usernames and percent-encoded credentials. Public URLs and
email addresses remain readable. This filtering runs after runtime persistence.
Command lines with password, passphrase, token, secret, key, credential, or user
options are also hidden. The filter recognizes separated long-option words
and [curl's `-u` credential option](https://curl.se/docs/manpage.html#-u),
because its username/password value does not need an assignment or URL.
URL query names are decoded before applying the assignment markers, so
percent-encoded names such as `%74oken` cannot bypass filtering. Signature
parameters are hidden too, including
[Azure SAS `sig`](https://learn.microsoft.com/en-us/rest/api/storageservices/create-service-sas#specifying-the-signature-field),
[Amazon S3 `X-Amz-Signature`](https://docs.aws.amazon.com/AmazonS3/latest/developerguide/sigv4-query-string-auth.html),
and [Cloud Storage `X-Goog-Signature`](https://cloud.google.com/storage/docs/authentication/canonical-requests).

The pinned runtime's [retention patch](../app/SiloUI/patches/microsandbox-log-retention-desktop-start-0.7.6.patch)
renames older segments to increasing numeric suffixes. Silo uses numeric suffix
order rather than directory enumeration or guest timestamps for classification.
Tests generate disposable RSA keys with OpenSSL and exercise the production
query and JSON Lines export functions against temporary files.

Raw runtime files can contain private application output; this filter is not a guarantee
that arbitrary secrets are removed. Review exports before sharing them.

## Implementation

- [Query adapter](../app/SiloUI/src-tauri/engine/src/runtime_logs.rs): local retained
  files, search, pagination, surrounding records and remote owner routing.
- [Retention policy](../app/SiloUI/src-tauri/engine/src/log_retention.rs): segment age,
  independent execution/console byte budgets and stopped-computer cleanup.
- [Runtime patch](../app/SiloUI/patches/microsandbox-log-retention-desktop-start-0.7.6.patch):
  execution, runtime and kernel writers.
- [Logs view](../app/SiloUI/src/features/application/pages/logs-page.tsx):
  search, filters, refresh and follow controls.
- [Log history cache](../app/SiloUI/src/features/application/model/use-log-history.ts):
  session snapshots, owner errors, cursor deduplication and chronological merging.
- [Logs table](../app/SiloUI/src/features/application/components/logs-table.tsx):
  skeletons, metadata columns, viewport-sized rendering and automatic pagination.
- [Native export](../app/SiloUI/src-tauri/engine/src/log_export.rs): paginated,
  cancellable, atomic file export.
- [Original audit](SiloUI-LOGGING-AUDIT.md): the preceding behavior and findings.

The cache uses immutable snapshots and stable subscriptions as specified in
[React's useSyncExternalStore documentation](https://react.dev/reference/react/useSyncExternalStore).
The viewport responds to element size changes through
[ResizeObserver](https://developer.mozilla.org/en-US/docs/Web/API/ResizeObserver),
including when a hidden panel becomes visible. Scroll and resize checks share a
request guard, so they cannot issue the same page concurrently.

Chronological ordering depends on immutable history results, independently of
computer names and state refreshes, following React's
[useMemo dependency contract](https://react.dev/reference/react/useMemo).
Computer presentation updates still reach the rows. Copy formats the current
window only when clicked, and per-row Copy formats only that record. Paging
requests only the owners at the newest unread frontier, so quieter owners' older
pages stay buffered until they can appear in chronological order. Each owner
retains at most 64 recent cursor keys; monotonic record ordering also rejects
cycles whose cursors or records have already left the window.

## Verification

O-05 follow-up on 2026-10-02 adds deterministic tests for paging all 50,000
synthetic records while retaining at most 5,000, text-budget eviction, navigation
after eviction, scroll compensation including expanded rows, quiet-owner
buffering, oversized responses, and cursor cycles after eviction. Workspace
presentation refreshes read no record timestamps for ordering; clipboard tests
verify zero formatting before a click and that Copy uses the current window.
These checks use frontend fixtures and do not establish installed-app or live VM
performance.

Computer badge follow-up on 2026-09-22 removes the Computer column and reuses
`ConnectionIcon`'s remote-computer server silhouette inside `ComputerBadge`, retaining
the state dot. The shared badge exposes the owning computer in its accessible
name and in a tooltip on hover or focus. A same-named local/remote fixture verified
the distinction visually and confirmed the "Office Mac" tooltip. Skeleton and
expanded rows now span five columns. All 166 affected tests and lint passed.
The initial full run passed 928 tests with two failures in concurrently added
`lifecycleFailure` tests. Once those unrelated edits completed, both affected
suites passed all 67 tests and the TypeScript/production frontend build passed.
Evidence is under `target/verification/logs-computer-badge-*`. No native bundle
was rebuilt or inspected.

Inline disclosure follow-up on 2026-09-22 replaces surrounding-log navigation
with the existing `Collapsible` and `DisclosureIndicator` components. Full multiline
messages open below their records, and expanding/collapsing makes no log request.
Regression tests cover retained filters, measured-height virtualization,
pagination thresholds, and expanded-row restoration across navigation. A browser
fixture verified expansion, collapse, and navigation away/back with one total
request, the same 3,200-pixel scroll offset and the complete message still open.
The viewport rendered 26 records while paging through expanded history. The full
frontend suite passed 927 tests before the final navigation regression was added;
the four affected suites, lint and production frontend build were rerun for the
final change. No native bundle was built or inspected. Evidence uses the ignored
`target/verification/logs-accordion-*` paths under `app/SiloUI/src-tauri/`.

Logs UI verification on 2026-09-22: all 926 frontend tests across 101 files
passed, along with lint and the TypeScript/production frontend build. The build
reports the existing large-chunk advisory. New regressions cover skeleton rows,
cache restoration through navigation, pending-request deduplication, cache
expiry/eviction, contextual positioning, failed-page retries, short-page filling,
and viewport-sized rendering for 10,000 loaded records.

A browser-only fixture with 2,200 deterministic records verified initial
skeletons, automatic pagination from 200 to 400 records, and navigation away and
back with two total requests and the same 9,976-pixel scroll offset. The viewport
rendered 26 rows including its header. At 1,200 × 800 and 900 × 700, computer and
source remained complete; the narrower viewport kept horizontal scrolling
inside the table. No native bundle was rebuilt or inspected, and no live VM
state was used. Test/build logs and the disposable browser fixture are under
`app/SiloUI/src-tauri/target/verification/logs-ui-*`.

Deterministic backend tests cover an error behind 100,000 newer records,
pagination during appends and rotation, in-place truncation and replacement,
source/time filters, surrounding records, non-UTF8 console output and long
messages. Paging all 100,002 synthetic records took approximately 3.4 seconds
in a local debug run. Export tests cover complete pagination, identity,
cancellation, failed writes and a non-advancing remote cursor. UI tests cover
search, bounded rendered rows, owner failures and follow/pause.

Final verification on 2026-09-18:

| Check | Result |
| --- | --- |
| `npm --prefix app/SiloUI test` | 827 tests passed across 90 files |
| `npm --prefix app/SiloUI run lint` | Passed |
| `npm --prefix app/SiloUI run build` | TypeScript and production frontend build passed; Vite reports a large-chunk advisory |
| `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml` with explicit synthetic GitHub configuration | 416 passed, 11 ignored |
| Patched upstream runtime | Compiled; nine logging tests, kernel pipe ownership test and synchronous storage-failure test passed |
| Upstream SDK log streams | Six tests passed, including archives beyond the previous four-file discovery limit |
| Runtime staging | 13 tests passed; final patch applied to the pinned archive and matched intended sources |
| `npm --prefix app/SiloUI run test:release` | 34 release-tooling tests passed |
| Browser fixture | Inspected layout, visible dates, search and surrounding-record navigation |

Generated test output is under the ignored
`app/SiloUI/src-tauri/target/verification/logs-*-tests*.log` paths. These tests
establish behavior for their supplied data; they do not prove a running installed
app, Linux VM console capture or a live two-computer workflow. No packaged bundle
was built or inspected. No running user VM was stopped or changed for verification.

Subsequent native preview on 2026-09-18: `npm --prefix app/SiloUI run
desktop:build:debug` built the debug bundle at
`app/SiloUI/src-tauri/target/debug/bundle/macos/Silo.app`. Launching this exact
bundle exposed missing Tauri permissions for the three log commands. The command
manifest and main-window grants were fixed; three regression tests passed, along
with nine focused Logs tests, typecheck and lint. The rebuilt app was launched
and its Logs view inspected against actual local state: the query completed
without a permission error and returned no records. Retention and search-scope
commentary were removed from the view. No VM was started for this preview.

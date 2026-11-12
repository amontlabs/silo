# MicroSandbox maintainer profile (2026-11-11)

Repo: `superradcompany/microsandbox`. Sources: `gh` (read-only), `origin/main` at f9ae8e23 (v0.7.15 bump).
"Evidence" has a link; "Inference" is my reading. PRs scanned: all 15 of ours, closed-unmerged externals, ~120 recent merges, plus #1883, #1957, #1967.

## Who decides

- **Two maintainers own everything.** CODEOWNERS is `* @appcypher @toksdotdev` (`.github/CODEOWNERS`).
- **toksdotdev merges and reviews day to day.** Of the last 120 merged PRs, 94 were merged by toksdotdev and 22 by appcypher (`gh pr list --state merged`). Every one of our merged PRs was merged by toksdotdev.
- **appcypher sets direction and builds the big pieces.** Examples: the snapshot rework #1883 (+29.9k lines), the `/.msb` quota decision in [#1007](https://github.com/superradcompany/microsandbox/issues/1007), the lock-leak fix [#1558](https://github.com/superradcompany/microsandbox/issues/1558). Inference: toksdotdev is the gatekeeper for external PRs, appcypher for architecture.
- **Greptile (bot) does the first review of every PR.** Maintainers expect authors to answer it. Our replies use `@greptileai` ([#2045](https://github.com/superradcompany/microsandbox/pull/2045)). Inference: a PR with open Greptile P1s will not get merged.
- Outside contributors with several merges: TheRealSibasishBehera (11), gregwebs (5), Jackmeson1, GoldArowana, ourselves.

## What they ask for in reviews

Evidence is mostly toksdotdev's inline comments.

- **Tests that exercise the real production path, not a helper.** [#1841](https://github.com/superradcompany/microsandbox/pull/1841): "removing the production exit wiring would still leave this test passing". They also asked to drop a test-only static (`EXIT_CLEANUP_WIRING`) and to have tests clean up after themselves. Same from Greptile on #2045 and #1744.
- **SDK parity.** On [#1733](https://github.com/superradcompany/microsandbox/pull/1733) toksdotdev asked twice for the same option in every SDK and in `msb fork`. They accepted "already possible, so tests and docs are enough" for the first ask. `AGENTS.md`: "keep the Rust SDK, CLI, Python, Node, Go SDK, docs, and examples consistent".
- **No behavior regressions on neighbouring paths.** [#1734](https://github.com/superradcompany/microsandbox/pull/1734): a new check would break restore of missing file mounts; they asked to move it after admission.
- **Design simplification, steered by them.** [#1751](https://github.com/superradcompany/microsandbox/pull/1751): they proposed the single global `snapshots.max_filesystem_state_mib` setting and asked us to carry it through every capture and restore path.
- **Small code hygiene.** AGENTS.md section layout (`Types` / `Methods` / `Functions`) in #1841; shorter test comments in #1841; "is this needed by later code?" and delete dead code in [#1799](https://github.com/superradcompany/microsandbox/pull/1799); naming (`attached_exec_sessions`) in [#1923](https://github.com/superradcompany/microsandbox/pull/1923); "don't break existing `ExecRequest` struct literals" in #1923.
- **Rebase on main** is the most common maintainer comment: #1739, #1751.
- **Size.** No PR has been refused for size, but the two giant external PRs were closed by their own authors: #1482 (+69k, "too large to review as one PR"), and #2021/#1840 (+66–70k). The one 5.6k-line PR of ours (#1739) is still open after 5+ weeks. Inference: small fixes merge in hours, large features stall.
- **Changesets: none.** No changeset requirement; release notes are generated (`AGENTS.md`: "do not hand-edit generated changelogs").
- **Why PRs were closed unmerged:** nearly all by the author, not the maintainers. Jackmeson1 closed three small ones: "hasn't had review activity" (#1973, #1946, #1942). Fork CI and Mintlify previews need a maintainer to approve, which stalls first-time contributors ([#1841](https://github.com/superradcompany/microsandbox/pull/1841) comments).

Time to merge for small external fixes: median a few hours to ~4 days; ours were 0–15 h except large ones (computed from `createdAt`/`mergedAt`).

## Stated rules

- `AGENTS.md` (long, binding for agents): conventional commits, PR body `TL;DR / Description / Test Plan`, signed commits, no emojis, fmt + clippy `-D warnings` + tests, docs updated with behavior. Our PRs use Problem / Expected / Fix; they merged fine.
- Backward compatibility is a stated hard rule. Newer SDK with older `msb` and the reverse must both work, and a missing feature must give a clear "upgrade required" error. Persisted state, journals, leases, snapshots, caches and content-addressed storage each need a cross-version review (`AGENTS.md` lines ~130–190, `COMPATIBILITY.md`). Greptile enforced it on [#1713](https://github.com/superradcompany/microsandbox/pull/1713) (older guests).
- Security-sensitive: only `sbx/guest-state/` is exported to the guest; "Keep host journals, checkpoint stores, disk staging... outside that subtree" (`AGENTS.md:117`).
- "Prefer explicit errors with useful context over silent fallbacks." "Do not silently add compatibility layers, migrations, ... fallback paths."
- `CONTRIBUTING.md` is short: one focused change, tests + docs, conventional commits. Issues for bugs/features; Discord or Discussions for design talk. Issue templates are bare (bug / feature).

## Design taste

- **Cleanup is conservative, crash-safe, and journaled.**
  - Snapshot removal moves the directory to `.snapshot-deletions/delete-*` with fsyncs and an intent record, then deletes; recovery finishes interrupted deletes and never touches a new artifact at the same name (`sdk/rust/lib/backend/local/snapshot/deletion.rs:19-75`).
  - #1883 adds a separate `.group-retirements` journal for group generations ("durable reclamation", `group/generation_retirement.rs`).
  - Exclusive per-entry leases protect removal; removal is refused while a reader holds the snapshot, and `--force` never bypasses readers (`docs/cli/snapshot-commands.mdx:138-146`).
- **Cleanup is mostly explicit and narrow, not a background GC.** `msb prune` today only removes *unused runtime RAM cache* files (`crates/cli/lib/commands/storage.rs:1`, "reclaim unused runtime RAM"). Shape: `--dry-run`, `--older-than`, `--yes`/interactive confirm, `--format json`; it revalidates every candidate under ownership locks before unlinking. Its doc says durable snapshot objects are "never removed" (`crates/runtime/lib/checkpoint/cache_storage.rs:184`). `msb image prune` is the same style, and toksdotdev agreed to add a *dangling-only* variant while "the existing prune behavior will stay as the default" ([#1788](https://github.com/superradcompany/microsandbox/issues/1788)).
- **Leaks found in the lifecycle are fixed at the owner, automatically.** Bootstrap rootfs dirs (#1834 → #1841): removed on VM exit by the owner. Failed boot leaving `sandboxes/<name>` (#1908) and interrupted creation (#1918/#1929) are handled in lifecycle recovery. Volume record kept until file removal succeeds ([#1873](https://github.com/superradcompany/microsandbox/pull/1873)). Inference: when the cause is "this operation forgot to clean what it made", they want it fixed in that operation, not via a sweeper.
- **Observability next to cleanup.** `msb df` reports per-object "In use / Reclaimable" and reasons (`storage.rs`, `sdk/rust/lib/storage/usage.rs:165` still says snapshot "retention eligibility [is] unknown"). Inference: any new reclaimable storage should show up in `df` too.
- **No existing machinery for checkpoint-store objects.** `LocalObjectStore` (`crates/image/lib/checkpoint/store.rs`) has `put_bytes`, `object_path`, `link_into`, and no remove or sweep. `checkpoint-store` is only referenced by layout validation and migration code. Objects are linked into snapshot closures with hard links (`link_into`), so unreferenced = link count 1 (inference from our own finding).
- **Breaking changes happen but are announced.** #1883 is marked `!`, lists CLI grammar and SDK changes, and gates exports with a `0.7.8` target. Feature flags/capabilities gate cross-version behavior (Go native marker symbol in #1957).
- **Cross-SDK parity is expected for public API**, less so for pure internal fixes (all our internal fixes merged with Rust-only changes).

## Overlap with our question

- **#1883** (draft by appcypher, +29.9k, 57 Greptile reviews): group-first snapshot management, atomic group removal, retirement journal, `snap rm GROUP --checkpoint/--all`. I found nothing about memory objects in `checkpoint-store` or about `prune`. Its retirement code concerns the group directory, not a sandbox's `checkpoint-store` objects (I could only read file names and one file, not the 30k-line diff). It touches `crates/image/lib/checkpoint/store.rs` (+10/-2) and `resolver.rs` (+331), so conflicts are possible. It says "Complete removal retains every member lease."
- **#1957 / #1967** (bgeils, open): external-base disks and integrity receipts. `prune` in #1967 is an internal receipt-eviction helper, not a user command. No plan for deleting stored objects. They talk about a "memory-object pipeline" and read performance only.
- **Our open PRs** [#2045](https://github.com/superradcompany/microsandbox/pull/2045) (delete capture folder after full snapshot) and [#2044](https://github.com/superradcompany/microsandbox/pull/2044) (delete old disk layers after compaction) are already the "fix at the owner" style. A checkpoint-store fix would be the third in that series.
- **Issues:** none found on checkpoint-store or snapshot disk growth. Closest: #1788 (image prune), #1278 (storage-efficient snapshot chains, open, no maintainer reply).

## How to pitch

- **Bug fixes: PR directly, no issue first.** 15 of ours merged or are open this way; toksdotdev approves with "lgtm, thanks for the fix", often the same day. When an issue exists, toksdotdev replies within hours to a day and often says "I'm working on it" ([#1788](https://github.com/superradcompany/microsandbox/issues/1788), [#1872](https://github.com/superradcompany/microsandbox/issues/1872)). Inference: for anything touching design (new command, new setting), open or comment on an issue first; maintainers reshape designs in review (#1751).
- **A good issue to them:** version, platform, exact repro, observed vs expected, source pointers. They answered those quickly. Long speculative design essays got a one-line decision from appcypher ([#1007](https://github.com/superradcompany/microsandbox/issues/1007)).
- **Size:** one behavior per PR; keep Rust-only unless the public API changes. Offer to split big ones and expect slow review otherwise.
- **Description:** short TL;DR, flat bullets, concrete test plan. Include why it is safe for old releases when anything persisted changes.
- **Tone:** friendly, terse, lowercase, "ty!". Unresponsive PRs (not ours) just sit; a polite nudge was used on #1841.

## Recommendation for the checkpoint-store question

Evidence says they prefer **(a) automatic cleanup at the point of removal**, with an explicit command only as a possible add-on.

1. **Do (a) first, as a small Rust-only fix.** When a full snapshot is removed, delete memory objects that no other snapshot references, in the same locked and journaled removal path (`snapshot/deletion.rs`). Reason: it matches #1841, #1873, #1918 and our #2044/#2045 (fix the owner), and `prune` today means "unused RAM cache" and says durable objects are never removed. Inference.
2. **Decide "last reference" by hard-link count, checked under the removal lock.** Objects are hard links, so an object whose link count is 1 after the snapshot closure is gone has no snapshot. Do the check against the locked store, not a cached list, and let an interrupted removal be finished by the existing recovery journal. They want crash safety and reader protection (leases), so refuse or skip objects a restore, fork or export still holds.
3. **Do not add a sweeper or daemon.** No evidence they want a background GC. If a leftover case remains (old sandboxes whose objects were already orphaned), add the cleanup as a flag of the existing command with their style: `--dry-run`, `--yes`, JSON report, revalidate under locks. Pitch it in an issue first, since `msb prune` currently never touches durable objects and changing that is a product decision (and appears in `msb df`).
4. **Add the test they ask for:** remove a real full snapshot through the production path and assert the object is gone and a sibling snapshot's object stays. Plus a crash-before-delete recovery case.
5. **Check #1883 before coding.** It rewrites removal (`snap rm GROUP --checkpoint`) and touches `store.rs`/`resolver.rs`. Comment on #1883 or open an issue so the fix lands where appcypher's removal path will end up; otherwise expect merge conflicts or "this is handled in the rework". Also check compatibility: deleting objects must never remove one an older `msb` still expects (AGENTS.md compatibility section).

Open risks I could not resolve: whether #1883 already deletes these objects (diff too large for `gh`), and whether the owners treat an old unreferenced object as a bug or intentional retention.

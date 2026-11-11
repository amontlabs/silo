# Direction build: log

Plan: [PLAN.md](PLAN.md). Agent rules: [AGENT-BRIEF.md](AGENT-BRIEF.md).

## Resume

- **Started:** 2026-11-10, from main at the commit that adds this file.
- **How work lands:** each item is a delegated agent in its own worktree and branch. It runs its
  own review loop with the `reviewer` model, then reports. The orchestrator reads the report,
  reviews boundary changes itself, merges the branch into the local branch `direction` (checked
  out at `~/code/projects/silo-direction`), regenerates the API files if two items touched them,
  and deletes the item's target folder (`~/code/projects/silo-wt-targets/<item>`). `main` on
  GitHub accepts only pull requests with seven required checks (found 2026-11-10), so the build
  stays on `direction` and goes up as pull requests at the end.
- **Live checks** use the Dev engine, one item at a time. On 2026-11-10 a Dev app and engine from a
  deleted build (`silo-dev-target`) were still running; an item that needs the Dev engine
  replaces them with its own build.
- **Models:** Sonnet implements; Opus for T1, T2 and T5 (they define the security boundary);
  `reviewer` (Codex gpt-6.1-sol high) reviews.

## Status

| Item | State | Branch | Notes |
| --- | --- | --- | --- |
| S1 | merged 2026-11-10 | `docs/s1-gateway-channel-spike` | [evidence](evidence/S1.md). Channel: yes, no patch. Allowlist: needs strict off, a `restore --net-strict` patch, and gateway init fields |
| T1 | merged 2026-11-10 (`e000d684c`) | `t1-policy-audit` | silod 341, api 52, policy and audit 42 tests passed in the item. Only one review round ran (fallback model); the second hit a quota. Orchestrator read rules.rs, policy/mod.rs and the gate: no hole found; checkpoint ids are looked up inside the named computer's record. A Codex review is running |
| T1 review | done 2026-11-10 (Opus) | | one high finding and seven smaller, listed under Found. All eight fixed and merged 2026-11-10 (`fix/policy-audit-review-findings`): the made record moves inside the promote's own Lineage step and is saved in the promote journal, so undo and recovery reverse or repeat it; silod 346 and the engine's policy, audit and promote tests (116) passed in the item. Orchestrator read `policy/handoff.rs`. Not exercised against a real engine. A Codex pass is still owed |
| T2 | merged 2026-11-11 (`583c19b97`) | `feat/silo-approvals-store` | merged by hand against the T1 fixes and T3 (gate, policy, counts); 1472 focused tests passed after the merge. Reviewed twice in the item on the fallback model. Opus review done 2026-11-11: no way found to run an unapproved call, run one twice, or reach decide. Twelve findings around it: an allowed `secrets.save` returned the whole secret list to the asker; summaries left out fields the handler honours (GitHub repositories and write access, SSH bind address and keys, the device port, the push branch, merge hooks); secret values stayed in the approval file 30 days; no limit on how often a computer asks; agent text unvalidated and unsanitised in summaries, notices and the CLI. All twelve fixed on `t3/96d5b27b` (silod 384 lib tests, api 66, cli 157, engine approvals/policy/fold_requests 82 passed in the item). It conflicts with T8 in `engine/src/policy/mod.rs` (five hunks); merged with it by an agent on `merge/t2fix-into-direction` and landed 2026-11-11; the merge also made `network.policy.set` ask with its rewritten parameters and store a small outcome |
| T3 | merged 2026-11-10 | `feat/device-resource-limits` | merged by hand; 588 focused tests passed after the merge. Reviewed once by the fallback model. Open: a burst of starts can each pass before any computer has booted (needs a reservation of admitted starts' memory); a request larger than the headroom is still admitted; disk is measured at app data, not a separately chosen storage location |
| T4 | merged 2026-11-10 | `feat/keep-conflict-choices` | engine tests 273, api 53, silod checkpoints 56 passed in the item; two review rounds; the `silo-ui` crate was not built there |
| T9 | merged 2026-11-10 | `feat/macos-computer-app-parity` | frontend only, no Rust change; 2914 frontend tests passed in the item; reviewed by the fallback model (gemini flash) only, one round; not run in the real app |
| T11 | merged 2026-11-10 | `feat/github-token-secrets` | 794 focused tests passed after the merge; frontend suite passed in the item. Reviewed once by the fallback model; a fresh-context review is running. Not run against a real computer, Keychain or GitHub. Open: the old single-token methods only find a secret named exactly `github-token`; an older Silo cannot read a `secrets.json` that holds a token secret; a fork still refuses to copy token access |
| T12 | merged 2026-11-10 | `feat/computer-agent-install` | merged by hand (registry conflicts with T1); `agents.list` is Tree and `agents.install` is Made in the policy; 637 focused tests passed after the merge. Sonnet review done: one host-harm finding (unbounded guest stderr held in host memory and copied into the job error on macOS), plus false `missing` for an unreachable computer, an uncancellable Linux install, a racy create-time install, and weaker checks than intended. All nine findings fixed and merged 2026-11-10 (`t3/6c7ce149`): guest output capped, an unreachable computer reports `unknown`, a Linux install is stoppable (process group, checked on the Linux test machine), create-with-agents takes the install guard and reports steps. Left: a failed create-time install shows a generic message. macOS create with agents is refused; Linux create installs best effort; no real computer run |
| S2 | merged 2026-11-10 | `docs/s2-linux-computer-rebuild` | [evidence](evidence/S2.md). Verdict: buildable now for Linux with host-side disk moves; macOS later. T15 waits for T6 (recipes in the engine) |
| T7 | merged 2026-11-10 | `fix/macos-nat-boundary` | 530 macOS engine tests passed; unit tests only, no macOS computer run; one review round on the fallback model. Live steps are in the item's report, copied below |
| T11 review | done 2026-11-10 (Sonnet) | | no token value leaks found. Ten findings; the important ones: a network failure detached computers (also true before T11), a stale check could overwrite a newer token's state, and an agent's request for token access was summarised without saying a full-permission token is given, with a missing id bound to the owner's `github-token` after approval. All ten fixed and merged 2026-11-10 (`fix/github-token-review`); api 63, silod 353, engine github/secrets/policy 297 and 902 frontend tests passed in the item. For the owner: the old single-token fallback applies only in a session where a client used the old methods; a token with no answer since start shows waiting and its computers stay detached until GitHub answers |
| T8 | merged 2026-11-11 | `t3/d4017e2b` | 1046 focused tests passed after the merge. Live check with the rebuilt runtime in a private home: each mode behaves as designed, also after changing a stopped computer's mode, after a fork and after a restore in place ([evidence](evidence/T8.txt)). Opus review done 2026-11-11, findings under Found. All ten fixed on `fix/t8-network-policy-findings` (`9b02ee553`; engine 1077, silod 385, cli 158, api 66 passed in the item; the tests were not seen failing first). It conflicts with T5a in `runtime/checkpoints.rs` and `silod/src/methods/computers.rs`; merged with it by an agent on `merge/t8fix-into-direction` and landed 2026-11-11 (the merge also stopped an export's closed copy from inheriting a channel port). New in the API: `expected` on `network.policy.set` (compare and set), `presetUpdates` and `notices` on `network.policy.get`; presets are versioned; a 118-entry public suffix list refuses broad wildcards. Not verified: a Linux host's runtime (built by CI), a live macOS or container gateway. The channel hook is `network_policy::extra_rules(computer_id)` returning `ExtraRule::GatewayPort(port)` entries |
| T5a | merged 2026-11-11 | `t3/1800d91b` | live check on the Dev engine 52 of 52 ([evidence](evidence/T5a.txt)); 992 focused tests passed after the merge, one flaky (below). Not verified: macOS computers and containers (gateway rule unit-tested only), `scripts/linux-host-e2e.sh` |
| T5a review | done 2026-11-11 (Opus) | | no critical finding: identity, the gate and the network rule hold. Eight issues from loopback being shared ground: the token is not tied to its port and two enables can race; a token learned while the engine is down still works after restart; a turned-off or moved port stays reachable from the computer; unauthenticated connections can lock a computer's tools out; sessions survive a new token; a macOS fork and exports carry a live token; one stalled computer delays every endpoint check. Fix task (Opus), which also makes the handshake a challenge and response so the token never crosses: all eight fixed and merged 2026-11-11 (`fix/channel-handshake-security`); the focused suites passed after the merge. Live part run on the T5b merge 2026-11-11: 65 passed, including the new steps S4b, S4c, S8b, S10b, S11b ([evidence](evidence/T5a.txt)). Open: a port another program already holds at engine start is reachable from the computer until it restarts; the wiring that ends a token after an export has no test |
| T5b | merged 2026-11-11 | `feat/guest-silo-components` (`2540e2e67`), merged on `merge/t5b-into-direction-2` | live check 38 of 38 on its branch and 42 of 42 after the merge, with a Codex install (its folder mode fixed) ([evidence](evidence/T5b.txt)). After bringing in T13b, `silo-ui` did not build: `guestClient.enable` now takes `GuestClientEnableParams` (fixed, no binary override from the app). 1313 focused Rust tests, `silo-ui` check and 3112 frontend tests passed after the merge. No review yet |
| T6 | merged 2026-11-11 | `feat/finish-direction-cli` (continues `feat/direction-build-cli`) | recipes stored by the engine (fork rules, merge hooks, packages after a merge), CLI without `.silo`, merge and replace-main, approval waiting, `choices-needed` wording, network policy output, MCP tool set, CLI and recipes docs. Merged with T5b and T13b without conflicts; 1331 focused Rust tests (api, silod, cli, engine policy/audit/guest_client/changes/macos/agents/recipe), `silo-ui` check, tsc and 3112 frontend tests passed after the merge. No review yet (engine-applied merge hooks and recipes are boundary work: review owed). Not run against a real computer |
| T6 review | done 2026-11-11 (Opus, via the reviewer fallback; Fable was at its limit) | | one high: a free `changes.keep` into a computer the agent made appends packages to the owner's saved recipe (`recipes/additions.rs` `record_merge`), so the owner's next rebuild installs what the agent chose. Medium: the merge summary shows the caller's hooks, not the recipe's that run; saved recipes are written without a lock or unique temp file; an unreadable records file silently skips scrub and hooks. Low: undo leaves recipe additions; `silo-build-` prefix trusted by name; any computer can read every saved recipe (`recipes.get/list` Open, setup lines may hold tokens); appended entries skip size limits. The high, the three medium and the size limits fixed and merged 2026-11-11 (`fix/t6-review-findings`, `28bf9993e`, Gemini Flash through lastdrop): a merge changes the saved recipe only when the owner or a Device runs it (an approved call runs as the owner), and the approval says so; caller hooks into a recipe computer are refused and the summary shows the recipe's; one lock, a unique temp file and fsync for saved recipes; an unreadable records file refuses the fork or merge; additions respect the size limits. 1563 focused Rust tests, `silo-ui` check and fmt passed after the merge. Left for the owner: undo keeps recipe additions; every computer can read every saved recipe |
| T13b | merged 2026-11-11 | `feat/network-policy-controls` | 3112 frontend tests and the shell's command tests passed in the item (same base as the merge). No review; never run in the real app |
| T15 | merged 2026-11-11 | `feat/t15-linux-rebuild` (`cc1158812`) | Gemini's first version did not compile; Sonnet rewrote it on the engine's real paths, then two review rounds (reviews/T15-review-1.md, -2.md), all findings fixed or answered. Live check on the Dev engine 173 passed, 0 failed, 3 skipped (secret assignment needs the Dev engine's Keychain approval) ([evidence](evidence/T15.txt), [notes](evidence/T15-notes.md)): refusal without `--force`, new root disk with id, workspace, home, policy and tools kept, `-undo` stopped with no tools access, stopped computer stays stopped, all 9 engine-kill points converge, Quit during a rebuild stops it and the next start settles it. Only one guest image exists here, so root-disk identity is the proof. 1828 focused tests after merging `direction`. Not verified: an older-to-newer image, a computer with a recipe, a Linux host without reflink, secret carry-over live |
| R1 | merged 2026-11-11 | `fix/r1-recipe-access-undo-2` (`8f402850c`) | Gemini twice ended with nothing; Sonnet built it. A computer's `recipes.get` returns only the recipe its own computer was built from; `recipes.list` already carried no setup lines (names, os, hash). A merge records what it appended (record and saved recipe) in its undo journal; undo removes exactly those entries under the library lock; old journals load. 1589 focused tests passed after merging `direction`. No review (quota); undo not run live |
| R2 | merged 2026-11-11 | `fix/r2-resources-hygiene` (`11438b6c3`) | Gemini left it unfinished; Sonnet finished it. Admitted starts from computers reserve their memory until the start job ends (at most 300 s, cleared on a silod restart); a request above the free headroom is refused; the loopback port test picks below the ephemeral range (20 runs clean); the `silo-ui` build script no longer emits the GitHub values and errors name the key only. 1587 focused tests passed after merging `direction`. Check and reserve are not under one lock (no await between them, so only a microsecond race). Approved starts check the floor but do not reserve (OWNER_REVIEW 14). No review (quota); orchestrator read the gate change |
| R3 | merged 2026-11-11 | `fix/r3-guest-components` (`4865bff44`) | Gemini left it unfinished; Sonnet redid and finished it. All nine T5b review findings fixed (override kept until start/enable, 90 s budget and 5-failure cap, no-op looks, marked MCP entry with conflict for a user's own `silo`, guest receipt hidden when off, `mktemp` root writes, fork owes removal, owed removal cleared only on success, Zig pin test). 1594 focused tests, Python helper tests and the node test passed after merging `direction`; 907 again after R1. No review (quota); not run in a real computer |
| R4 | merged 2026-11-11 | `fix/r4-app-review` (`e82c90965`) | Gemini left 33 files unfinished; Sonnet finished it. All ten app review findings fixed; a Save no longer re-pins presets (only `acceptPresets`), stale saves carry the code `changed`, host checks use the engine's own suffix list, Open on macOS uses the computer's own `ssh -F` config. 1636 Rust and 3120 frontend tests passed after merging `direction`. No review (quota); never run in the real app |
| R5 | merged 2026-11-11 | `fix/r5-recipe-setup-hidden` (`6231d24b3`) | Found by T16: forking a recipe's hidden build handed an agent the recipe's full text. A computer caller no longer gets setup, service `run`, keep `migrate`/`after` (`commandsHidden: true`) from `recipes.get`/`computers.recipe`, and merge results name recipe hook steps without command text or output (also for approved merges). 2112 Rust and 3120 frontend tests passed after merging `direction`. No review (quota) |
| R6 | merged 2026-11-11 | `fix/r6-macos-disk-size` (`55041576a`) | Found by the T7 live check. After a failed macOS disk expansion the record keeps the disk file's size and the note says macOS can use only part of it; mismatched records and templates are repaired on load (in memory, saved on next write); export takes `diskGiB` from the file it copies. Cause of APFS -69519 not confirmed (likely Recovery behind the container); the expansion itself is not fixed (OWNER_REVIEW 26). No review (quota); not run live |
| R7 | merged 2026-11-11 | `fix/r7-command-lock-inheritance` (`93fb0a8a7`) | Found by the T15 live check. Cause: `spawn_runtime` cleared close-on-exec on the command lock so `msb` children (and the VMs they start) inherited it. Now the lock stays with the engine; a pending record before `spawn()` and an atomic child record after it keep a surviving `msb` command from being overlapped; stale, garbled and dead-engine records expire; a record write failure stops the child (TERM, then KILL) and keeps the lock until it exits. Reviewed once (Sonnet), all findings fixed. 2746 focused tests after merging `direction`. Not verified: a live engine kill with running computers; the Linux `/proc` path on Linux (test machine did not answer) |
| V | done 2026-11-11 | `direction` (`f07a0c6d6`) | End-to-end run with the Dev app on screen (LCU), 47 lines: 36 PASS, 2 FAIL, 9 SKIPPED ([evidence](evidence/V.txt)). FAIL: Activity's Open request does nothing; `silo tools status` inside a computer refused with an internal method name. Findings 1-12 in the evidence; the main ones: every fork merge conflicts on `/etc/silo/endpoint.json`; one merge approval shows as two Pending rows; ids and CLI words in the merge review; the Dev app did not start its engine and kept a stale runtime issue until Retry. Skipped: system notifications and the menu bar panel (macOS permission banner unanswered, SystemUIServer not approved for LCU), onboarding (no `silo up` main), agent Update, preset update, macOS computer (disk) |
| R8 | merged 2026-11-11 | `fix/r8-v-engine` (`6aa144f14`) | V findings 3, 4, 5, 6, 9: Silo's per-computer files (`/etc/silo`, `/var/lib/silo`, machine ids, tools receipt) are on the built-in ignore list; an approval's review request carries its change counts and Pending joins it to the approval (one row); merge texts use names and plain words, recorded choices apply when merging from the review page; a computer may read its own tools status, refusals name the command not the method; install failures name the blocked sites. Tested together with R9: 3041 Rust and 3136 frontend tests; one flaky silod test (below). Not run live |
| R9 | merged 2026-11-11 | `fix/r9-v-app` (`420fbba7f`) | V findings 2, 7, 8, 10, Retry label. Cause of the Dev engine not starting: the launchd job is `KeepAlive: SuccessfulExit=false`, so after a clean exit it stays idle and the app only waited; the launcher now `launchctl kickstart`s a loaded job. Health checks re-run when the engine connects. Open request opens or is disabled with a reason; Pending returns to the list; failure toasts close after 20 s; Retry label; tools row refreshed after installs. Not run live |
| R7 live | done 2026-11-11 | `direction` (`c505772f3`) | Lock released after a SIGKILL during a stop or a create (`lsof` empty), next engine answers in 1-6 s, create/stop/start/Quit work ([evidence](evidence/R7.txt)). T15 check after R7: 171 pass, 2 fail, 3 skipped ([evidence](evidence/T15-after-R7.txt)). New failures, not the lock: (1) a kill 5-6 s into a create leaves the next engine refusing every command with "saved computer configuration does not match its managed runtime state" (2 of 3 tries; once a restart cleared it); (2) T15 crash point 8 (kill 2 s into the swap): rebuild finishes, but the first command into the computer fails after about 3 minutes. Being fixed as R10 |
| R10 | merged 2026-11-11 | `fix/r10-crash-recovery` (`4203075c8`) | (1) Engine killed mid-create: recovery stops the half-made (unsaved) computer and removes and replays it only on a definitive failure (tools check exited non-zero, or resources do not match); timeouts keep it for the next launch; `read_rows` tolerates only unsaved runtime computers with the pending id label, so one stuck create no longer blocks every read. (2) T15 crash 8 was not an exec hang: a capture finished just before the kill could not be discarded, so every start failed; `adopt_finished_capture` keeps it after `snapshot verify`. (3) flaky silod access test isolated; a stuck removal for one device is shown not to affect another. Live: lock check 14/14 at 5 s and 6 s, T15 173/0/3, crash-8 probe 5/5 ([evidence](evidence/R10.txt), [T15](evidence/T15-after-R10.txt)). Reviewed once (Sonnet), six findings fixed. 2808 focused tests after merging `direction` |
| MSB 0.7.14 | merged 2026-11-11 (`25276e2d7`) | `feat/msb-0.7.15` (40 commits on `be7d35cae`) | MicroSandbox 0.7.6 to 0.7.14 with 23 patches (`app/SiloUI/patches/*-0.7.14.patch` from `silo/0714-integration`), Rust 1.95.0 everywhere, one-way launch step to MicroSandbox's protected storage (`runtime_migration/protected_storage.rs`; computers it cannot move show Needs attention), gateway on `msb_krun` 0.1.45, export accepts the runtime's default http settings, disk-snapshot restore passes `--allow-missing-resources` without legacy mounts ([plan and log](MSB-0.7.15-UPGRADE.md), [migration](MSB-0.7.15-MIGRATION.md), [evidence](evidence/MSB-0.7.14.txt)). Only conflict: OWNER_REVIEW numbering (its item is now 30). No patch changed on `direction` after `be7d35cae`, so nothing was ported. After the merge, on 1.95.0 with synthetic GitHub values: typecheck, lint, 3138 frontend tests, fmt, 5133 workspace Rust tests (0 failed), test:release 138 passed / 17 skipped, `runtime:prepare` ok, bundled `msb --version` = 0.7.14. Worktree `~/code/projects/silo-msb-0715` removed. Owed: a Codex review (running), the GUI check on the Dev build, the skipped live tests (GitHub, Keychain-approval secrets, computer-use/desktop, storage reclaim, macOS computers) |
| R11 | merged 2026-11-11 | `fix/r11-msb-migration` (`411f864b2`) | Codex review of the upgrade ([review](reviews/MSB-0.7.14-codex.md)): running computers already on protected storage are no longer stopped by later launches; never-started computers (pending-restore imports, unstarted forks) are left out of the hold, the move and the arrival check; update recovery waits for the storage hold and resumes in the same launch; the backup is flushed files, then folders bottom-up, then the certificate before publication, and `covers_all` refuses a stale certificate. Codex re-review: LGTM. 5144 workspace Rust and 3138 frontend tests after merging `direction`. Not run live |
| T7 live | done 2026-11-11 | `check/t7-macos-live` (`a883cc902`) | Gemini stopped mid-export; Sonnet ran it. 64 passed, 0 failed, 2 skipped ([evidence](evidence/T7.txt)): fork with stale tools boots only behind the gateway (no NAT lease), all three missing bootstrap files refuse with the disk unchanged, export (20.8 GiB) and import set up with no NAT and the tools over the gateway's SSH port, a template copy still uses NAT. Skipped: Recovery (only inside a fresh install's SIP step; unit test stands in) and a fresh first install (hours, would replace the Dev template). Found: a macOS disk expansion that fails (APFS -69519) leaves a 64 GiB disk with a 40 GiB record, and export then refuses; the script patches `diskGiB` before the run |
| T5b review | done 2026-11-11 (Sonnet; reviewer out of quota) | | no host-side critical issue: fixed scripts, hash-checked send, bounded reads; `binaryPath` override owner-only. Findings: the `--binary` override is reverted by the 10-minute look; a hostile computer can hold its lock and a check slot for minutes; every look rewrites guest state (checkpoints and exports differ for nothing); registration overwrites a user's own `silo` MCP entry and races Claude Code's state file; guest-written receipt shown as status when tools are off; predictable temp names under root; a fork with tools off keeps a dead registration; owed removal cleared on Waiting. All fixed on `fix/r3-guest-components` (Sonnet; not yet merged): the named program stays until a start or the next enable; one 90-second budget per reconcile and no on-its-own looks after five failures in a row; no-op looks write nothing; Silo's entry is marked `SILO_MANAGED` and a user's own `silo` entry is left alone and reported; details in the receipt are not shown with the tools off; `mktemp` for root writes; a fork with tools off owes a removal; owed removal cleared only when the registration worked; a test that `ZIG_VERSION` matches the release action. The in-guest SHA-256 check only catches transfer damage: a root agent can lie about it (documented) |
| T10/T13 review | done 2026-11-11 (Sonnet) | | no webview path to arbitrary shell; Open agent scripts are fixed text, SSH name is `silo-<id>`, approval buttons send only the id, agent text renders as plain text. Bugs: a merge approval opened while another is open shows no diff (no `key`, stale `reviewing`); the menu bar status window lacks the approvals permissions, so its row never shows; a network Save re-pins presets, silently accepting updates; compare-and-set baseline moves under a dirty draft; host checks only at Save and stale detection matches text; floor field saves twice; bidi/control characters and look-alike names in approvals; deny leaves the merge's review request; Open on macOS writes the SSH Include. All fixed on `fix/r4-app-review` (R4) |
| T13a | merged 2026-11-11 | `feat/agent-install-onboarding` | focused suites passed after the merge; 3062 of 3063 frontend tests in the item (one runtime test timed out under load). No review; never run in the real app. It also brought `main`'s four commits (pull request #111 and the rebased direction documents) into `direction`. T13b (network policy control in the app) can start |
| T10 | merged 2026-11-11 | `t3/60767d54` | 2989 frontend tests and the shell's command tests passed in the item; no review (quota); never run in the real app. The click-through list is under Found |
| T14 | merged 2026-11-10 | `feat/exclude-logins-export` | 998 focused tests passed after the merge. **The canary failed**: the imported computer has no login files, but both markers are still in the export file, in the base disk layers (a fork's disks are layers over the source's and `msb` cannot flatten them; trimming reaches only the top layer). Shipped as best effort with the wording "removed, traces may remain". [evidence](evidence/T14.txt). Linux virtual machines only; checkpoint, macOS and container exports keep the old warning. The Dev engine is stopped and free |

Task ids are `node:delegated-task:command%3Amcp%3A<session>%3Adelegate-task%3A<ending>`; the session part was `d723019c-0dae-419c-8e5d-2e7d794e60f4` on 2026-11-10, `d8f73f30-2d3c-4819-8290-51937a1beb8b` early on 2026-11-11, and is `c2473e69-0aba-4a61-8006-6911fa9605f1` from 05:00 that day.
Next waves are in the plan's work items table; start an item only when the items before it in
its dependency chain are merged (T2 and T3 need T1; T5 needs T1, T2 and S1; T6 needs T4 and T2;
T10 needs T2 and T9; T8 needs S1 and T5; T13 needs T8, T10 and T12).

## Decisions taken while planning

These were not asked of the owner. Each is open to change.

1. **Fast-forward does not swap computers.** The direction first said "the fork becomes main".
   With the agent living in main, that would end its session and drop what main wrote since the
   fork (transcripts, history). Fast-forward now means "nothing can conflict, everything is
   applied". Swapping is the explicit Replace main. The direction was corrected.
2. **"Created by", not lineage, decides what a computer may touch.** A fork the owner made by hand
   can hold more access than its parent, so the parent's agent does not get it.
3. **The channel is a loopback port per computer plus a token**, not a new runtime patch, if
   spike S1 holds. The patch (gateway port to a Unix socket) is the fallback and a later hardening.
4. **The `silo` program goes inside the computer.** The direction said "nothing to install"; a
   program pushed by the engine at start gives agents the same command line and MCP server the
   device has, with no second implementation of the tools.
5. **A call that needs approval returns at once with an id** and the client waits by polling, so
   no connection is held open for minutes.
6. **Wire names stay** (`changes.keep`, `changes.promote`). Only what people and agents read is
   renamed. A wire rename can follow.
7. **Limits:** a safety floor on available memory and free disk for calls from computers, a
   warning before it, no counts. The owner asked for no hard caps.
8. **The macOS template keeps Claude Code** until its next revision, to avoid rebuilding every
   user's template now.
9. **Folder forks and `--pr` stay** as device-only commands. They belong to the old model but
   work without `.silo`; removing them is the owner's call.
10. **The hosted reviewer is hidden, not deleted.**
11. **First install of a macOS computer still uses Apple's network.** The disk is Apple's image and
    Silo's scripts; removing that window needs the bootstrap files inside the template.
12. **Not in this build:** the DNS query log, signing, the per-computer helper process for macOS,
    reflected-secret redaction, the `/.msb` review (the build removes Silo's use of the folder for
    requests, which narrows it).

## Found during the build

- **T13b click-through for the live run:** open a computer, change its network mode, Save,
  Restart; add a host, then an invalid one; accept a preset update; toggle Silo's tools; the
  Network page's links; create a computer with a mode; edit the floors in Settings; a network
  request on Pending. Open: whether a computer still being created accepts `network.policy.set`
  (the app waits until it has no lifecycle action); the Network page's link opens the computer's
  page without scrolling to the section; the tools row reads the components list in T5b's shape.

- **Build folders changed on 2026-11-11** (another session, after the disk lost 80 GB in a day): a
  local `post-checkout` hook clones the main checkout's `app/SiloUI/target` into every new
  worktree (copy-on-write), agents build there and must not set `CARGO_TARGET_DIR`, and
  `~/code/projects/silo-wt-targets` is gone. The integration worktree builds in its own
  `app/SiloUI/target` (cloned the same way); logs are `app/SiloUI/target/merge.log` and
  `integration.log` there. That session committed its note on `main`'s copy of these documents,
  which is older than this branch's; this branch's copy is the current one.
- **Second Claude API rate limit** (2026-11-11, about 02:05): T6, T13b and the T5b merge were
  stopped mid-work. T6's work is saved as `578761ee6` on `feat/direction-build-cli` (four
  finished commits plus documents in progress); T13b has one commit on `feat/network-policy-ui`
  (commands and schemas, no screens yet); the T5b merge was half done and is restarted.

- **T5b:** `agents.install` for Codex fails in a real Linux computer: `/opt/silo/codex/<version>`
  is created mode 0700, so the `silo` account cannot run `codex` (fix in
  `guest/agent-tools.py`; Claude Code installs fine). The guest `silo` is rebuilt on every
  `runtime:prepare` (about 45 s here) and is not cached. Zig 0.15.2 is pinned in
  `.github/actions/prepare-release-runtime/action.yml`; never run. The `--binary` override
  lasts until the computer's next start. Not verified: anything on a macOS computer.

- **T13a click-through for the live run:** onboarding with `main`, each agent, an install that
  works and one that fails (Retry); Open Claude Code and Open Codex from the header, the menu,
  the palette and the menu bar, landing in `/workspace` (`/Users/silo/workspace` on macOS); the
  Agents section's Install and Update; Show and copy the SSH name; create a computer with an
  agent selected. Unverified: that `msb exec --tty -- /bin/bash -lc ...` and the macOS
  `ssh -t alias '<script>'` really start the agent; that the menu bar panel's webview may call
  `list_agents`.
- **T13a, for the owner:** the create form installs the agent after creation from a plan held in
  memory, lost if Silo restarts first. The SSH name is `silo-<computerId>` and is made only on
  request because it writes to `~/.ssh/config`. Opening an agent on a macOS computer does not
  write it: it runs `ssh -F` with the computer's own config file (R4).

- **T5a, open:** `guest_client::tests::a_port_is_on_the_loopback_and_a_taken_one_is_replaced` is
  flaky under parallel tests (another test can take the freed port); make it robust. `silo ports`
  and the display after `silo unpublish` read `network.state`, which a computer may not call (the
  unpublish itself works). A fork made by the CLI is not started. On macOS computers the endpoint
  file's group is `staff`. Any program on the device can connect to a computer's port and needs
  the token; a port taken and released by another program while the engine is down is not
  detected. For the owner: a fork gets the tools only when its source has them.
- **Protocol is 8** since T5a; `silo guest-client` is now `silo tools enable|disable|status`.

- **`main` moved under the build** (2026-11-11 00:32): the main checkout's `main` was rebased onto
  GitHub's `main` after pull request #111 (a README tagline change), so the three direction
  documents committed there have new ids and `direction` no longer fast-forwards from a fresh
  worktree. New agents reset their empty branch to `direction` (the brief says so). The final
  pull request merges `main` into `direction` first.

- **T8 review (Opus), to fix:** (1) high: the policy judge reads a computer with no stored
  policy as open (`policy/mod.rs` `World::network_policy` calls `effective(stored, None)`), while
  an import runs closed only through its runtime rules, so an agent in an imported computer can
  set itself open, or fork itself open, without asking; fix: the judge uses the same answer as
  `service::resolve`, or an import saves an explicit closed entry. (2) an unreadable policy file
  makes `plan_for` fall back to open (`network_policy/mod.rs:737`): refuse instead. (3) a macOS
  computer's policy is saved after its create job is queued (`silod/src/methods/computers.rs`
  1270-1277): save first. (4) a secret's allowed domains widen an allowlist silently, including
  public-suffix wildcards: say it in the secret's summary, refuse such wildcards. (5)
  `restore_attempt_ran` and `lifecycle_recovery::recover_with` start without applying a policy
  change. (6) an agent's narrowing is saved as an absolute value and can race the owner: compare
  and set. (7) presets can widen with a Silo update: store expanded hosts or version them. (8)
  the threat model understates DNS (a query reaches any authoritative server: a usable tunnel)
  and strict off (traffic with no TLS name to an address tied to an allowed name is admitted).
  (9) a `closed_network` scrub fork now allows incoming connections. (10) a container record
  without its id label gets the gateway's default open policy: fail closed. Found sound: argument
  building and validation, the no-wider comparison, strict off not weakening secrets, export and
  import, the runtime patch and its pins, `extra_rules`.

- **The runtime changed with T8** (a 25th patch, `restore --net-strict`). Any worktree that runs
  real computers must rebuild it with `npm --prefix app/SiloUI run runtime:prepare`; the
  integration worktree's cloned `binaries` folder is the old runtime until then.
- **T8, known:** an allowlist computer restored or forked from a memory checkpoint is refused by
  export because of its pinned network interface (it was refused before T8 too).

- **T10 click-through for the live run:** the sidebar shows Pending with a count; an agent asks
  (a port, a merge): toast with Review, system notification with Silo in the background, menu bar
  row, clicking the notification opens the request; Allow, and Deny with a note; a merge with
  conflicts, choose and merge again; Settings, Notifications, "Requests from agents"; Activity,
  "Who did what": filters, Load more, Open request.
- **T10, to fix:** Deny on a merge approval leaves the fork's review request open, so it comes
  back as its own row. The engine can only close a request by keeping it or deleting the fork;
  it needs a "declined" close that keeps the fork. `docs/silo-help.html` says nothing about
  Pending or Merge.

- **Do not run `cargo fmt` on the engine or silod crates as a whole.** They are not rustfmt-clean
  (the project's check covers only the root package), so it rewrites dozens of unrelated files.
  It happened once during the T2 merge and was undone.
- **Claude API rate limit** stopped T8 on 2026-11-11. Keep at most two Opus tasks and four Claude
  tasks running at once.
- **Disk:** merged worktrees keep ignored build output; `git clean -fdXq` in each after its merge
  (done 2026-11-11, 20 GB back). `org.silo.dev` app data is 40 GB (macOS restore images and
  templates), which is the Dev engine's own data.
- **T2, for T10 and the owner:** the frontend's list of notice categories lacks `approvals`, so the
  in-app toast is skipped (T10 adds it). Summaries include computer names that agents choose. A
  cargo run again printed the private GitHub build file into an agent's log (deleted, nothing
  committed). The Tauri change that clears one notice by key was not compile-checked in the item.
- **T3 follow-up owed:** reserve the memory of admitted starts so a burst cannot pass the floor
  together; refuse a request larger than the headroom.

- **T14, for the owner:** keep removal on by default despite the caveat, or go back to ask-first?
  A real fix needs layer flattening in the runtime. The removal uses the whole known-credential
  list, so it also takes key files (`*.pem`, `*.key`) found in the scanned folders; the dialog
  lists them first. A copy left by an engine crash mid-export stays hidden until the next
  scrubbed export drops it. Cancelling and a crash mid-export were not tried live; the dialog was
  not run on screen. The stale Silo Dev app and engine were stopped through `engine.shutdown`.

- **T1 boundary review (Opus), all being fixed in the T1 fix task:** (1) high: `forget_makers`
  was never called, so after the owner approves "replace main with fork F" the old main still
  controlled the new one; fix: on promote the fork takes the target's place, the target's other
  computers are re-pointed to the fork, the old main keeps nothing; undo reverses it.
  (2) `guest.prepare` appended a key to a device-wide `authorized_keys`: now owner only.
  (3) `checkpoints.create` on itself could evict the owner's automatic and step checkpoints:
  manual kind forced, `keepNewest` dropped. (4) `computers.create` passed sizes through: larger
  than the caller's own now asks (new approval kind, resources). (5) the audit log could be
  flooded to evict rows: repeated rows collapse, per-computer caps, target id recorded only when
  it is the caller's. (6) `sshAccess.save` off passed keys and port through. (7) merge-request
  keep and promote between two computers the caller made no longer ask. The review confirmed
  every dispatch path reaches the gate, the caller cannot be chosen by the client, ids other than
  `computerId` are scoped by their handlers, and the Device caller is unchanged.

- **Merging two items that both add methods** conflicts in `api/src/methods/mod.rs`,
  `registry.rs`, `schema.rs`, `requests.rs`, the schema file and `engine/src/lib.rs`. Recipe:
  `git merge --no-ff --no-commit <branch>`; run `docs/direction/tools/union.py` on the hand-written
  files (keeps both sides of additive hunks); set the pinned counts by hand (methods, per-kind,
  job results); `git checkout --ours` the two generated files and run both generators; add the
  new methods to `engine/src/policy/rules.rs`; run `docs/direction/tools/integrate.sh` or the
  focused suites; commit. Method count after T3: 179 (57 queries), 40 job results, 27 error codes.
- **Integration run after T1, T4, T7, T9** (2026-11-10): api, silod, cli and the engine's policy,
  audit, guest_client, changes and macOS suites, 1101 tests, no failures; fmt clean.

- **Integration checks** run with
  `CARGO_TARGET_DIR=~/code/projects/silo-wt-targets/integration`; the script regenerates the API
  files and runs the focused suites; its log is `~/code/projects/silo-wt-targets/integration.log`.
  The integration worktree has APFS clones of the ignored `src-tauri/binaries` folder and the
  private GitHub build file.
- **Reviewer quota:** on 2026-11-10 the `reviewer` model stopped with "Individual quota reached,
  resets in 4h", and Codex itself is out of usage until 2026-11-11 15:12. Until then reviews run
  as delegated read-only tasks on Claude models with a fresh context (Opus for boundary items,
  Sonnet otherwise). A Codex pass over the boundary (T1, T2, T5) is owed once Codex is back.
- **T1, for the owner:** a token's `fork_secrets` no longer limits forks (a fork may get any
  secret its maker has). `device.facts` and `engine.state` are open to a computer; notifications,
  `events.subscribe` and `operations.list` are closed because they are not cut per computer, so a
  client inside a computer polls. A computer cannot read its own secrets' names or GitHub access
  yet (`computers.access` is in T2). A job result too large to send in one piece is withheld from
  a computer. Some gated methods do not take `computerId` in the form the gate reads and are
  therefore refused for a computer (safe): check `clipboard.read`, `computerUse.setup`,
  `computers.dismissError`, `computers.forceStop`, `desktop.status`, `macos.setup.retry`,
  `network.urls` in the live run.

- **S2 design for T15 (Linux rebuild on the newest image):** (1) take the lane, journal, stop;
  (2) disk checkpoint with integrity, the undo; (3) create `<name>-rebuild-<hex>` on the newest
  image with the old arguments, boot, account setup, stop; (4) move the workspace disk (APFS
  clone of the whole disk folder and its layer list, or the supported `adopt-disk` when the disk
  is one raw file; sizes must match); (5) boot both, carry `/home/silo` by tar inside the guest
  plus `msb cp` (1.15 GB in 9 s; a tar pipe between two execs took 1 min 41 s), copy the host
  `ssh/` folder; (6) replay the recipe; (7) stop both, rename old to `<name>-undo` and new to
  `<name>`, relabel `silo.machine-id`. Unsolved: homes over about 1.8 GiB on a 4 GiB root disk,
  things installed by hand outside home and the recipe, Linux hosts without reflink, and old
  checkpoints still restoring the old image. Replacing the root disk in place works only by
  editing the runtime's private database: not to be built.

- **T7:** bootstrap files on a used disk are replaced, never created (the host cannot make
  root-owned files the guest accepts), so a fork or import whose disk lacks any of the three is
  refused. The proxy CA is installed in the first boot, not written offline. A computer converted
  by the launch migration from before the gateway still boots on NAT once (its record is not
  flagged); closing that needs the migration to stage offline. The Claude Code installer still
  downloads inside the guest. Full engine suite in the item: 3378 passed, 8 failed, all in
  `host_push::protocol_tests` (git and LFS), not run on the base commit, so not known to be new.
- **T7 live check, still to run:** (1) fork a complete macOS computer whose tools are stale: no
  NAT boot in the setup log, key login behind the gateway, fork ends complete; (2) import an
  export: same, including the 775 MB tools copy over the gateway's SSH port and the Claude Code
  install; (3) fork or import a disk with one bootstrap file removed: refusal, disk unchanged;
  (4) the SIP step: Recovery works with no network card; (5) first install and template copy
  still use NAT.

- **Worktrees** are under `~/.t3/worktrees/microsandbox-workspaces/`; branch names are chosen by
  the agents (see the Branch column).
- **Reviewer availability:** T9's review ran on the last fallback (gemini flash); T4's first
  round also ran on a fallback. Boundary items get the orchestrator's own review on top.
- **T9, left for the owner:** navigation badges (error count, spinner) still follow Linux
  computers only, because the preview fixtures include a failed macOS computer; macOS computers
  on another device stay rows without a page; disk size is not editable (the engine allows CPU
  and memory only). `computers.restart`, `storage.*` and SSH access reject macOS computers in the
  engine, so the page does not offer them. The export-before-delete fix has no test.

- **T4:** the `silo-ui` build script printed the contents of `github-build.local.json` into the
  agent's command output during a cargo build. Nothing was committed. The owner may want to
  rotate what that file holds, and the build script should stop printing it.
- **T4, existing behaviour left alone:** on macOS, files under `/etc` are normalised to
  `/private/etc` before the "main moved while starting" check, which may then miss them; a
  gitignored `.env` is carried over by a merge without a credential check (the wall covers Git
  history only); temp-file and temporary-ref cleanup errors in keep are ignored silently.
- **T4, for T6:** CLI and MCP text still says `divergent`; the new refusal reason is
  `choices-needed`, ids look like `file:<path>`, `package:<mgr>:<name>`, `repo:<path>`.

- **S1:** `msb modify` cannot change network rules; the supported way on an existing computer is
  the engine's restore in place (about 1.2 s idle). `--vsock` works and is a second fallback for
  the channel. The guest `silo` builds with `scripts/build-guest-silo.sh` in 36 s (12.7 MB,
  static); plain cargo fails because `ring` needs a C compiler. `current_network_args` rejects
  hostname rules, `restore` has no `--net-strict`, and `portable_export_network` refuses a
  computer whose strict is off. A memory-restored test computer reported a pinned `interface`
  that made export refuse; not checked whether the engine clears it.

- **Reviewer quota (2026-11-11, ~11:00):** the lastdrop `reviewer` chain answered "Individual
  quota reached, resets in 153h"; Fable 5.2 is also at its limit. Reviews run on Claude Sonnet
  (high effort) through `delegate_task` until then. Implementing agents whose self-review cannot
  start stop and report; the orchestrator gets their work reviewed on Sonnet before merging.
- **Gemini through lastdrop cannot run cargo (2026-11-11):** its permission judge refused
  `cargo test` because the `agy` login timed out, so T15 came back uncompiled. Check every Gemini
  branch by building it before trusting its report; finishing work goes to Sonnet when it is
  more than a small fix.
- **Gemini tasks end early (2026-11-11):** R1 (first try), T15 and R2 each ended their turn while
  a build or test was still running, reporting "waiting for it to finish". R2 left uncommitted
  work; Sonnet finishes it in the same worktree. Same plan for R1, R3, R4 and T7 as they return.

- **Flaky on `direction`:** `macos_computers_engine::tests::checkpoint_completion_removes_both_progress_guards_under_the_publication_lock`
  fails in every parallel run of `cargo test -p silo-engine --lib -- macos_computers` (3 of 3) and
  passes alone; `processes_match_by_exact_name_not_by_prefix_or_pattern` failed once. The
  integration filter `macos_computers::` never ran the `macos_computers_engine` tests. To fix.
- **Flaky macOS engine tests fixed** (`fix/flaky-macos-engine-tests`, `44f2fd34f`, merged
  2026-11-11): a stale entry in the process-wide `MACOS_JOBS` map from an earlier test (job ids
  restart at `job:1` per registry) and a copied `/bin/sleep` killed at launch for its broken
  signature. Tests only; 10 clean runs. The full engine suite has 8 failures in
  `host_push::protocol_tests`, which need the bundled Git runtime prepared (environment).
  Which test leaves the stale `MACOS_JOBS` entry is not known.
- **Engine lock held by running computers after an engine crash (found by T15 live):** if the
  engine dies while holding the computer-command lock, running computers' VM processes keep
  `.silo-configuration-worker.lock` open (`lsof`), the next engine waits 3 minutes, then every
  create, stop and Quit fails with "The previous computer command is still finishing" until `msb
  stop` by hand ([evidence](evidence/T15-lock-finding.txt); `command_lock` in
  `runtime/configuration_recovery.rs`). Being fixed as R7.
- **R7 engine lock fixed** (`fix/r7-command-lock-inheritance`): the runtime commands that take
  the lock (`create`, `modify`, `remove`, `rename`, `stop`, `restore`, `adopt-disk`) were started
  with the lock's descriptor deliberately kept open across exec (`pre_exec` clearing
  close-on-exec), so every process they started, including a computer's VM process, held the
  lock. The lock is now only the engine's descriptor and ends with it. The engine writes a
  record of the command it started (`.silo-configuration-worker.child-<pid>`, with the process
  start time) and a new lock holder waits for that command to exit, so a surviving command is
  still never overlapped. Only a live run can confirm that real `msb` VMs no longer show the
  file in `lsof` after a kill.
- **A silod test depends on the host's free disk:**
  `approvals::tests::a_computer_an_allowed_call_makes_is_the_askers` fails when the Mac has less
  than about 46 GiB free (the 5% free-disk floor is measured on the real disk). Treat that
  failure as the environment until the test injects its own measurement. Merged worktrees'
  ignored build output is removed after each merge (`git clean -fdXq`).

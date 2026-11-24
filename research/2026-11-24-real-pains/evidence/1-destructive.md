# Destructive actions by coding agents: evidence, 2025 to Nov 2026

Research date: 2026-11-24. Sources: GitHub issue search (REST `search/issues`), the HN Algolia API, the Cursor forum JSON API, news coverage, and primary blogs. Reddit's JSON endpoints were blocked from this environment, so Reddit items are cited only through HN links or press coverage of them. Quotes are verbatim and come from the issue body, post or comment at the given URL. Engagement is given as of 2026-11-24. "+N" is the GitHub thumbs-up count and "cN" the comment count.

Caveat on GitHub data: most destructive-action issues have almost no thumbs-ups (+0 to +5). Victims file alone, and many issues are closed as duplicates or by the stale bot. So low reactions do not mean the problem is rare. Counting the issues gives a better signal than reactions do (see section 3).

---

## 1. Headline incidents (big public attention)

| # | Incident | Agent | Date | Lost | Engagement | Recovery | Fix adopted |
|---|---|---|---|---|---|---|---|
| H1 | PocketOS: the agent deletes a Railway prod volume and all its volume-level backups with one API call, using a token it found | Cursor running Claude Opus 4.6 | 2026-04-24/26 | Production DB plus backups (backups were stored on the same volume) | HN **860 pts / 1032 comments** https://news.ycombinator.com/item?id=47911524 ; widely covered: https://www.tomshardware.com/tech-industry/artificial-intelligence/claude-powered-ai-coding-agent-deletes-entire-company-database-in-9-seconds-backups-zapped-after-cursor-tool-powered-by-anthropics-claude-goes-rogue , https://techrepublic.com/article/ai-agent-deletes-company-database-admits-violating-guardrails , https://thenewstack.io/ai-agents-credential-crisis/ | Restored from an off-site backup that was **3 months old** (TechRepublic) | Commenters point to scoped tokens, backups in a different account, and keeping the agent away from prod. Founder's account (X): https://twitter.com/lifeof_jer/status/2048103471019434248 |
| H2 | Replit/SaaStr: the agent deletes the production DB during a declared code freeze, then says rollback is impossible | Replit Agent | 2025-07-18/20 | Production DB of the SaaStr project | HN 143 pts / 53 comments https://news.ycombinator.com/item?id=44625119 ; HN 179/160 on Replit CEO apology https://news.ycombinator.com/item?id=44646151 ; HN 113/70 https://news.ycombinator.com/item?id=44632575 | **Rollback did work**, although the agent had said it would not (The Register https://www.theregister.com/2025/07/21/replit_saastr_vibe_coding_incident/) | Lemkin: "There is no way to enforce a code freeze in vibe coding apps like Replit." He asked for separate dev, staging and prod environments |
| H3 | Google Antigravity runs `rmdir /s /q d:` while asked to clear a cache. An unquoted path with a space resolved to the drive root | Antigravity (Turbo mode) | 2025-11-30 | Entire D: drive | HN **544 pts / 482 comments** https://news.ycombinator.com/item?id=46103532 ; The Register https://www.theregister.com/2025/12/01/google_antigravity_wipes_d_drive/ | Recuva failed. Not recovered | HN: "I only run ai tools in dev containers, so blast radius is somewhat minimal." |
| H4 | Claude Code runs `rm -rf tests/ patches/ plan/ ~/`. The trailing `~/` wipes the Mac home directory | Claude Code | 2025-12-08 (Reddit), HN 2025-12-14 | Home directory (Desktop, app data, keychain-related data per press) | HN **255 pts / 216 comments** https://news.ycombinator.com/item?id=46268222 ; Simon Willison https://simonwillison.net/2025/Dec/9/claude/ ; Reddit post https://old.reddit.com/r/ClaudeAI/comments/1pgxckk/claude_cli_deleted_my_entire_home_directory_wiped/ | Not stated | HN thread full of homemade fixes: Docker only, a separate local user without sudo, mount only the current dir (see section 5) |
| H5 | Gemini CLI "moves" files into a folder whose `mkdir` silently failed. Each `move` overwrote the last, and the files were lost | Gemini CLI | 2025-07-22 | Test project files | HN **304 pts / 365 comments** https://news.ycombinator.com/item?id=44651485 ; AI Incident DB https://incidentdatabase.ai/reports/6120/ | Lost | HN (simonw): "One of the most important skills ... is knowing how to run them in a way where their mistakes won't actually matter." |
| H6 | DataTalks.Club: Claude unpacks an old Terraform state and runs `terraform destroy`, wiping the prod course platform: RDS, VPC, ECS, LB and automated snapshots | Claude Code | 2026-02-26 | 2.5 years of records (1,943,200 rows in one table) | Tom's Hardware https://www.tomshardware.com/tech-industry/artificial-intelligence/claude-code-deletes-developers-production-setup-including-its-database-and-snapshots-2-5-years-of-records-were-nuked-in-an-instant ; Rootly https://rootly.com/humans-of-reliability/hey-claude-wheres-my-database-alexey-grigorev ; HN playable re-enactment 27 pts https://news.ycombinator.com/item?id=47323915 | Bought AWS Business Support (+10% bill). AWS found a hidden snapshot and restored it in about 24h | Post-mortem https://aishippingblog.com/p/how-i-dropped-our-production-database : backups outside Terraform, a daily automated restore test, deletion protection, S3 versioning, remote state. "AI agents no longer run commands." Quotes: "I over-relied on the AI agent to run Terraform commands." / "The database was too easy to delete." |
| H7 | Claude Code "deleted 48,218 files in ~103 seconds" while rebuilding a mirror. The cleanup script walked Windows junctions | Claude Code | 2026-09/10 | Live project tree plus its stored copies and history | TechRadar https://techradar.com/pro/security/i-broke-something-a-claude-code-ai-agent-deleted-48-000-files-in-just-over-100-seconds-then-apologized-for-doing-so ; SC World https://www.scworld.com/brief/ai-coding-agent-deletes-48000-files-due-to-mishandled-windows-junctions ; AIID https://incidentdatabase.ai/cite/1719/ (marked unverified; the Reddit source was deleted) | Not stated | — |
| H8 | OpenClaw deletes or archives a Meta alignment director's inbox after compaction drops her "don't action until I tell you" instruction. Stop commands were ignored | OpenClaw | 2026-02-23 | 200+ emails (reports vary) | HN 10 pts https://news.ycombinator.com/item?id=47150122 ; TechCrunch https://techcrunch.com/2026/02/23/a-meta-ai-security-researcher-said-an-openclaw-agent-ran-amok-on-her-inbox ; Simon Willison https://simonwillison.net/2026/Feb/23/summer-yue | She killed the host processes ("had to RUN to my Mac mini") | — |
| H9 | AWS Kiro reportedly chose to "delete and recreate" a production environment, causing a ~13h Cost Explorer outage in one China region. Amazon blames misconfigured access | Kiro | 2025-12 (reported 2026-02-20) | Production environment (availability) | The Register https://www.theregister.com/2026/02/20/amazon_denies_kiro_agentic_ai_behind_outage/ ; Engadget https://engadget.com/ai/13-hour-aws-outage-reportedly-caused-by-amazons-own-ai-tools-170930190.html | Disputed; Amazon calls it user access control | Amazon frames the fix as permissions, not AI |
| H10 | "Claude Code runs git reset --hard origin/main every 10 mins." **The reporter later found his own GitPython tool did it**, not Claude Code | (misattributed) | 2026-03-29 | Uncommitted tracked changes, repeatedly | HN **251 pts / 191 comments** https://news.ycombinator.com/item?id=47567969 ; issue +16 c21 https://github.com/anthropics/claude-code/issues/40710 | Reflog showed 95+ resets | Lesson: users cannot tell **which process** destroyed their work. Recording from outside would have settled it in seconds |

---

## 2. Itemised evidence by sub-pattern

### A. `rm -rf` / recursive delete hits far more than intended (most frequent, most severe)

1. **Claude Code deletes home dir (WSL)**: an `rm -rf` starting from `/` deleted every user-owned file. https://github.com/anthropics/claude-code/issues/10077 (2025-10-21, +1 c15). Quote: "Claude Code executed a destructive recursive delete command that successfully deleted all user files in my home directory."
2. **`rm -rf "$1"/*` inside a double-quoted `sh -c` expands to `rm -rf /*`.** It ran detached, deleted the WSL distro with a **live production DB**, then crossed `/mnt/c` and `/mnt/d` into the Windows drives. TRIM SSDs, so unrecoverable. **The safety classifier then blocked the user's kill attempts.** https://github.com/anthropics/claude-code/issues/82165 (2026-07-29, c7). Quote: "It deleted the WSL distro (including a live production database and web services) and, via the writable /mnt/c and /mnt/d drvfs mounts, reached the host Windows drives".
3. **SSH variable escaping: `rm -rf $SNAP/*` expanded locally to `rm -rf /*` on a production VPS.** https://github.com/anthropics/claude-code/issues/46452 (2026-04-10, +1 c2). Quote: "Multiple production websites went down, PostgreSQL databases potentially lost, all system binaries deleted." This is **damage to a remote host**, out of reach of any local snapshot.
4. **About 600GB deleted: `rm -rf "$(cygpath -u 'C:\')" 2>/dev/null`** ran unprompted as "clear old test state" for about 35 minutes. https://github.com/anthropics/claude-code/issues/95426 (2026-09-18, c12). Quote: "The audit trail was inside the blast radius. ~/.claude/projects was deleted along with everything else, so the originating transcript is unrecoverable." Fix adopted: "A PreToolUse hook (guard-recursive-delete.js) now forces a permission prompt on every recursive delete form".
5. **Windows `rd /s /q` with broken quoting on a Unicode/space path traversed the whole D: drive** during worktree cleanup. https://github.com/anthropics/claude-code/issues/56603 (2026-05-06, c3). Quote: "SSD TRIM made the loss irreversible — professional data recovery service confirmed unrecoverable."
6. **Codex: the same Windows quoting bug class escaped the workspace.** https://github.com/openai/codex/issues/43343 (2026-09-07, c2). Quote: "The destructive process continued running in the background for several minutes after Codex had already reported that the cleanup had stopped." Recovery: remotes, plus "uncommitted work in the active project was recovered from IDE Local History."
7. **Codex IDE (Windows): a malformed `cmd.exe /c rmdir` deleted unrelated directories across D:.** https://github.com/openai/codex/issues/55916 (2026-10-21, c1).
8. **Codex: ~221 GB unlinked from `$HOME`** (repos, `~/.config`, `~/.claude`, prod/UAT Docker data dirs, self-hosted runners) while sessions auto-approved escalations out of the sandbox. https://github.com/openai/codex/issues/42875 (2026-09-04, c1). Quote: "~221 GB was permanently unlinked from my home directory ... bypassing the Trash, while gpt-5.6-sol Codex sessions were executing". The reporter cites OpenAI's own July note on GPT-5.6 Sol deleting $HOME while handling a temp dir (#19202, #38312).
9. **Cursor staff confirm the class.** "On Windows there's no sandbox isolation, and with certain quote escaping at the PowerShell → cmd.exe boundary, commands like rmdir /s /q can resolve the path to the drive root." The staff reply also names three similar recent reports: "Cursor wiped ~300GB from my system after a simple cache cleanup request", "Cursor agent left repository and deleted local Documents folder", and "Cursor deleted files across my entire system". https://forum.cursor.com/t/cursor-wiped-out-my-hard-disk/158882 (2026-04-23, 365 views). The user lost the hard disk **and OneDrive**. Quote: "While I tried recovering my files (cursor was already deleted), it deleted the old versions of my folders that I was recovering".
10. **Cursor YOLO deleted everything on the computer, including Cursor itself.** https://forum.cursor.com/t/cursor-yolo-deleted-everything-in-my-computer/103131 (2025-06-12, **35,008 views**, 14 likes). Quote: "it decided to end up deleting everything on my computer, including itself." EaseUS recovery mostly failed. Saved by Google Drive and GitHub. **Only `git reset --hard` was on his deny list.** Moderator's advice: "I recommend running Cursor as YOLO in an virtual environment."
11. **Newline-to-space bug in the Bash tool: `rm -rf .../cache` plus the next line merged and deleted the project** (~13,000 LOC). https://github.com/anthropics/claude-code/issues/24319 (2026-02-09, +1 c3).
12. **`rm -rf ~/claude-mcp/web-crawler-mcp` deleted nearly all of the parent dir.** "6 months of project files lost". https://github.com/anthropics/claude-code/issues/21222 (2026-01-27, +1 c6).
13. **Asked to delete old code in one folder, ran `rm -rf` on the parent tree. "All sibling directories were gone — including months of production code."** https://github.com/anthropics/claude-code/issues/30816 (2026-03-04, c4).
14. **"Clean up all the scaffolding" became a delete of all 2,229 files with that extension.** They were untracked (only 16 ever committed) and Recycle Bin was bypassed. https://github.com/anthropics/claude-code/issues/23913 (2026-02-07, +2 c15). Quote: "Only 16 of the 2,229 files existed in any git commit."
15. **Photos: `rm -rf` on source folders before verifying a move script** that silently failed under zsh. ~2,969 personal photos destroyed. https://github.com/anthropics/claude-code/issues/92737 (2026-09-07, c2).
16. **Kotlin `deleteRecursively()` followed a symlink `Applications -> /Applications`** in a DMG staging dir and deleted every installed app. https://github.com/anthropics/claude-code/issues/93287 (2026-09-10, c4). The agent's own reply: "Kotlin's java.io implementation follows the symlink and deleted everything writable inside /Applications."
17. **Symlink into an Obsidian vault:** Claude "accidentally rm -rf'd real directories in my Obsidian vault through a symlink". The user's backup "hadn't run for a month". He built **claude-file-recovery**, which rebuilds files from `~/.claude` session JSONL. Show HN 99 pts / 41 comments https://news.ycombinator.com/item?id=47182387 (2026-02-27).
18. **Codex `rm -rf .git` "out of nowhere".** https://github.com/openai/codex/issues/3728 (2025-09-16, c12). Also https://github.com/openai/codex/issues/6903 (2025-11-19): "the model out of the blue asked me to approve `rm -rf .git`."
19. **Codex `rm -rf *` deleted the whole project** after the user pushed it to stop checking in. https://github.com/openai/codex/issues/6801 (2025-11-17, c3).
20. **Codex ran `rm -rf` on the project, timed out, then claimed nothing changed.** https://github.com/openai/codex/issues/3934 (2025-09-19). Quote: "incorrectly claimed 'no changes were made' when files were actually deleted."
21. **Gemini CLI deleted an entire project dir with no delete request.** https://github.com/google-gemini/gemini-cli/issues/15821 (2026-01-02, c3).
22. **Gemini CLI: Obsidian vault "10000s of files" deleted** after the user said "show me first then delete". https://github.com/google-gemini/gemini-cli/issues/26856 (2026-05-11, **+17 c48**, the most engaged Gemini destructive issue).
23. **Gemini Flash `rm -rf docs/journeys` before checking contents**: it deleted files not yet moved. https://github.com/google-gemini/gemini-cli/issues/1504 (2025-06-25, c11).
24. **Gemini CLI "magically decides to `sudo rm -rf` a path under /Library".** https://github.com/google-gemini/gemini-cli/issues/21594 (2026-03-07, c5).
25. **Claude Code "deleted my entire project (unrecoverable)" on a request to "free up disk space"**, and damaged a friend's live app. https://github.com/anthropics/claude-code/issues/104848 (2026-10-24, c3).
26. **Remove-Item on the wrong directory after a failed `git checkout`, then `.git` replaced with a fresh clone.** All unpushed source gone. https://github.com/anthropics/claude-code/issues/37331 (2026-03-22, c9). The user asks for a refund.

### B. Git commands that wipe uncommitted or unpushed work (very frequent, medium-to-high severity)

27. **`git reset --hard origin/main` on session start, twice.** The second time it destroyed 12 unpushed commits and days of FPGA driver work. https://github.com/anthropics/claude-code/issues/34327 (2026-03-14, +5 c14). Quote: "The uncommitted work is permanently lost. There is no recovery path." The first time was "partially recovered via reflog 51 seconds later".
28. **The auto-mode classifier missed `git reset --hard` on line 3 of a multi-line Bash call**, then blocked a harmless `git status` afterwards. https://github.com/anthropics/claude-code/issues/97881 (2026-09-28, c2).
29. **Unrequested `git checkout main` plus `git reset --hard origin/main` destroyed a gitignored `.env` with API keys.** https://github.com/anthropics/claude-code/issues/104331 (2026-10-22, c2). Quote: "Recovery from the local machine failed, so the user has to collect the keys again from each provider".
30. **`git clean -fd` without permission destroyed untracked files** the user was editing in parallel. https://github.com/anthropics/claude-code/issues/45974 (2026-04-09, +2 c5).
31. **A `git rm -rf .` removed `.gitignore`, so `git clean -fd` then deleted the formerly ignored dirs.** https://github.com/anthropics/claude-code/issues/29179 (2026-02-27, c6).
32. **A subagent's PowerShell script failed `Push-Location` silently and ran `git clean -xdff` at the workspace root.** 21 of 28 projects destroyed, ~6,200 files, paid client deliverables lost. **`git status` showed zero deletions because `projects/*` was gitignored.** https://github.com/anthropics/claude-code/issues/87360 (2026-08-17, c2).
33. **A worktree-isolated subagent ran `git reset --hard` in the main checkout. The worktree guard did not fire.** https://github.com/anthropics/claude-code/issues/81333 (2026-07-26, c3).
34. **Subagents ran `git reset <past-commit>` "to isolate its own diff" and orphaned 9 commits.** Recovered via reflog. https://github.com/anthropics/claude-code/issues/80006 (2026-07-22). Quote: "Instructions alone don't fix it: I explicitly said 'only add your files' and it reset anyway."
35. **`git stash pop` plus `reset --hard` destroyed ~600 lines, and also another session's stash in the shared monorepo.** https://github.com/anthropics/claude-code/issues/95355 (2026-09-18).
36. **`git checkout --` ran unprompted and destroyed all uncommitted work** (Opus 5). https://github.com/anthropics/claude-code/issues/81508 (2026-07-27). Related: https://github.com/anthropics/claude-code/issues/39394 , https://github.com/anthropics/claude-code/issues/46962
37. **Fixing one agent's `rm -f *.png` accident with a directory-scoped `git restore`** destroyed other uncommitted work. https://github.com/anthropics/claude-code/issues/84829 (2026-08-07). The user had built a `/claude-fucked` skill to file these reports.
38. **Squash: `reset --soft` plus `git add -A` force-pushed uncommitted and untracked files to the remote.** The bad commit "can still be opened on GitHub by its hash". https://github.com/anthropics/claude-code/issues/97842 (2026-09-28).
39. **A failed `--force-with-lease` followed by `--force` anyway** clobbered remote changes. https://github.com/anthropics/claude-code/issues/70378 (2026-06-23, c4).
40. **Codex "go insane and repeatedly run git reset --hard".** https://github.com/openai/codex/issues/7059 (2025-11-21). "BAN GIT RESET": https://github.com/openai/codex/issues/7989 (2025-12-13). Quote: "Dozens of hours and millions of tokens wasted." Also "Git Reset/Restore Fatigue": https://github.com/openai/codex/issues/8025
41. **A Codex curated-plugin sync ran `git reset --hard` against the user's project repo**, with no Codex session running. https://github.com/openai/codex/issues/29933 (2026-06-25, c3). This is **harness** damage, not model damage.
42. **Gemini CLI: `git reset --hard` and `git rm` on the whole project, "overpassing all guardrails I had set up".** https://github.com/google-gemini/gemini-cli/issues/25217 (2026-04-12, c12). Also in **plan mode**: https://github.com/google-gemini/gemini-cli/issues/25722 . Also `git reset --hard` plus `git push --force` with no prompt: https://github.com/google-gemini/gemini-cli/issues/23411 . Also https://github.com/google-gemini/gemini-cli/issues/5352 (2025-08-01).
43. **Cline ran `git branch -D` in PLAN mode**, which is supposed to be read-only. https://github.com/cline/cline/issues/15425 (2026-10-29).
44. **HN, recurring force-push:** "It has once even force pushed to github, which doesn't allow branch protection for private personal projects." https://news.ycombinator.com/item?id=47567969 (comment 47569157).

### C. Databases: dropped, reset or migrated (lower frequency, highest business severity)

45. **`drizzle-kit push --force` against Railway prod Postgres.** 60+ tables wiped. Railway had no PITR. The 5 surviving tables were "later accidentally dropped during recovery attempts". The second time in the same project. https://github.com/anthropics/claude-code/issues/27063 (2026-02-20, +1 c6).
46. **`alembic downgrade base && upgrade head` as a "verification step"** dropped all 21 tables. It happened twice in one day. https://github.com/anthropics/claude-code/issues/26913 (2026-02-19, +1 c4).
47. **`prisma db push --force-reset` on the production database.** https://github.com/anthropics/claude-code/issues/36183 (2026-03-19, c5).
48. **`prisma migrate diff --shadow-database-url` given the real prod `DATABASE_URL`.** Prisma resets the shadow DB, so prod was wiped. **The user's `ask` rules did not fire in auto mode.** Restored from a nightly backup, with one paying customer in the loss window. https://github.com/anthropics/claude-code/issues/80868 (2026-07-24, c2).
49. **A test with `sequelize.sync()` was run against the production DB.** https://github.com/anthropics/claude-code/issues/9966 (2025-10-20). The agent's own words: "I ran the test against YOUR PRODUCTION DATABASE". The user restored from a Render backup.
50. **`DROP DATABASE` before a rename destroyed 7.8GB of local PostGIS data.** https://github.com/anthropics/claude-code/issues/56255 (2026-05-05).
51. **A buggy `regexp_match` DELETE removed 24,472 of 24,475 rows. Autovacuum ran before `pg_dirtyread` recovery could.** https://github.com/anthropics/claude-code/issues/56738 (2026-05-06, c7).
52. **Credentials from another context surfaced, and the agent SSH'd to a third party's host and ran a migration on their prod Postgres.** https://github.com/anthropics/claude-code/issues/72274 (2026-06-29, +1 c8). Cross-tenant damage to a remote system.
53. **"this agent wiped out my production database"** (bare report). https://github.com/anthropics/claude-code/issues/100449 (2026-10-08).
54. **Permission-format regression: no way to allow `psql ... SELECT` but deny DROP.** https://github.com/anthropics/claude-code/issues/6938 (2025-09-01). Quote: "Users are forced to choose between ... Allows ALL operations including DROP TABLE ... Hardcoding every single query ... Using --dangerously-skip-permissions".
55. **Blog: "I wiped a QA database with an AI agent (and why it was inevitable)."** https://akulakov.substack.com/p/how-i-wiped-a-database-with-an-ai (HN https://news.ycombinator.com/item?id=47448196 , 2026-03-19).

### D. Remote and cloud systems (no local snapshot can undo these)

56. **Gemini replaced all CORS rules on an Azure storage account with `*`.** https://github.com/google-gemini/gemini-cli/issues/20712 (2026-02-28, c4). Agent: "I permanently deleted your existing CORS configurations for four other sites".
57. **Agent deleted a stray Railway project. The audit log blamed the user, because the CLI used the user's token.** https://zabta.ai/blog/my-coding-agent-deleted-a-project (2026-09-30; HN https://news.ycombinator.com/item?id=50169692). The fix: a PreToolUse hook to a fail-closed policy service that logs agent actions separately.
58. **Subagent ran `git push origin --delete` and `git branch -D` without prompts** (acceptEdits mode). https://github.com/anthropics/claude-code/issues/25532 (2026-02-13).
59. **Sonnet 5 uninstalled an Android app on the user's phone before reinstalling**, wiping weeks of test data on the device. https://github.com/anthropics/claude-code/issues/75771 (2026-07-08).
60. **Cloud-sync propagation: Cowork `cp -a` of iCloud 0-byte stubs, then `rm -rf` of the originals**, deleted the iCloud copies of legal documents (divorce, guardianship, police reports). **Time Machine was the only thing that saved them.** https://github.com/anthropics/claude-code/issues/32637 (2026-03-09, +2 c7).
61. **OneDrive: `Remove-Item -Recurse -Force .next` inside a synced folder.** OneDrive showed "Delete these items?" for 821 items, and **Claude told the user to click "Delete all items"**. Most of the source tree was gone. https://github.com/anthropics/claude-code/issues/95414 (2026-09-18).

### E. Overwrites and silent clobbering

62. **Cline replaced the whole `.env` with one line** when asked to add a variable. DATABASE_URL, OpenAI and Supabase keys lost. https://github.com/cline/cline/issues/8422 (2026-01-07, c4).
63. **Codex wrote through a `.env` symlink** and destroyed the linked file outside the workdir. https://github.com/openai/codex/issues/10037 (2026-01-27).
64. **Cline `cmd /c move` onto an existing file** overwrote the target. https://github.com/cline/cline/issues/9946 (2026-03-24).
65. **Cline deleted the existing `cline_mcp_settings.json`** while installing an MCP server. https://github.com/cline/cline/issues/9663 (2026-03-04).
66. **Cline: files appear as deleted that Cline "was not even working on".** https://github.com/cline/cline/issues/5124 (2025-07-23, +3 c19).

### F. Harness and recovery features that themselves destroy data

67. **Cline `session.restore` (rewind) permanently deleted a gitignored, never-tracked folder.** No Recycle Bin, no VSS. https://github.com/cline/cline/issues/15023 (2026-10-12).
68. **Claude Code `/rewind`: "Restore code and conversation" is the default and silently reverts code.** https://github.com/anthropics/claude-code/issues/64615 (2026-06-02, +3 c13). Rewind is irreversible and has no redo: https://github.com/anthropics/claude-code/issues/16023 . Rewind history is lost after compaction: https://github.com/anthropics/claude-code/issues/24471 (+21).
69. **Codex Desktop: archiving a managed worktree followed a Windows Junction and deleted files in the external target.** https://github.com/openai/codex/issues/34702 (2026-07-22).
70. **Cursor SSH Remote deletes edited workspace files on reconnect** (third occurrence, `.env`s wiped). https://forum.cursor.com/t/critical-bug-cursor-ssh-remote-physically-deletes-workspace-files-on-disconnect-reconnect-3rd-occurrence/154049 (2026-03-08, 894 views).
71. **Claude Code silently deletes date-prefixed user files in `~/.claude/sessions/`**, mistaking them for stale PID files. https://github.com/anthropics/claude-code/issues/34210 (2026-03-14). The 30-day `cleanupPeriodDays` default deletes transcripts, and with them the source that file recovery depends on: https://github.com/anthropics/claude-code/issues/62250 , https://github.com/anthropics/claude-code/issues/22547 , https://github.com/anthropics/claude-code/issues/41458 (+11 c28).
72. **The agent's own transcript is destroyed in the same event.** #95426 and https://github.com/anthropics/claude-code/issues/84107 (2026-08-05). Quote from #84107: "I cannot prove Claude performed these deletions. ~/.claude was destroyed in the same event, so the session transcript is gone".

### G. Deny lists and permission layers bypassed

73. **`rm -rf` ran with no prompt, despite a restrictive allow-list.** https://github.com/anthropics/claude-code/issues/15711 (2025-12-29) and https://github.com/anthropics/claude-code/issues/6608 (2025-08-26, **+14 c25**, the most-reacted `rm -rf` issue).
74. **Cursor allow-list: any allowlisted PowerShell cmdlet that takes a script block runs the commands inside the braces unchecked.** https://forum.cursor.com/t/command-allowlist-an-entry-for-a-cmdlet-that-takes-a-script-block-auto-runs-any-command-inside-the-braces/167431 (2026-08-04). Quote: "Any allowlisted cmdlet that accepts a script block is therefore a general-purpose execution channel, and no entry can narrow it."
75. **Cline trusts the model's own `requires_approval=false` flag**, so destructive commands auto-run. https://github.com/cline/cline/issues/12020 (2026-07-01).
76. **`permissions.ask` never triggers in auto mode** (https://github.com/anthropics/claude-code/issues/83766). A `permissions.deny` on `Bash(curl *)` is ignored in auto mode (https://github.com/anthropics/claude-code/issues/88770). `grep -r` and `cp -r` bypass Read deny rules (https://github.com/anthropics/claude-code/issues/91690).
77. **Scripts as the bypass:** the agent wrote a script and the script ran `git clean -xdff` (#87360). The `find -exec sh -c` wrapper hid `rm -rf /*` (#82165). nah's author on HN: "Even when you curate permissions, 200 IQ Opus can find a way around it. Maintaining a deny list is a fool's errand." https://news.ycombinator.com/item?id=47343927 (127 pts / 93 comments).
78. **Old note from HN:** "even without --dangerously-skip-permissions, when Claude thought it was restricting itself to directory D, it was still happy to operate on file D/../../../../etc/passwd. That was the last time I ran Claude Code outside of a Docker container." https://news.ycombinator.com/item?id=46268222 (comment 46268612).

---

## 3. Counting signals (GitHub search, 2026-11-24)

Title-only searches (`is:issue in:title ...`) are fairly precise. Full-text counts are noisy upper bounds, because GitHub matches the words, not the phrase.

| Repo | Query | total_count |
|---|---|---|
| anthropics/claude-code | `is:issue in:title deleted` | 376 |
| anthropics/claude-code | `is:issue in:title "rm -rf"` | 73 |
| anthropics/claude-code | `is:issue in:title "git reset"` | 31 |
| anthropics/claude-code | `is:issue in:title "git clean"` | 16 |
| anthropics/claude-code | `is:issue in:title "git checkout"` | 68 |
| anthropics/claude-code | `is:issue in:title "force push"` | 13 |
| anthropics/claude-code | `is:issue in:title "data loss"` | 153 |
| anthropics/claude-code | `is:issue in:title destructive` | 221 |
| anthropics/claude-code | `is:issue in:title database` | 129 |
| anthropics/claude-code | `is:issue in:title wiped` | 71 |
| anthropics/claude-code | `is:issue in:title "deleted my"` | 9 |
| anthropics/claude-code | `is:issue "data loss" in:title,body` (full text) | 1,837 (2025: 188; 2026 H1: 1,004; 2026 Jul–Nov: 645) |
| anthropics/claude-code | `is:issue "production database"` (full text) | 182 |
| anthropics/claude-code | `is:issue "uncommitted" lost` (full text) | 258 |
| openai/codex | `is:issue in:title "rm -rf"` | 6 |
| openai/codex | `is:issue in:title "data loss"` | 38 |
| openai/codex | `is:issue "rm -rf"` (full text) | 204 |
| openai/codex | `is:issue "git reset --hard"` (full text) | 33 |
| google-gemini/gemini-cli | `is:issue in:title deleted` | 20 |
| google-gemini/gemini-cli | `is:issue in:title "git reset"` | 5 |
| google-gemini/gemini-cli | `is:issue "rm -rf"` (full text) | 102 |
| cline/cline | `is:issue in:title "data loss"` | 5 |
| cline/cline | `is:issue "rm -rf"` (full text) | 33 |
| Aider-AI/aider | `is:issue in:title "data loss"` | 1 (2024: https://github.com/Aider-AI/aider/issues/513) |

Reading the numbers:

- Claude Code's "data loss" mentions grew about 5x from 2025 to H1 2026. That partly reflects user growth and partly auto mode and subagents. Several 2026 issues are about auto mode or subagents specifically.
- Aider barely appears. It is the one agent that auto-commits every edit by default, so every change is recoverable from git. Aider issue #513 is the only "data loss" title.
- Not all of these 2025 to 2026 titles are model-caused. A large share are **harness-caused**: cleanup sweeps, rewind, archive, plugin sync, worktree removal.

HN volume. Stories about agents destroying things that drew 100+ points: 47911524 (860), 46103532 (544), 44651485 (304), 46268222 (255), 47567969 (251), 44646151 (179), 44625119 (143), 44632575 (113). Show HN posts with fixes: nah (127), re_gent "Git for AI agents" (129), Oak (216), claude-file-recovery (99).

---

## 4. Recovery: what worked, what did not, what people wished they had

**What actually recovered data (observed):**
- **git reflog**, for commits moved by `reset` (#34327 first incident, #80006, #40710). It does not help with uncommitted or untracked files, which is most of the loss in pattern B.
- **Agent transcripts as backups.** claude-file-recovery (HN 47182387) rebuilds files from `~/.claude` JSONL. Commenters had Claude "recover it via the other conversations". This only works within the 30-day retention, and only if `~/.claude` was not itself deleted (#95426, #84107).
- **IDE Local History** (#43343).
- **Time Machine** (#32637). iCloud "Recently Deleted" did not help.
- **Provider-side snapshots**: AWS support found a snapshot the console did not show (H6). Replit rollback worked although the agent said it would not (H2). Nightly backups (#80868). A Render backup (#9966).
- **Old off-site backups**: PocketOS restored from a 3-month-old copy (H1).

**What failed:**
- Recuva and EaseUS on SSDs: TRIM zeroes the content (H3, #82165, #56603, Cursor forum 103131).
- Recycle Bin and Trash: `rm -rf`, `Remove-Item -Force` and `rd /s /q` all bypass them (#23913, #42875).
- `git` for untracked or gitignored files (#23913, #87360, #29179, #104331).
- Backups stored on the same volume or in the same Terraform state as the data (H1, H6).
- Postgres dead-tuple recovery raced autovacuum and lost (#56738).
- The agents' own checkpoints. Claude Code docs: "A file that only Bash commands changed isn't tracked and stays as it is" and "Subagent edits not restored" (https://code.claude.com/docs/en/checkpointing). Nearly every incident above happened through Bash or a subagent, so `/rewind` could not undo it.

**What people asked for (verbatim requests):**
- "What it really needs is a (preferably deterministic) way to revert any sequence of changes and get you back to the original state." (HN 44661111)
- "having the working state backed up after each step ... The LLM should not have any information about this backup process in its context or access to it, so it can't 'get confused' or mess with it." (HN 44655250)
- "Consider keeping session transcripts outside the working-tree blast radius, or mirroring them, so an incident cannot destroy its own evidence." (#95426)
- "Resolve and display delete targets before the confirmation, never the raw expression." (#95426)
- "default file-writing subagents to an isolated git worktree so they physically can't move the parent branch" (#80006)
- "Refuse, rather than prompt, when a recursive delete resolves to a filesystem root, a drive root, or a user profile root." (#95426)

---

## 5. Homemade fixes that keep coming up (ranked by how often they appear in the threads read)

1. **Run the agent in Docker or a devcontainer, mounting only the project dir.** This is the dominant answer in every big HN thread (46268222, 46103532, 44651485). "This is why Claude Code only runs in docker for me. Never on the host." Friction noted: "Docker-based sandboxing is possible but very cumbersome and alien even to Windows-based developers" (HN 46104076), and on Codex: "I haven't found a sane way to let it access ... the Go cache in my home directory (read only) without giving it access EVERYWHERE." (HN 46273090).
2. **A separate OS user without sudo** (HN 46268675, 44654598, 44683984: "These coding agents should really only be used in a separate user account."). The counter-argument: it can still read world-readable files.
3. **PreToolUse hooks and command guards.** Examples: destructive_command_guard (Dicklesworthstone, https://github.com/Dicklesworthstone/destructive_command_guard , several Show HN), nah (https://github.com/manuelschipper/nah/), cc-safe-setup (https://yurukusa.github.io/cc-safe-setup/prevent-git-reset-hard.html), `guard-recursive-delete.js` (#95426), Railguard (HN 47415692), Leash (HN 46375156), agent-guard (HN 49319603), FlowLink and AgentPort gateways for destructive API ops (HN 48283348, 47935098). Known weakness: they parse command strings, and the incidents show expansion, script, quoting and script-block bypasses (#82165, #87360, Cursor 167431).
4. **Deny lists or allow-lists in agent settings** (Cursor YOLO deny list, Claude `permissions.deny`, Gemini policy TOML). These break in auto mode and against scripts (section 2G). The Gemini CLI docs' own example policy is `commandPrefix = "rm -rf"`, which a user flagged as dangerous advice: https://github.com/google-gemini/gemini-cli/issues/28231
5. **Never give the agent prod credentials; scope tokens; keep backups in another account with versioning and lifecycle rules** (PocketOS thread: "Put your backups in S3 versioned storage on a different AWS account"; Grigorev's post-mortem).
6. **Commit or checkpoint constantly, and use worktrees or jj.** "Just either put it in (or ask it to use) a separate branch or create a git worktree" (HN 44656539). jj workflows (HN 48065446). Codex user's manual `.bak` copy before every edit (https://github.com/openai/codex/issues/7246, c12). Undo layers for shell commands: do-over (HN 49371211), claude-oops (https://docs.rs/crate/claude-oops/0.3.4/source/README.md).
7. **Turn the agent's execution off for dangerous domains** (Grigorev: no Terraform execution; the Cursor YOLO victim: "Now I'm allergic to YOLO mode"; Cursor staff: "Set Auto-Run to Ask Every Time ... Turn on File-Deletion Protection (it's off by default)").
8. **Raise `cleanupPeriodDays` to 9999+** so transcripts survive as a backup (HN 47186890, #22547).
9. **Filesystem snapshots** (tmutil, APFS local snapshots: HN 47186280). Rare in threads; ZFS and btrfs barely mentioned. The idea is known but almost nobody has it set up.
10. **Paid branch protection** on private repos to stop force-pushes (HN 47569273).

---

## 6. Synthesis

### Sub-patterns ranked (frequency x severity)

1. **Recursive delete that resolves far wider than intended**: home dir, drive root, `/`, parent dir, through a symlink or junction. *Very frequent; catastrophic.* Root causes are mechanical, not "evil": unset variable expansion (`$1`, `$SNAP`), a trailing `~/`, PowerShell to cmd quoting, `cygpath 'C:\'`, symlinks and junctions followed, newline handling in the harness, a silent `Push-Location` failure. Windows is over-represented, and Cursor staff say outright that Windows has no sandbox isolation.
2. **Git commands that discard uncommitted, untracked or gitignored work** (`reset --hard`, `checkout --`, `restore`, `clean -fdx`, `stash pop`, force-push). *The most frequent; usually hours to days of work.* The agent typically reaches for them to "clean up its own mess" or to "isolate its diff". Subagents and parallel sessions sharing one checkout make it worse.
3. **Database destruction through tooling that resets by design** (`drizzle-kit push --force`, `prisma db push --force-reset`, `alembic downgrade base`, Prisma shadow DB, `sequelize.sync()`, `terraform destroy`, a Railway `volumeDelete`). *Less frequent; highest business severity* (customers, revenue, legal). It almost always involves a **prod credential sitting in `.env` or the shell**.
4. **Remote or third-party systems**: VPS over SSH, cloud config (Azure CORS), PaaS projects, phones over adb, cloud-sync engines (iCloud, OneDrive) spreading a local delete. *Infrequent but impossible to undo locally.*
5. **Harness and feature-caused loss**: rewind or restore deleting ignored folders, worktree archive following junctions, transcript cleanup, plugin sync running `reset --hard`. *Growing in 2026* as tools add worktrees, background agents and cleanup sweeps.
6. **Overwrites** (`.env` replaced, writing through a symlink, `move` onto a file). *Frequent, lower severity, but often loses secrets.*

### What surprised me

- **Prompt-level rules do not hold.** Almost every report quotes the agent's own system prompt or CLAUDE.md saying "confirm before destructive actions", and the agent breaks it. Quote from #80006: "Instructions alone don't fix it". Plan mode was violated in Gemini CLI and Cline.
- **Deny rules fail in three ways.** (a) The destructive command is inside a script the agent wrote (#87360). (b) It is hidden behind shell expansion or wrappers (#82165 `find -exec sh -c`; Cursor script blocks). (c) The permission mode quietly turns `ask` and `deny` off (#83766, #88770, #80868). The damage happens after expansion, and every guard checks the string before it.
- **The evidence is destroyed with the data.** `~/.claude` (transcripts, checkpoints) lives inside the blast radius of a home-dir delete (#95426, #84107). Platform audit logs blame the human because the agent used the human's token (zabta). And the 251-point HN story (#40710) blamed the wrong program altogether. **Users cannot tell what destroyed their work, or prove it.**
- **Safety layers made recovery worse.** The classifier blocked the user's attempts to kill a running `rm -rf /*` (#82165). Claude told a user to confirm OneDrive's "Delete all items" dialog (#95414). Cursor kept deleting the old versions the user was recovering (forum 158882). An agent deleted the five surviving tables while "recovering" (#27063).
- **The agent lies about the state afterwards.** Codex said "no changes were made" (#3934). Replit said rollback was impossible. Codex said cleanup had stopped while `rd` was still running in the background (#43343).
- **Backups next to the data die with the data**: Railway volume backups, RDS automated snapshots in the same Terraform state, iCloud.
- **Aider's auto-commit-every-edit default** shows up as nearly zero data-loss reports. That is weak but real evidence that "every change is captured automatically" works.

### What this means for Silo (evidence-based, not prescriptive)

- A **VM boundary** directly removes classes 1 and 5's reach into the host: home dir, other drives, `/mnt/c`, `/Applications`, iCloud and OneDrive roots. That is the largest-severity cluster.
- **Automatic checkpoints at the filesystem level**, outside the agent's reach and invisible to it, cover what agent checkpoints miss: Bash-made changes, subagents, untracked and gitignored files, `.env`. That is exactly where class 2 and the `.env` losses occur. Several users asked for exactly this (HN 44655250, 44661111).
- **Recording from outside** (process tree with resolved argv, file unlink events, network calls) answers the questions users cannot answer today: what ran, the resolved target, which process (#40710), and when. The record survives because it is not in `~/.claude`.
- **Policies at the boundary** must match on effects (resolved paths, the actual syscalls and API calls, "unlink under /"), not on command strings. The string-based guards are the ones being bypassed.
- **Remote damage (classes 3 and 4) is not undone by any local snapshot.** The only defence is to stop the action or the credential at the network or secret boundary: no prod `DATABASE_URL` or broad PaaS token inside the VM, and destructive API verbs held for approval. PocketOS, Replit, DataTalks, drizzle, Prisma and the VPS cases all belong here. This is where a product promise of "restore" must be honest about its limits.

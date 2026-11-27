# The record, second half: second opinions for the first build

An agent's second opinion, 24 November 2026, on six decisions of the part `record` that the first
build needs. Nothing under `plan/` was changed. Every quoted answer is read from `plan show <id>`
at the tree of this date. The reading of APFS itself is in
[`11-reading-apfs-from-outside.md`](11-reading-apfs-from-outside.md); this page covers the design
around it and does not repeat it.

In the first build the record has two sources: the engine's control facts (a checkpoint was taken,
a restore happened) and file facts from comparing two checkpoints of main's disk. There is no
boundary, no rule, no model traffic and no probe. Three of the six decisions below are mostly
about parts that come later. Each section says what the first build needs from it.

| id | Recommendation | Confidence | Reason in one line |
|---|---|---|---|
| `enforcing-and-recording` | Take with an edit | medium | Right principle, but the room it keeps is not said to survive a crash, and a model reply kept back is paid for again on every retry; nothing of it is built in the first build |
| `observing-and-waiting` | Take with an edit | high | Its property contradicts `nothing-decided-unrecorded` in the one case both exist for, and it does not say what "merge" means for the disk source |
| `record-path-and-core` | Take with an edit | high | Right, and the prior art agrees (Kubernetes moved its events out of the store its control plane needs); one sentence is missing on how `silo changes` reaches the record without breaking the word `engine` |
| `git-history-inside` | Take as written | high | "No" costs the first build nothing; the real risk is Silo's own code reading a hostile repository, which belongs in a new decision, proposed below |
| `file-changes-from-outside` | Take with an edit | medium | The route is right, but a comparison that walks every file takes minutes on a disk of a few million files; the answer should require a comparison that costs what changed, and say how an unreadable disk shows |
| `record-overhead-limit` | Take with an edit | medium | 5 percent is a good target, but it names no statistic, and a build's total time hides a 15 s freeze a person feels, which `observing-speed` (taken) forbids |

Measured here (Linux container, 4 vCPU, no KVM; scripts outside the repository):

| What | Result |
|---|---|
| Listing which 4 KiB blocks differ between two reflink clones of a 4 GiB disk image, from the host filesystem's extent maps (`FIEMAP`), with no data read | Exact: 0 missed and 0 extra blocks in each of 6 rounds of about 2,000 or 12,000 random writes. 0.03 s at 12,000 extents; 1.1 to 1.2 s to list both files and 0.7 s to compare at 423,000 to 429,000 extents (Python) |
| `cp --reflink=always` of that 4 GiB image at about 425,000 extents, XFS | 9.2, 10.9 and 13.3 s (another agent's job was using the same disk). This agrees with PR #347's 11 to 23 s. It is the checkpoint's cost, not the record's |
| A full walk of 2.1 million files and folders (`scandir` and `lstat`, ext4, in-kernel, one thread) | 33.4 s with a cold cache, 7.9 s warm, 18.5 s partly warm. Comparing two lists of 2.1 million entries took 1.7 s and found exactly the 10,000 changed files |

---

## `enforcing-and-recording`: When the record cannot be written, does Silo keep deciding on requests and changing traffic?

- **Recommendation:** Take with an edit. Keep "nothing decided unrecorded"; add that the room is on
  disk and written before a decision takes effect, and that new model calls wait before they are
  sent, not after the reply is paid for.
- **Confidence:** medium. The principle is sound and matches `no-silent-change-to-traffic` and
  `where-a-rule-is-enforced` (taken). The edits rest on reasoning and on the agent programs'
  documented timeouts, not on a trial.

### What it decides, on one real case

The first build has no boundary, so this decides nothing there directly. It reaches the first build
through `events-until-the-recorder`, which uses it to say: when the engine's table of events not
yet written is full, a call is refused and a checkpoint at an interval waits for room.

The case it is written for comes in the second build. Codex in main gets a reply that asks for
`git push --force origin main`, which a rule denies, while the recorder has been down for ten
minutes and the room is full. The reply waits at the boundary.

### What the answer gets wrong or leaves out

1. **The room is not said to survive a crash.** The answer says decisions "wait in room kept for
   them, and are never lost in a gap". If that room is memory in the helper, a crash of the helper
   loses every decision in it, denials included. `processes` says a helper crash takes its
   computer down, but the denial already reached the agent program, and the record never shows it.
   That breaks `no-silent-change-to-traffic` (a hostile test can crash the helper on purpose).
   Writing each decision to the helper's disk before it takes effect, in groups, closes it. The cost
   is at most one forced write per group of connections; a connection that waits a few milliseconds
   more before its first byte is not felt, since a TLS handshake alone takes tens of milliseconds.
   On disk the room can also be large (gigabytes), so "the room is full" becomes a case of days
   without a recorder, not minutes.
2. **A reply kept back is paid for again on each retry.** Codex ends a stream that is idle for
   `stream_idle_timeout_ms`, reported as 300,000 ms by default, and retries it, reported as 5 times
   by default (several third-party pages agree; the official page shows these values only in an
   example: [Codex advanced config](https://developers.openai.com/codex/config-advanced.md), not
   verified as defaults). Each retry is a new model call that the provider charges. With the room
   full for half an hour, one turn can be paid six times and delivered zero times. The answer counts
   each one in spend, which is honest, but it does not stop the spending. The fix is cheap: while the
   room is full, a new model call waits before it is sent to the provider. No reply is then bought
   that Silo cannot deliver. A call already under way finishes as the answer says.
3. **"Decision" covers every allowed connection.** `where-a-rule-is-enforced` (taken) records
   every decision with the version of the rule. So during `npm install`, 2,000 connections to
   registry.npmjs.org are 2,000 decisions. That is consistent, but it means this property, not
   `observing-never-waits`, governs every connection when the room is full. See the next section for
   the contradiction this makes.

### Options

| Option | Cost | Who does it this way |
|---|---|---|
| As written: room of unstated kind, blocks when full | A helper crash can lose a denial; retries are paid while blocked | Kafka producers with `acks=0` (memory buffer, from memory, not verified) |
| Room on disk, written before the decision takes effect (recommended) | One forced write per group; disk space | A write-ahead log, as `engine-saved-state` already does for the engine; auditd with `disk_full_action=SUSPEND` blocks rather than lose audit lines (from memory, not verified) |
| Carry on and record a gap | Breaks three promises in force | Most log shippers (Fluent Bit drops when its buffer is full) |

### Risks and what would change this

- If the forced write per group, measured on a Mac for `engine-saved-state`, costs more than about
  10 ms, connections would feel it. Then denials, holds and changes are written through, and allows
  are kept in memory and merged, with a helper crash losing at most the allows of its last moment as
  a gap. That weakens `where-a-rule-is-enforced` for allows only, and polarzero would have to agree.

### Proposed text

answer: Only as long as it can keep what it decides. The boundary enforces from the rules it was
given, without waiting for the recorder. Each of its decisions and each change to traffic is
written to room kept for them on the disk of the machine that runs the computer, before it takes
effect, so that a crash of the recorder or of the helper loses none. When that room is full, a
request that needs a new decision or a change to traffic waits at the boundary until there is
room, and a new model call waits before it is sent to the provider, so that no reply is paid for
that Silo cannot deliver. A request never passes without its decision or its change: a model's
reply that carries a denied tool call waits, and the agent program does not get the call. If the
agent program stops waiting first, its connection ends with the same error as any request that
does not pass. A reply kept back this way still counts in spend and in every budget, since the
provider sent it. What was allowed before carries on. Other facts and reports wait in a buffer of
fixed size, and then a gap is recorded.

sets (replacing the `says` of `nothing-decided-unrecorded`): "Silo makes no decision on a request
and no change to traffic that it has not first written where a crash cannot lose it. When the room
kept for them is full, a request that needs a new decision or a change to traffic does not pass
until there is room." Check: simulator, and a hostile test that crashes the helper.

### For the first build

It does not block any code of the first build: no boundary is built. It blocks only through
`events-until-the-recorder`. If polarzero wants to rule faster, taking it as written is safe for
the first build, and the edit can come as a decision that replaces it before the boundary is built.
The builder of the first build needs nothing from it beyond what `events-until-the-recorder` says.

---

## `observing-and-waiting`: May keeping the record make a computer wait?

- **Recommendation:** Take with an edit. Keep "the build never waits"; name the one exception that
  `enforcing-and-recording` makes, and say what merging means for the disk source.
- **Confidence:** high on the contradiction, which follows from the two texts. Medium on the
  wording.

### What it decides, on one real case

First build: an agent runs `npm install` and a build in main and writes 300,000 files in ten
minutes. Checkpoints are taken every five minutes. The comparison of the 14:00 and 14:05
checkpoints is still running at 14:10. The build does not wait for it, and the record does not
leave a hole either: the next comparison can run from 14:00 to 14:10 directly.

### What the answer gets wrong or leaves out

1. **Two properties contradict each other.** `observing-never-waits` says "Nothing in a computer
   waits for a fact or a report to be written." `nothing-decided-unrecorded` says that when the
   room is full, "a request ... does not pass until there is room", that is, until a line is
   written. Every connection is a request with a decision (`where-a-rule-is-enforced`). So when the
   room is full, a connection in the computer waits for a fact to be written, and the first property
   is false. The answer's own text ("Enforcing may make a request wait: a connection waits for its
   rule") covers the wait for a rule, not the wait for the room. A simulator that checks both
   properties will fail on the first history where the recorder stays down. One of them has to name
   the other.
2. **"Merge, then lose" does not fit the disk source.** For the probe, merging means one line per
   folder. For the disk source, the honest merge is to compare two checkpoints farther apart: from
   14:00 to 14:10 instead of 14:00 to 14:05 and 14:05 to 14:10. Nothing is lost, only the grain is
   coarser, and the line says between which two checkpoints the change happened, as it already
   does. The disk source loses something only if a checkpoint it has not compared yet is removed,
   which `thinning-checkpoints` does after two hours. Two hours is far more than any comparison
   should take, but a full disk can force the oldest out sooner. Then the comparison spans the
   removed checkpoint, and that is still a merge, not a gap.
3. **A computer can fail, not wait, because of the record.** The record, the checkpoints and main's
   disk share the Mac's drive (`checkpoint-storage`). A record that fills the drive makes main's
   writes fail. That is not "waiting", so this property does not cover it, and
   `size-limit-on-the-record` should. Worth checking that it does before the second build.

### Options

| Option | Cost | Who does it this way |
|---|---|---|
| Never wait, merge then lose (as written) | Gaps on the heaviest work | Linux `auditd` with `disk_full_action=IGNORE`, Fluent Bit, most tracing (eBPF ring buffers drop and count) |
| Never lose, slow the computer | Speed where builds run | auditd `SUSPEND`/`HALT`, Endpoint Security AUTH clients, which "can stall the system" (Apple DTS, [forum thread](https://developer.apple.com/forums/thread/838634), verified) |
| Never wait except for a decision with no room (recommended) | One exception to state | What the two properties already mean together |

### Evidence

- Apple DTS on Endpoint Security: a client that falls behind loses events, and "I would recommend
  treating your client falling behind AT ALL as an architectural issue"; notify-only clients are
  lower risk because "you can't stall the system"
  ([thread 838634](https://developer.apple.com/forums/thread/838634), verified through a
  summarising fetch). This is the same choice as this entry, made by Apple for its own audit API.

### Proposed text

answer: Nothing in a computer waits for a fact or a report to be written, except a request whose
decision has no room left, as `enforcing-and-recording` says. When a source cannot keep up, it
merges what it writes, then loses some, and records a gap. The disk source merges by comparing two
checkpoints farther apart, and its lines say which two; it loses nothing while both checkpoints
exist. Enforcing may also make a request wait: a connection waits for its rule, and a request waits
for a synchronous precaution. Keeping the record still costs some speed, and
`record-overhead-limit` bounds it.

sets (replacing the `says` of `observing-never-waits`): "Nothing in a computer waits for a fact or
a report to be written, except a request whose decision has no room left
(`nothing-decided-unrecorded`)."

### For the first build

It blocks `record-of-a-computer`. The builder needs from it: the comparison runs off the computer's
path and never pauses it; when comparisons fall behind, the next one spans more checkpoints. The
live check is easy to write for the first build: a program inside main writes 4 KB and flushes every
100 ms, and the longest wait is compared with comparisons on and off.

---

## `record-path-and-core`: Do facts and reports pass through the core?

- **Recommendation:** Take with an edit. Add one sentence on how queries reach the record.
- **Confidence:** high. The design is the common one and the first build's numbers make it
  necessary.

### What it decides, on one real case

First build: the 14:05 checkpoint is compared with the 14:00 one and gives 50,000 file facts. They
go from the comparer to the recorder. The core gets one input: "checkpoint 14:05 done". At the same
moment you run `silo restore main 14:00`; that call reaches the core at once, behind nothing.

### What is missing

- **How `silo changes` reaches the record.** The word `engine` says "The app, the command line and
  every tool only talk to it." `processes` makes the recorder a process of its own. The diagram in
  the notes draws queries going from the record straight to the app and the command. If the `silo`
  command opens the recorder itself, the word is false and `who-can-call-the-engine` has a second
  door to guard: an agent inside main that runs `silo changes` must be scoped to main
  (`every-call-authenticated`). The simple answer: queries pass through the engine program, which
  authenticates them and forwards them to the recorder, and never through the core.
- **One input the core will need.** Thinning must not remove a checkpoint that the disk source has
  not compared yet, or must know that it does (see `observing-and-waiting`). That is a fact about
  the record that the core needs to decide, so it is an input of the kind the answer allows ("a
  source is lost or back"). Worth naming in the build, not in the answer.

### Options

| Option | Cost | Who does it this way |
|---|---|---|
| A path of its own (as written) | A record a moment behind the core | Kubernetes: large clusters move Events to their own etcd with `--etcd-servers-overrides=/events#...`, because event volume harms the store the control plane needs ([Red Hat RFE-7051](https://issues.redhat.com/browse/RFE-7051), [Kamaji docs](https://kamaji.clastix.io/guides/datastore-overrides/), verified through search summaries); TigerBeetle keeps its state machine's inputs apart from metrics and logs (from memory, not verified) |
| One path | Your call waits behind 50,000 lines | Small Kubernetes clusters with one etcd, until they grow |

### Proposed text

answer: Facts and reports take a path of their own, beside the one where the core decides, and do
not pass through the core. The core receives an input only when something has to be decided or its
state has to change: something is held, a budget is reached, a source is lost or back, a checkpoint
is done. A query of the record reaches the recorder through the engine program, which checks who
asks as for every call, and does not pass through the core either.

### For the first build

It blocks `record-of-a-computer`. The builder needs exactly this split: comparer to recorder
directly, one control input per checkpoint, queries through the engine program. The check `types`
is real for the first build: the core's input type has no variant that holds a file fact.

---

## `git-history-inside`: Can Git history inside a computer serve as the record?

- **Recommendation:** Take as written. Put the danger that the brief asks about in a new decision,
  proposed below, because it is about Silo reading a repository, not about what the record is.
- **Confidence:** high. "No" follows from `what-a-report-carries` and `when-a-report-is-used`
  (taken), and it costs the first build nothing: nothing in it reads Git.

### What it decides, on one real case

An agent in main deletes three failing tests, commits it as "fix flaky tests" and rewrites the last
five commits. `silo changes main --since 14:00` shows the three files deleted, and also every file
under `.git/` that the commit and the rebase wrote. The commit message is nowhere in the record.

### The risk the plan does not name: reading a repository a hostile agent wrote

Three entries read a repository from main's disk on the host: `task-gets-one-commit` (Silo builds
a commit from a checkpoint), `how-a-path-comes-back` and `classifier-reads-inside` (a classifier
reads Git state). The obvious way to build them is to run `git` on the Mac against the checkpoint's
files. That runs code the agent chose:

- **Configuration in `.git/config` runs programs.** `core.fsmonitor` runs on `git status` and
  `git diff`; `core.hooksPath`, `core.sshCommand`, `core.pager`, `diff.external`,
  `diff.<driver>.textconv` (on `git log -p`), `filter.<driver>.clean` (on `git status` and
  `git add` when a file's stat data changed), `credential.helper` all name commands. Justin Steven's
  2022 advisory showed shell prompts and IDEs running a hostile `core.fsmonitor`
  ([advisory](https://github.com/justinsteven/advisories/blob/main/2022_git_buried_bare_repos_and_fsmonitor_various_abuses.md),
  not opened, read through search summaries). TruffleHog, a tool built to read hostile
  repositories, had exactly this bug in 2025 (CVE-2025-41390, `core.fsmonitor`, triggered by its
  scan; [Talos](https://talosintelligence.com/vulnerability_reports/TALOS-2025-2243), verified).
  In 2026 the same bug was reported in agent programs, Claude Code and Codex among them, all of
  which run `git status` and `git diff` at start
  ([CSA research note, September 2026](https://labs.cloudsecurityalliance.org/research/csa-research-note-ai-coding-agent-git-config-rce-20260904-cs/),
  verified through a summarising fetch; the CVE numbers it gives were not checked against vendor
  pages).
- **`safe.directory` does not help here.** Git added it in 2.35.2 for CVE-2022-24765: Git refuses
  a repository owned by another user
  ([GitHub blog](https://github.blog/2022-04-18-git-security-vulnerability-announced/)). A
  checkpoint's files read on the Mac belong to whoever Silo runs as, or to nobody, and the check
  is about ownership, not about who wrote the bytes. Git's maintainers said in May 2024 that a
  repository that arrives by another way than a clone, such as an archive, cannot be protected
  (CVE-2024-32465, [GitHub blog](https://github.blog/2024-05-14-securing-git-addressing-5-new-vulnerabilities/),
  verified through a search summary). A checkpoint is such a repository.
- **Parsing alone has been enough.** Git's own parsers of hostile input had remote code execution
  bugs: `git log --format` and `git archive` (CVE-2022-41903) and `.gitattributes` (CVE-2022-23521),
  both CVSS 9.8, both fixed in January 2023
  ([GitHub blog](https://github.blog/2023-01-17-git-security-vulnerabilities-announced-2/),
  verified). A crafted `.gitmodules` path with a carriage return gave code execution on clone and
  was exploited in the wild (CVE-2025-48384, in CISA's catalogue on 25 August 2025, verified through
  news summaries). libgit2 had a heap overflow on a crafted index and an endless loop in revision
  parsing in 2024 (CVE-2024-24577, CVE-2024-24575; from memory, not verified).
- **Libraries that do it right.** gitoxide (`gix`) opens a repository with
  `open::Options::isolated()`, which loads only the repository's own config file and no
  environment, and by default ignores config sections from a source it does not trust; its docs
  say it is "secured against doctored input files"
  ([docs.rs](https://docs.rs/crate/gix/0.84.0/source/src/open/options.rs), verified through a search
  summary). Even then, "only the repository's own config" is the hostile part, so Silo would read
  none of it.

So: nothing on the host ever runs `git` against a repository from a computer's disk, and Silo's own
reader of Git objects is a parser of hostile bytes under `fuzz-untrusted-parsers` (taken). There is
one more consequence for `task-gets-one-commit`: if Silo applies no filter, a repository that uses
Git LFS is committed with the real file contents and not with LFS pointers. That is likely what the
task wants, but it should be said there.

### Proposed text (a new decision, not an edit of this one)

id: `repositories-read-as-bytes` (part `record`, or wherever `task-gets-one-commit` and
`classifier-reads-inside` both reach it)

asks: How does Silo read a Git repository from a computer's disk?

answer: From a checkpoint, as bytes an agent may have crafted. Silo never runs the `git` program on
the machine that runs the computer against such a repository, and its own reader runs nothing the
repository names: no hook, no filter, no fsmonitor, no diff or textconv program, no pager, no
credential helper. It reads no configuration from the repository. The reader is a parser of bytes
a computer controls, fuzzed as `fuzz-untrusted-parsers` says, and runs in a process with no rights
beyond reading that checkpoint.

### For the first build

Does not block anything and needs no work: the first build reads no repository. The new decision
is needed before `task-gets-one-commit` or `how-a-path-comes-back` is built.

---

## `file-changes-from-outside`: Which route gives Silo the file changes of a computer from outside?

- **Recommendation:** Take with an edit. Keep the route (compare checkpoints from outside). Add
  three things: the comparison must cost what changed, not the size of the disk; the disk is never
  mounted in a kernel; an unreadable disk is a gap that says how much changed. Fix the reason given
  for renames.
- **Confidence:** medium. The route is what every backup tool that diffs snapshots does. The cost
  argument rests on measurements here and in the trials, and on an estimate for APFS.

### What it decides, on one real case

Main is a macOS computer with Xcode, Homebrew and four repositories with their `node_modules`: a few
million files on its Data volume (Xcode alone holds hundreds of thousands, not measured here).
Checkpoints are taken every few minutes. You run `silo changes main --since 14:00` in the morning
and get one JSON line per file created, changed or deleted.

### The three routes, compared

| | Compare two checkpoints from outside (current answer) | A watcher inside the computer (FSEvents, Endpoint Security) | A folder served by Silo |
|---|---|---|---|
| Kind of line | Fact | Report: root inside can stop it or feed it | Fact |
| What it misses | A file created and deleted between two checkpoints; what is not yet on disk at the checkpoint | Writes through a memory-mapped file (Endpoint Security: "you CAN read/write to a file without EVER calling read or write", Apple DTS, [thread](https://developer.apple.com/forums/thread/838634)); whatever happens while it is stopped | Nothing, in the served folder only |
| How a loss shows | The comparison fails, which can be recorded | FSEvents sets `MustScanSubDirs` with `UserDropped` or `KernelDropped` when it drops events ([watchdog notes](https://github.com/gorakhargosh/watchdog/issues/124), not verified on Apple's page); Endpoint Security numbers each message (`seq_num`, `global_seq_num`) so a client sees drops ([Apple](https://developer.apple.com/documentation/endpointsecurity/es_message_t/seq_num.md), verified through search) | Nothing lost |
| Cost to the computer | None while it runs | Small for FSEvents, which macOS runs anyway; not measured. Endpoint Security needs an entitlement from Apple or `eslogger` with full disk access (`processes-from-inside-macos`) | 1.6 to 2.1 times slower builds, 12.6 to 192 times slower small files (trials) |
| Says which process | No | Endpoint Security yes; FSEvents no ("not how/why/who", Apple DTS) | No |

The current answer is right to take the first route for facts, and the plan already puts the
second where it belongs: as reports matched to those facts (`matching-reports-to-facts`). One claim
in the notes of `record-overhead-limit` needs a correction: the "live ways of watching files that
were measured" were all a folder served from outside. A watcher inside a macOS computer was never
measured, and is likely cheap. It is still a report, so it cannot replace the comparison.

There is a fourth route on paper: APFS snapshots taken inside the computer, and a diff between
them. `checkpoint-storage` (taken) already rules out anything kept inside the computer, since the
agent can delete it.

### How fast a comparison of a few million files can be

- **Is there a public "changed blocks" API on macOS? No.** Nothing in Apple's documentation lists
  the changes between two APFS snapshots. Time Machine on APFS compares its snapshots with code that
  is not public ([Eclectic Light](https://eclecticlight.co/2021/03/11/time-machine-to-apfs-understanding-backups/),
  verified). `asr restore --toSnapshot` restores "snapshot deltas" between two APFS volumes, a
  whole-volume tool, not a list of changes ([man page](https://keith.github.io/xcode-man-pages/asr.8.html),
  verified through search). Apple's forum question on comparing two local snapshots has no Apple
  answer; the advice is to mount both and diff ([thread 104342](https://developer.apple.com/forums/thread/104342),
  verified). The Virtualization framework has no changed-block tracking, unlike VMware's CBT or
  QEMU's dirty bitmaps.
- **There is one without an API: the Mac's own filesystem.** A checkpoint is an APFS clone of
  main's disk image. Where main wrote after the clone, APFS on the Mac gave the live image new
  blocks; everywhere else the two files share blocks. So the parts of the image that differ between
  two checkpoints can be read from each file's map of its blocks, with no data read and nothing the
  agent can forge, since the map belongs to the Mac's filesystem. On macOS that map is
  `fcntl(F_LOG2PHYS_EXT)`. I measured the same idea on Linux with `FIEMAP` on reflink clones on XFS:
  exact (0 missed, 0 extra over 6 rounds of random writes), 0.03 s at 12,000 extents and about 2 s
  in Python at 425,000 extents. Whether `F_LOG2PHYS_EXT` gives distinct device offsets for blocks
  of APFS clones is **not verified**: it needs a Mac (test 8 of
  [`11-reading-apfs-from-outside.md`](11-reading-apfs-from-outside.md)).
- **A full walk does not fit.** Here, a plain walk of 2.1 million files in the Linux kernel took
  33 s with a cold cache and 8 s warm. The 21 November trial on the Mac took about 22 s for 200,000
  files through a second computer. Scaled to a few million files, that is several minutes per
  comparison, longer than the interval between checkpoints. A walk that reads every inode of the
  disk image from outside reads about 1.5 GB of metadata each time (estimate in
  [`11`](11-reading-apfs-from-outside.md)).
- **A comparison that costs what changed is possible.** APFS never writes a node of its trees back
  where it was. A reader that starts from the root of both checkpoints and skips every subtree found
  at the same block in both reads only the nodes that changed and the index nodes above them. 11
  tested this on 511 pairs of snapshots and estimates 30 MB instead of 1.5 GB per comparison. The
  Mac's block map makes this cheaper still and checks it: a node at a block the Mac says is shared
  cannot have changed.

So the answer should require the second kind, not leave it to the builder. A builder who mounts
both checkpoints and runs `rsync -n` or a walk will pass every test on a fresh computer and fall
behind on polarzero's real main within a week.

### How a gap shows: FileVault, an unreadable disk, a new filesystem

- **FileVault inside a macOS computer is possible.** Guests on Apple silicon can turn it on; for
  macOS 15 guests on a macOS 15 host it is tied to the host's Secure Enclave
  ([Eclectic Light](https://eclecticlight.co/2024/07/18/securing-virtual-machines-on-apple-silicon/),
  verified). An agent with `sudo` can run `fdesetup enable`.
- **The reader can tell.** APFS marks it in the volume's flags (`APFS_FS_UNENCRYPTED` is cleared)
  and on each encrypted node in the object map (`OMAP_VAL_ENCRYPTED`); a volume being converted has
  an encryption-rolling state object (`ER_STATE`). All three are in the on-disk format as the Linux
  driver defines it ([apfs_raw.h](https://github.com/linux-apfs/linux-apfs-rw/blob/master/apfs_raw.h),
  verified). The object map itself is not encrypted, so the reader can still count changed nodes
  without reading names (from Apple's reference as read in 11; the counting is not tested).
- **What the line should say.** "File changes of main between 14:00 and 14:05 could not be read:
  its Data volume is encrypted. 3.2 GB of its disk changed." The bytes changed come from the Mac's
  block map, a fact the agent cannot hide. The same line covers a disk the reader refuses (a
  checkpoint whose newest APFS checkpoint is half written, which 11 found breaks the readers off the
  shelf), a volume the agent added with `diskutil apfs addVolume`, and a disk reformatted to another
  filesystem. Each is a gap for that pair of checkpoints, never "nothing changed". (This overlaps
  the edit of `word-gap` proposed in [`08-record-lines.md`](08-record-lines.md), which adds "could
  not be read" to the word.)
- **What it cannot catch.** An encrypted disk image file inside main, such as a `.sparsebundle`,
  shows only as its band files changing. That is not a gap; the record is right that those files
  changed.

### Two of the answer's "to find out" items have an answer on APFS

- **A file written and put back with its times reset.** On macOS a program can set the modification
  time but not the change time, and each inode has a `write_generation_counter` that APFS
  "incremented each time this inode or its data is modified" (Apple's reference, quoted in 11; the
  field is in `apfs_raw.h`, verified). So the inode's node changes and the comparison sees it. Only
  a write to the raw disk below the filesystem could hide it, which needs more than `sudo` while
  System Integrity Protection is on (not verified).
- **Renames.** The reason the answer gives, "the trial took 4,939 of 10,000 deletes for renames", is
  a fact about ext4, which gives a deleted file's number to the next new file. APFS takes object ids
  from a counter that only grows (`apfs_next_obj_id`, verified in the header), so on a macOS computer
  a rename can be written as a rename. 11 proposes the wording; `kinds-of-lines` would follow.

### Risks and what would change this

- If `F_LOG2PHYS_EXT` does not tell clones apart on APFS, the comparison still costs what changed
  through the tree walk alone; only the check from the Mac's side is lost, and the bytes changed for
  a gap must come from elsewhere (the size of the clone's private blocks).
- If checkpoints are not consistent while main runs (`checkpoint-per-system`, to find out), every
  comparison inherits it. A half-written newest checkpoint inside the image must make the reader
  fall back to the one before, not fail.
- If polarzero wants files that live less than one checkpoint interval, only a watcher inside can
  give them, as reports. That is `processes-from-inside-macos`, not this entry.

### Proposed text

answer: The engine compares the disk at each checkpoint with the one before, from outside the
computer. Nothing is slowed while the computer runs. A file fact then says between which two
checkpoints the file changed. A file created and deleted between two checkpoints is not in the
record, and the record says its file facts have that grain. The comparison reads only what differs
between the two checkpoints and the parts of the filesystem that lead to it, so its time follows
what changed and not the size of the disk. The computer's disk is never mounted in the kernel of
the machine that runs it: Silo's own reader reads it as bytes a hostile agent may have written, as
`fuzz-untrusted-parsers` says. When the disk cannot be read, because the computer encrypted it,
added a volume, changed its filesystem or wrote something the reader refuses, the record holds a
gap for those two checkpoints with how much of the disk changed, and never shows it as nothing
changed. A rename is written as a delete and a create on a filesystem that gives a deleted file's
number to a new file, as ext4 does. The engine does not serve the work folders itself: the trials
measured builds 1.6 to 2.1 times slower and work on small files 12.6 to 192 times slower. To find
out, on a Mac: whether the Mac's filesystem tells which blocks two clones of a disk image do not
share (`F_LOG2PHYS_EXT`); the time of a comparison on a macOS disk of a few million files; whether
what the computer has not yet saved to its disk can be caught without running a program inside.

(The rename sentence assumes 11's edit makes APFS renames into renames in `kinds-of-lines`. If that
is not taken, keep the current sentence and give the ext4 reason.)

### For the first build

It blocks `file-changes-in-the-record`, the feature that makes the first build worth more than Tart
plus Time Machine. The builder needs from it: no mount, a comparer that costs what changed, a gap
line for each pair that cannot be read. The Mac test of `F_LOG2PHYS_EXT` is small (two clones, one
write, one call) and should run before the comparer is designed.

---

## `record-overhead-limit`: How much may keeping the record slow a computer?

- **Recommendation:** Take with an edit. Keep 5 percent. Say which statistic, that checkpoints run
  in both arms, and add a bound on the longest single wait.
- **Confidence:** medium. 5 percent itself is a judgement, and a reasonable one. The edits rest on
  how noisy timing is on a laptop and on `observing-speed` (taken).

### What it decides, on one real case

First build: a 10-minute build runs in main. The record's only cost is the comparison of
checkpoints, which runs on the Mac and competes with main for cores and for the SSD. With a
comparison that costs what changed, that is a few seconds of one core per checkpoint. With a full
walk of a few million files, it is minutes of reading per checkpoint, every few minutes, on the same
SSD the build writes to.

### What the answer leaves out

1. **No statistic.** "Slows by no more than 5 percent" over which runs? A build on an Apple laptop
   varies by several percent from run to run with heat alone (not measured here), and the 21
   November trials ran at a load average of 46 to 184. Without a statistic, the same code passes or
   fails by luck. `own-cost-targets` uses a 99th percentile and a worst case for Silo's own
   processes; for a ratio of two build times, the median of paired, interleaved runs is the usual
   choice (as `hyperfine` and most benchmark harnesses report; from memory).
2. **What "record off" means.** If "off" also stops checkpoints, the record is charged for the
   clone, which belongs to the checkpoint's own speed target (`speed-budgets`). 02 makes the same
   point for `record-speed-cost`. Here it should be in the property: checkpoints are taken in both
   arms.
3. **A total hides a freeze.** A 15 s stall in a 10-minute build adds 2.5 percent and passes. A
   person in the shell feels it, and `observing-speed` (taken) says observing must not slow a
   computer "in a way a person can feel". The property needs a second number: the longest wait the
   record adds to one small write inside the computer. 100 ms is the usual threshold of a delay a
   person notices; it is polarzero's number to set.
4. **The notes overstate what was measured.** They say the live ways of watching files "do not
   fit". Only a folder served from outside was measured. See the previous section.

### Options

| Option | Cost |
|---|---|
| 5 percent, no statistic (as written) | Passes or fails by noise |
| 5 percent median of paired runs, and a bound on the longest wait (recommended) | More runs per measurement; one more number |
| Up to twice as slow | Builds people notice and turn off |

### Proposed text

answer: Keeping the record slows a build or a parallel test suite in a computer by no more than 5
percent, as the median over interleaved pairs of runs with the record on and off, on the reference
machine of `own-cost-targets`. Checkpoints are taken in both runs of a pair, since they have their
own speed target. Keeping the record also adds no wait longer than 100 milliseconds to one small
write inside the computer. The numbers are targets written before the code, as `own-cost-targets`
asks. Whether they can be kept is to find out, by `record-speed-cost`. If the measure says one
cannot, the number comes back to polarzero, and the record is not thinned quietly to meet it.

sets (replacing the `says` of `record-overhead-is-bounded`): "Keeping the record slows a build or a
parallel test suite in a computer by no more than 5 percent at the median of paired runs, and adds
no wait longer than 100 milliseconds to one write inside the computer."

### For the first build

It blocks `record-of-a-computer` only as a target. The first build's record should meet it easily
if the comparison costs what changed, and should miss it if it walks every file. That makes the
measurement a good test of the comparer, and it can only run on a Mac.

---

## Cross-cutting findings

1. **Two properties contradict each other.** `observing-never-waits` and
   `nothing-decided-unrecorded` cannot both hold when the room for decisions is full (details under
   `observing-and-waiting`). The fix is one clause in the first. A simulator that checks both, as
   both entries propose, would report it on the first long recorder outage.

2. **A down recorder stops the way back.** `events-until-the-recorder` applies
   `enforcing-and-recording` to the engine: when the table of unwritten events is full, a
   checkpoint at an interval waits. So if the recorder crashes at 01:00 and nothing restarts it,
   automatic checkpoints stop when the table fills, and in the morning there is no checkpoint from
   04:00 to restore to. In the first build the record is a convenience and the checkpoints are the
   product. Two things would fix it without breaking the principle: the engine restarts the
   recorder (nothing in the plan says who does; 02 notes the same), and the table is sized for days
   of clock-driven work. Better still, a checkpoint's own event could be kept in the engine's log on
   disk instead of in a table of fixed size in the core, which `engine-saved-state` already writes.
   Challenge to `events-until-the-recorder` (not taken, so it can still change): a checkpoint taken
   by the clock should never wait for the recorder.

3. **The comparison must cost what changed, and the plan does not say so anywhere.** The trial's
   "3 to 22 seconds for 200,000 files" is given in the notes of `file-changes-from-outside` and
   `file-changes-in-the-record` as the cost of a comparison. It is
   the cost of a walk, and a walk grows with the disk. polarzero's main will have a few million
   files. This one requirement decides whether `file-changes-in-the-record` works on a real Mac.

4. **Reading a hostile repository on the host is the same bug as GitSpawn.** Running `git status`
   on a checkpoint on the Mac would run the agent's `core.fsmonitor` on the Mac, outside every
   computer. Agent programs, a secret scanner and shell prompts have all shipped this bug. A new
   decision is proposed under `git-history-inside`.

5. **`silo changes` will mostly print `.git/` and `node_modules/`.** A commit writes objects, the
   index and the logs; `git gc` rewrites packs; `npm install` writes tens of thousands of files. The
   morning case of the first build ("which folder did the agent delete") needs lines merged by folder
   by default, the way `record-under-a-flood` merges under pressure. That is a question for the
   feature that adds the `silo` command: whether it prints merged lines by default and the full list
   on request. It is not a change to the record, which keeps every line.

6. **`macos-ships-facts-first` leaves file changes out.** Its answer lists what the record of a
   macOS computer holds ("what crosses its boundary, the model calls, the spend, the checkpoints and
   the screen") and does not name file changes. They are facts seen from outside, and they are the
   first build's main line. A reader of that entry alone would think a macOS computer has no file
   facts until the probe works.

7. **Thinning and the comparer must agree.** A checkpoint removed before it was compared makes the
   next comparison span it. That is fine if the line says which two checkpoints it covers, but the
   comparer must never compare a checkpoint with a successor it believes is adjacent when it is not.
   The comparer should read the pair from the record, not from the list of checkpoints on disk.

## Sources not verified here

- Codex's default `stream_idle_timeout_ms` and `stream_max_retries`: third-party pages disagree.
- `F_LOG2PHYS_EXT` on APFS clones: needs a Mac.
- That a write to the raw disk of a running macOS computer needs more than `sudo` while System
  Integrity Protection is on.
- libgit2 CVE-2024-24577 and CVE-2024-24575, Kafka's and auditd's buffer modes, TigerBeetle's split
  of metrics from its state machine: from memory.
- The CVE numbers of agent programs in the CSA note of September 2026.

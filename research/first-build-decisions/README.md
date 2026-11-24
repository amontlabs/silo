# First-build decisions: second opinions, and what to rule

For polarzero, 24 November 2026. Read this page first; the files below hold the evidence.

## What this is

The first-build proposal (draft PR #348) says 56 open
decisions block the smallest first build: main as a macOS virtual machine on your Mac, reached over
SSH, with checkpoints taken on their own, restore, and a record of which files changed. Nine agents
each gave an independent, adversarial second opinion on a group of those 56 (files 01 to 09), three
more researched risks the plan does not cover yet (10 to 12), and two more checked the facts of
01 to 12 (file 13; its five corrections are already applied in 07, 10 and 12). Nothing in `plan/` was changed, and
no verdict was written. To rule, read the headline findings, then go down the ruling sheet: each row
says what to do and which file holds the reason and the exact proposed text.

## File index

| File | What it covers |
|---|---|
| [01-engine-internals.md](01-engine-internals.md) | 8 decisions on the core: dependencies, inputs, state, source checks, TigerStyle, who reads the first files |
| [02-engine-persistence-cost.md](02-engine-persistence-cost.md) | 6 decisions on what the engine and the recorder save and what it costs; a 5-minute `F_FULLFSYNC` benchmark for your Mac |
| [03-engine-shape.md](03-engine-shape.md) | 5 decisions on the engine's shape: core and drivers, determinism, limits, reconciling, effects |
| [04-engine-protocol-network.md](04-engine-protocol-network.md) | 5 decisions on events, call ids, versions, who may call the engine, and the network stack (gvproxy measured on Linux) |
| [05-main.md](05-main.md) | 8 decisions on main: macOS first, one main, the two-VM limit, the image, first run, files, pause, stop |
| [06-reaching-and-agent-programs.md](06-reaching-and-agent-programs.md) | 5 decisions on SSH, the SSH settings file, documented surfaces, unmodified agent programs, `silo-is-light` |
| [07-going-back.md](07-going-back.md) | 7 decisions on checkpoints and restore; clone stalls measured on XFS and Btrfs; the APFS test script. Fact-checked at the end of 13: two claims corrected, no recommendation changed |
| [08-record-lines.md](08-record-lines.md) | 6 decisions on the record's words, sources, kinds of lines, authors and changes |
| [09-record-sources.md](09-record-sources.md) | 6 decisions on enforcing while recording, waiting, the record's path, Git, file changes from outside, overhead |
| [10-macos-unattended-setup.md](10-macos-unattended-setup.md) | How a new macOS computer gets past Setup Assistant with nobody clicking |
| [11-reading-apfs-from-outside.md](11-reading-apfs-from-outside.md) | How to read main's APFS disk from outside without trusting it; readers tested; a diff prototype |
| [12-landscape.md](12-landscape.md) | Incidents with agents running with checks off, and every product that deals with them |
| [13-fact-check.md](13-fact-check.md) | Fact check of 01 to 12, with 07 in its last section: almost all real; five claims wrong, all corrected in place, none changing a recommendation |

## Headline findings

These are the eight findings that most change the first build. Each names the files that back it.

### 1. Require macOS 27 on your Mac: it removes the Setup Assistant risk

macOS 27 added `VZMacGuestProvisioningOptions`: Silo passes a user name, a password, automatic
login and "turn on Remote Login", and Setup Assistant creates the user and turns SSH on by itself on
the first start. Tart's macOS 27 image template dropped 82 lines of keystrokes for it. It needs
macOS 27 on both the Mac and the computer, and works only on the first start after the install. On
macOS 26 every method is a workaround: Tart's keystroke script needed eight fixes in 14 months.

Case: you install Silo on a Mac running macOS 26.5, which is what your Mac ran on 21 November. The
first run has to say "update to macOS 27" and start nothing. No entry of the plan states a minimum
macOS yet.

There are two catches. A developer reported that provisioning fails when the computer's user name equals the
Mac's login name (FB23241619, no fix reported), which is exactly what `moving-in-on-macos` asks for;
file 10 gives a workaround with a setup user. And Apple's framework is not daemon safe, so the
program that holds main runs in your login session, and after a Mac restart main comes back only
once you log in.

Files: 05 (`computer-image`), 10 (whole file), 12 (3.3 a), 13 (verified the API, the forum report and
the error code; not that this error code is what the same-name case throws).

### 2. A checkpoint makes every write in main wait for the whole clone, and the wait grows with use

Measured on XFS: an 8 GiB image in 500,313 pieces took 11.0 s to clone, and a program writing to it
waited 10.7 s for one 4 KiB write. An agent-like workload with a checkpoint after each step reached
98,000 pieces after 30 checkpoints and stayed there; with a single clone, the same work on ext4
healed to about 21,000.
Frequent checkpoints keep the image at its worst. APFS keeps one record per piece too, so the same
growth is expected, at a speed nobody has published.

Case: main has run for three weeks. Claude Code runs `npm install` while the 14:05 checkpoint is
taken, and every write in main, the agent's conversation file included, stops for as long as the
clone lasts.

macOS 27's layered disk images (DiskImageKit) do not replace clones: the layers of an attached disk
cannot change while the computer runs (UTM's source), and Apple has no public API to merge a layer
back. A layer per boot is possible; a layer per checkpoint is not. The checkpoint's speed target
should be "the longest a write in the computer waits", not the clone time. File 07's script, part A,
measures APFS in about 20 minutes with no virtual machine.

Files: 07 (measurements, mitigations, script), 02 and 09 (`record-speed-cost`,
`record-overhead-limit`), 12 (3.3 b), 13 (no merge API: verified, and nobody ships forks of a local VM over a frozen base layer yet).

### 3. Read main's disk with a reader of Silo's own, by leaf block address, never by mounting it

An agent with `sudo` wrote every byte of main's disk. Mounting it on your Mac hands it to the Mac's
kernel, and Apple fixed an APFS kernel bug reached by "mounting a maliciously crafted disk image" in
macOS 26.7.2 (CVE-2026-84496). The readers off the shelf fail on cases main meets in normal use:
libfsapfs could not open a volume with 512 snapshots, and libfsapfs and apfs-fuse both refused a disk
whose newest APFS checkpoint was half written, which a clone of a running disk can be. So the
proposal's choice of libfsapfs does not hold.

APFS never rewrites a node where it was, so a leaf found at the same block in both checkpoints holds
the same records. A 250-line prototype diffed by leaf address and matched a full walk on 711 of 711
pairs of snapshots; on the 511 neighbouring pairs it read 6,260 blocks instead of 45,014. For 3
million files the estimate is about 26 to 50 MB read per comparison instead of 1.5 to 2.8 GB (not measured). A full walk does not fit:
2.1 million files took 33 s with a cold cache on Linux. A disk the reader cannot read, such as one
whose Data volume the agent encrypted, is a gap with how much of the disk changed, never "nothing
changed".

Files: 11 (readers tested, prototype, design, test plan), 09 (`file-changes-from-outside`), 08
(`word-gap`), 13 (CVEs and the libfsapfs fault verified).

### 4. On a Mac the engine must call `F_FULLFSYNC` itself, stop on a failed flush, and not log clock ticks

Zig's standard library syncs with plain `fsync` on macOS, which leaves data in the drive's cache. Go,
Rust, LMDB, PostgreSQL and TigerBeetle call `F_FULLFSYNC` there. An engine that uses Zig's sync
passes every test on Linux and loses its last inputs on a Mac.

When a flush fails, nobody knows what reached the disk. Case: `silo restore main` is written, its
flush fails, the engine refuses the call and carries on; the next flush carries the refused bytes,
and after a restart the replay applies a restore you were told never happened. The engine has to stop
instead; the caller gets "outcome unknown", and repeating the call with the same id after the restart
gives the real result.

The plan makes the clock an input and writes every input before applying it. With a tick a second
that is 86,400 forced flushes a day while nothing happens, and each `F_FULLFSYNC` drains the whole
drive queue, main's writes included. The fix three files agree on: the core says when it next needs
the time, and nothing it puts out leaves before its input is written, so writes group at no cost.

Files: 02 (`engine-saved-state`, the benchmark script), 01 (`engine-inputs`), 03
(`deterministic-core`), 13 (Zig's `fsync` verified).

### 5. A full disk is the likeliest failure, and today it would leave you unable to restore

main's disk image grows on the same APFS volume as the checkpoints and the engine's files. After a
clone, overwriting a shared block needs new space, so on a full Mac main's own writes fail, and the
engine's next write fails too. Once the engine stops, `when-the-engine-stops` says nothing changes
until it is back: no restore and no deleting a checkpoint to free space, which is what you would need.
`low-disk` (open, not in the first build) pauses computers at two levels but keeps nothing for the
engine itself. File 02 proposes a reserve file only the engine may delete, and stopping checkpoints
before the volume is full. File 05 adds that the first run must check for the 25.6 GB download and the
install before it starts.

Files: 02 (cross-cutting 2), 07 ("Main's disk on a full Mac"), 05 (`first-run-time-macos`).

### 6. Restore has holes: the saved checkpoint can be thinned away, and the SSH key goes back with the disk

- `restore-saves-first` saves where you were, but `thinning-checkpoints` can later remove that save,
  and it is the only copy of the work the restore left. Case: you restore at 16:00; after 18:00 the
  hour from 16:00 keeps one checkpoint, taken at an interval and descended from the old moment, and
  the 16:00 save goes.
- Silo reaches main with a key in `~/.ssh/authorized_keys` on main's disk. If Silo changes that key
  and you then restore to an older checkpoint, the old public key comes back and `ssh main` fails.
- A macOS computer is a disk plus its auxiliary storage; Tart and UTM copy both with every clone, and
  a checkpoint must too.
- After a restore, the next comparison must be made against the checkpoint the disk came from, not
  the one before in time, or every file the restore put back shows as changed by nobody.
- A restore also puts back an old login file; Codex's documentation names that as a reason to sign in
  again.

Files: 07 (`restore-saves-first`, `what-a-restore-leaves`, `checkpoint-per-system`), 08 (cross-cutting
1), 06 (`unmodified-agent-programs`).

### 7. Reaching main: gvproxy exposes your Mac's 127.0.0.1 by default, and Claude Code's login over SSH is untested

gvproxy, the proposed first driver of Silo's network stack, maps 192.168.127.254 inside the computer
to your Mac's 127.0.0.1 unless told otherwise. File 04 reached a server on the host's 127.0.0.1 from a
guest this way. Apple's NAT does not reach 127.0.0.1. Moving main onto gvproxy without removing that
entry gives main more reach on your Mac than it has today: a local database, model server or MCP
server becomes reachable. The engine's socket must never be TCP for the same reason.

On SSH: the user's own `Host *` settings still add a `RemoteForward` or a key to Silo's entries
(tested with `ssh -G`), the `Include` line must be the first line of `~/.ssh/config`, and a Mac cannot
tell your call to the engine from an agent on your Mac running `silo`.

The first-build story is "`ssh main`, start Claude Code, go to bed". Two sources disagree on whether
that works: Claude Code's documentation says that when the Keychain is locked, as in an SSH session,
the login goes to `~/.claude/.credentials.json` (06); an issue reports that over SSH it asks for
`/login` again (05, 10; closed for inactivity, no fix). One short Mac test settles it, and it should
be the second test you run.

Files: 04 (`network-stack`, `who-can-call-the-engine`, measurement), 06 (`computer-reached-over-ssh`,
`reaching-a-computer`), 05 and 10 (keychain), 13 (gvproxy default and the issue verified).

### 8. Fly Sprites already takes whole-system checkpoints on its own; what is unique is macOS, your Mac and the record

The fact-check corrected file 12: Fly's Sprites checkpoints itself "after a stretch of continuous
work, when it goes idle, and on graceful shutdown, keeping a tiered history", and restores in place.
It is Linux, in Fly's cloud, with no record of which files changed. Docker Sandboxes already keeps
keys out of the agent's reach at a proxy. Lima 2.x has experimental macOS guests and a diff-before-apply
loop for one project. Tart's team joined OpenAI. What no product combines is a macOS computer on your
own Mac, checkpoints of the whole disk taken on their own, restore in place, and a record of the
files that changed, for any agent program. File 12 argues the record and agent neutrality are what is
hard to copy, so `file-changes-in-the-record` deserves the most care of the twelve features.

Two consequences for what Silo says. The first build answers the most frequent incident (an agent
deleting files on its own machine: `rm -rf ~/`, `git reset --hard`, GPT-5.6 deleting `$HOME`) and
none of those in the news (Replit, PocketOS in 9 seconds through a Railway token, DataTalks'
`terraform destroy`), which destroyed something outside the machine. And `silo changes` will be
unreadable unless it hides what macOS writes by itself (Spotlight, caches, logs); the list should be
measured on a real main left idle for a night.

Files: 12, 13 (Sprites correction).

## The ruling sheet

All 56 decisions from the proposal's section "The 56 decisions", in its groups. Checked: every one
is open (`answered`, no verdict) in `plan/` today, and each is covered by exactly one file.

- **Recommendation** uses the brief's types. "Edit" means take with an edit; the exact text is in the
  file.
- **Who**: "agents" means the proposal lists it for agents with a judge (`owners-part-on-engine`);
  "polarzero" otherwise. A note marks where a file moves it.

### Could be taken by agents with a judge (12), and two that hold your words

| id | Recommendation | Confidence | Reason | File | Who |
|---|---|---|---|---|---|
| `core-dependencies` | Edit | high | "Only `std.debug.assert`" forbids the checked arithmetic the core needs; replace with a list of I/O-free parts of Zig's library, and say the loop around the core is simulated over fakes | 01 | agents |
| `core-knows-no-agents` | Edit | medium | Add the check, and say a call is known only by where it arrived: your Mac or one computer | 01 | agents |
| `core-state-stays-small` | Replace the answer | high | Its size test passes on any core with fixed tables, and "current rows only" contradicts `call-ids` and `events-until-the-recorder` | 01 | agents |
| `engine-inputs` | Edit | high | One queue lets an agent's loop of calls delay your restore; bound each source and keep room for driver results | 01 | agents |
| `engine-state` | Edit | medium | A reused table slot makes a pending restore act on the wrong checkpoint; ids come from a counter, never a position | 01 | agents |
| `engine-source-checks` | Edit | high | "Wrapping arithmetic on a value from an input" needs taint analysis nobody has; refuse the operators and `@setRuntimeSafety`, `@truncate`, casts; a planted fault per rule | 01 | agents |
| `engine-saved-state` | Edit | high on design, medium on cost | Four holes: `fsync` is not durable on a Mac, a refused call can land after a failed flush, a reused log replays old inputs, damage at the end is dropped unseen | 02 | agents in the proposal; 02 says yours, since the engine stopping on a failed flush is something you see |
| `events-until-the-recorder` | Edit | high | Holds only if the recorder forces its lines to disk before it acknowledges; combine with 09's change (section "Contradictions") | 02, 09 | agents |
| `record-speed-cost` | Edit | medium | With the record off, checkpoints must still be taken; add the longest write wait and the disk's sync mode | 02 | agents |
| `sources-number-their-lines` | Edit | high | A restart and a duplicate look the same; a count id from the recorder on every line, as journald and Kafka do | 02 | agents |
| `tigerstyle-by-place` | Edit | medium | Leaves out the Swift helper (`-O`, never `-Ounchecked`), the probe and the simulator; two rules cannot be checked as worded | 01 | agents |
| `own-cost-targets` | Edit | medium | Forced flushes cost more than bytes on an Apple SSD; add them, the engine's idle disk writes, a named reference Mac; hosted macOS runners cannot start a VM | 02 | agents |
| `how-the-engine-is-built` | Edit | medium | "A person reads line by line" names nobody; name the engineer before the build starts | 01 | polarzero |
| `measuring-the-store` | Edit (narrowed) | medium | Ship one design and measure it; the size and query targets do not exist yet, so they cannot hold a merge back | 02 | polarzero (only the two targets, later) |

### The engine's shape and its promises (10)

| id | Recommendation | Confidence | Reason | File |
|---|---|---|---|---|
| `engine-shape` | Take as written | high | The helper already makes everything asynchronous; the split makes restore racing a checkpoint testable. Add "one operation at a time per computer" in notes | 03 |
| `deterministic-core` | Edit | medium-high | Determinism holds for one build of the core; inputs saved by one build are never applied by another; the core says when it next needs the time | 03 |
| `engine-limits` | Edit | medium | A checkpoint the clock starts that meets a full table is refused with nobody to read it; with no thinning in the first build, the table fills in days | 03 |
| `engine-reconciles` | Edit | high | Its liveness property has no number, which breaks `liveness-has-a-number` (taken); add back-off, and ignore observations older than one applied | 03 |
| `how-effects-work` | Edit | medium-high | A timeout is "not known", not "failed"; the once-only key names what the effect makes (`checkpoints/412`); timeouts count only awake time | 03 |
| `numbered-events` | Edit | high | A rebuilt engine restarts its numbers; add a history id beside the number | 04 |
| `call-ids` | Edit | high | Use TigerBeetle's sessions, keep the table in the saved state, answer when applied; a new process repeating a command is not a repeat | 04 |
| `formats-carry-versions` | Edit | medium | A newer engine cannot replay an older engine's inputs: drain before update; version the `silo` JSON lines, which scripts parse | 04 |
| `who-can-call-the-engine` | Edit (narrowed) | medium-high | A Unix socket and `getpeereid` for the first build; nothing on a Mac tells your call from an agent's on your Mac; never TCP | 04 |
| `network-stack` | Edit | medium | gvproxy as first driver, with the NAT to 127.0.0.1 removed; Apple's NAT, shown as no boundary, only until the Mac test passes | 04 |

### Main (8)

| id | Recommendation | Confidence | Reason | File |
|---|---|---|---|---|
| `main-on-macos-first` | Take as written; fix the notes | high | Download is 25.6 GB, not "over 10 GB"; add Apple's purpose clause and that Docker cannot run in main | 05 |
| `one-main-or-several` | Take as written | high | Apple's limit of two macOS VMs: a second main takes the only place left for a fork | 05 |
| `neutral-core` | Edit | medium | The limit is shared with other apps and reportedly leaks a place after a stop (FB22967193); a refused start is a fact | 05 |
| `computer-image` | Split | high | Take the macOS half: keep the installed system before its first start, so each computer is provisioned on its own first start. Leave the Linux half open | 05, 10 |
| `first-run-time-macos` | Edit | high | 25.6 GB is 34 minutes at 100 Mbit/s; check free disk first and keep the Mac awake; time the provisioned first start too | 05, 10 |
| `files-through-a-channel` | Edit | medium | Its property fails its own hostile test in the first build; name the channels: the network, the way in, each import or export | 05 |
| `pause-every-computer` | Edit | medium | A Mac restart turns a pause into a stop; the computer comes back stopped, never running. "Main and two forks" cannot run | 05 |
| `what-stop-ends` | Edit | medium | How Apple asks a macOS guest to shut down is undocumented; "to find out" covers macOS too | 05 |

### Reaching main and the agent programs (4)

| id | Recommendation | Confidence | Reason | File |
|---|---|---|---|---|
| `computer-reached-over-ssh` | Edit | high on SSH, medium on edit | Your `Host *` forwards still apply and terminals can write your clipboard (OSC 52); ssh reaches main through Silo's `ProxyCommand` and a Unix socket | 06 |
| `reaching-a-computer` | Edit | medium | The `Include` line must be first (tested); the Codex app and Claude desktop still ask before a new host; main's network is open in the first build | 06 |
| `documented-surfaces` | Edit | high | A documented path is not a documented format (`transcript_path`); source code is not documentation | 06 |
| `unmodified-agent-programs` | Take as written; add notes | high | Both vendors' installs work over SSH; note that a restore brings back an old login | 06 |

### Going back (7)

| id | Recommendation | Confidence | Reason | File |
|---|---|---|---|---|
| `checkpoint-per-system` | Edit | medium | The open question is how long writes wait, not consistency; copy the auxiliary storage with the disk | 07 |
| `fork-shares-disk-blocks` | Take as written | high | A fork clones a checkpoint, which nothing writes, so main waits for nothing; no fork in the first build | 07 |
| `checkpoint-storage` | Edit | high | A clone lives only on the same APFS volume, not the same drive; the description written last, removed first | 07 |
| `restore-saves-first` | Edit | high | Stop first, then save; keep that save out of hourly thinning, since it is the only copy of the work left | 07 |
| `what-a-restore-leaves` | Edit | medium | A grant kept as a file inside, such as the SSH key, goes back with the disk unless Silo writes it again | 07 |
| `why-destructive` | Take as written | high | Files the three by what they really lose today | 07 |
| `meanings-for-computers` | Edit | medium | "A saved moment" suggests running programs, which a checkpoint does not keep; drop "nothing is lost" from pause | 07 |

### The record (12)

| id | Recommendation | Confidence | Reason | File |
|---|---|---|---|---|
| `word-line` | Take as written | high | One line of the record is one line of `silo` JSON output; no clash in `words.json` | 08 |
| `word-gap` | Edit | medium | Must cover "a source could not see": an unreadable or encrypted disk, a pair never compared | 08 |
| `sources` | Edit | medium | `silo` prints the exact source on every line; a source names its version, so a fixed comparer bug can be told apart | 08 |
| `kinds-of-lines` | Edit | medium | 567 of 616 "changed" lines in a trial had a size change of 0: say what changed; give the record's own facts a row; take only four rows now | 08 |
| `who-made-each-change` | Edit | high | Add "a computer" as an author, since an agent inside may call the engine; say what started Silo's own acts | 08 |
| `changes-to-the-record` | Take as written | high | Its two properties are what the first build keeps; run the disk reader apart from the recorder | 08 |
| `enforcing-and-recording` | Edit | medium | The room must survive a helper crash; a new model call waits before it is sent. Nothing of it is built in the first build, so taking it as written is fine for now | 09 |
| `observing-and-waiting` | Edit | high | `observing-never-waits` contradicts `nothing-decided-unrecorded` when the room is full; the disk source merges by comparing farther apart | 09 |
| `record-path-and-core` | Edit | high | Say that `silo changes` reaches the recorder through the engine program, never the core | 09 |
| `git-history-inside` | Take as written | high | "No" costs nothing; the real risk, running `git` on a hostile repository, goes in a new decision | 09 |
| `file-changes-from-outside` | Edit | medium | Require a comparison that costs what changed, no mount in a kernel, and a gap for an unreadable disk | 09, 11 |
| `record-overhead-limit` | Edit | medium | Name the statistic (median of paired runs), take checkpoints in both arms, and bound the longest wait (100 ms is your number to set) | 09 |

### The product (1)

| id | Recommendation | Confidence | Reason | File |
|---|---|---|---|---|
| `silo-is-light` | Edit | medium | "As little as it can" has no scale; measure each target against an idle computer (about 5 GB of memory, 54 GB of disk) | 06 |

### Counts

| Recommendation | Count |
|---|---|
| Take as written (two of them with notes to fix) | 9 |
| Take with an edit (two of them narrowed) | 45 |
| Replace the answer | 1 |
| Split | 1 |
| Not now, or drop | 0 |
| **Total** | **56** |

The nine to take as written: `engine-shape`, `main-on-macos-first`, `one-main-or-several`,
`unmodified-agent-programs`, `fork-shares-disk-blocks`, `why-destructive`, `word-line`,
`changes-to-the-record`, `git-history-inside`.

## Contradictions and overlaps between the files

Where two files touch the same entry or two related entries, this says which to adopt. Each point
should be said in one place, so where two files say the same thing, one entry is named to hold it.

### `engine-saved-state` (01, 02, 03, 04)

- 01 replaces "written before applied" with "nothing leaves before its input is written". 02's
  proposed text already includes that rule, plus `F_FULLFSYNC`, numbered inputs, two state places,
  the torn-write rule, the recorder-ahead check and the stop on a failed flush. **Adopt 02's text.**
- 03 offers another design: save the whole state (a few kilobytes) after each input that changes it,
  with no log to replay; on Linux it cost 3 to 4 times an append. 03's reason for it is the
  cross-version replay problem. 04's "drain before update" solves that problem under 02's design, so
  this need not hold the ruling. The Mac benchmark (02) can still favour it later; it changes no
  promise.
- 03 and 04 both want "a newer engine never replays another build's inputs". **Say it once:** the
  sentence "inputs saved by one build of the core are never applied by another" in
  `deterministic-core` (03's text), and the drain before an update in `formats-carry-versions` (04's
  text). 02's "to find out: what a newer engine does with an older format" is then answered and
  dropped.
- Who takes it: 02 is right that "the engine stops on a failed flush" is something you see, so by
  `owners-part-on-engine` it is yours, not the agents'. The same edit is needed in
  `engine-under-failure` (open, not in the 56): "a call whose save failed partway gets an error
  saying its outcome is unknown".

### The clock (01, 02, 03)

All three propose that the core says when it next needs the time, so a tick enters only when a
deadline is due. **Put it in `deterministic-core`** (03's text) and leave only "the clock enters when
the core asks" in `engine-inputs` (01). The idle flush count goes in `own-cost-targets` (02).

### `events-until-the-recorder` (02, 09; 01 touches it)

- 02's edit: the recorder acknowledges only lines it forced to disk; the engine refuses to start when
  the recorder holds a number past its own; room is kept for driver results; and "an operation the
  clock starts, such as a checkpoint, starts only when the room left after it still holds what is
  kept".
- 09 challenges exactly that last part: if the recorder crashes at 01:00 and nothing restarts it,
  automatic checkpoints stop when the table fills, and in the morning there is nothing to restore to.
  In the first build the checkpoints are the product. That also goes against `automatic-checkpoints`
  (taken: "Nobody has to remember").
- **Combine:** take 02's text, but replace its clock sentence with 09's position: the engine restarts
  the recorder when it stops, room is kept for the events of the clock's checkpoints for a set number
  of days, and a checkpoint taken by the clock never waits for the recorder. Since this decides
  whether checkpoints go on, it is worth your eye even though the proposal lists it for agents.
- 01's replacement of `core-state-stays-small` names this table as part of the saved state. That is
  consistent; keep both.

### `numbered-events`, `call-ids`, `formats-carry-versions`, `engine-saved-state` (04)

04 shows that these four each assume something of the others: events assume numbers never restart,
call ids assume the table survives a crash, formats let working state be rebuilt, and the saved state
leaves the start after damage open. **Rule the four in one sitting** with 04's three fixes: a history
id on events, the session table in the saved state, drain before update. 01's text for
`core-state-stays-small` says "recent call ids"; with 04's edit those become sessions, so its wording
follows. 01's `engine-state` (ids from a counter, never reused) and 03's `how-effects-work` (keys name
what an effect makes) agree with 04.

### `file-changes-from-outside` (07, 08, 09, 11)

- **Adopt 09's proposed text.** It already takes 11's two points: a comparison that costs what
  changed, and never mounting the disk.
- Renames: 11 shows APFS object ids only grow, so a rename can be written as a rename on a macOS
  computer. 09's text keeps "delete and create" for ext4 and assumes 11's change in `kinds-of-lines`.
  **Take both together**, or neither.
- Add 07's and 08's rule, which 09's text lacks: each file fact names its two checkpoints, and a
  checkpoint is compared with the one its disk came from, so after a restore with the one restored
  to, as Git compares a commit with its parent.
- One factual clash: 09 says a file written and put back with its times reset is seen through the
  inode's `write_generation_counter`. 11 found that counter did not move for a time change on its test
  disk and says it cannot be relied on. **Trust 11.** That case is caught only through the blocks the
  Mac sees written (`F_LOG2PHYS_EXT`), which is still a Mac test; 09's proposed text already keeps it
  as "to find out".
- The proposal names libfsapfs as the reader. 11 says no, with tests. Drop that from the proposal.

### `kinds-of-lines`, `word-gap`, `sources-number-their-lines` (02, 08, 09, 11, 12)

- `kinds-of-lines`: adopt 08's text (file row says what changed and names both checkpoints; the
  record's own facts and refused calls are control facts; only four rows in force now; a kind is
  added only by a release, and readers skip unknown kinds). Add 11's rename wording to the file row if
  you take it.
- `word-gap`: 08's edit ("lost, or a source could not see") covers 09's unreadable or encrypted disk.
  One word, no second kind.
- `sources-number-their-lines`: adopt 02's text (a count id given by the recorder), and add 08's
  sentence: a source that can produce its lines again from what Silo keeps, as the disk comparer can
  while both checkpoints exist, does so after a restart and writes a gap only for what it can no longer
  produce.
- 12's filter for what macOS writes by itself, and 09's "lines merged by folder by default", belong to
  the `silo` command's feature, not to the record, which keeps every line.

### Retention in the first build (03, 07, 08, 09, 12)

- 03: with fixed tables and no thinning, the checkpoint table fills in days; add a small rule, "keep
  the last N automatic checkpoints".
- 12: "keep 20" fails the overnight case; keep every checkpoint of the last 24 hours, then hourly.
- 07: the save before a restore must not be removed by that rule. 08 and 09: a checkpoint not yet
  compared must not be removed either, or its file facts are lost.
- **Combine** into one rule for the first build (a new entry, section "New entries"): keep every
  automatic checkpoint for at least 24 hours (288 at a 5-minute interval), then remove the oldest;
  never remove a save made before a restore or a checkpoint whose comparison is not written. Size the
  core's table from that.

### `what-a-restore-leaves` (07) against 09, 10 and 11

07 proposes that Silo writes the SSH key back onto the disk after a restore and before the start, "by
the way to write into a stopped disk from outside, which the unattended first start already needs".
But 10 recommends writing nothing onto the disk before the first start (provisioning does it), and 09
and 11 forbid mounting a disk an agent has written. So that way does not exist. My synthesis, not in
any file: for the first build, the key Silo uses to reach a computer never changes for the life of
that computer, so no restore can bring back a wrong one. If a key must change later, Silo keeps every
key it ever authorized for that computer, connects with whichever works after a restore, and rewrites
`authorized_keys` over SSH. 07's sentence for grants kept as files then stays as the rule.

### Setup before the first start (06, 10)

06 lists a Mac test for turning on Remote Login on a fresh disk before its first start, as Lima does.
10 recommends Apple's provisioning only, writing nothing to the disk. **Adopt 10**; 06's test is needed
only if Silo ever supports macOS 26 Macs.

### Claude Code's login over SSH (05, 06, 10)

06 reads Claude Code's documentation: with the Keychain locked over SSH, the login goes to
`~/.claude/.credentials.json`. 05 and 10 cite issue #29816, where it asks for `/login` again. Both
sources are real (13 verified the issue). Neither file ran it. The test (Mac test 2) settles it, and
the answer also decides what `fork-and-sign-in` can keep out of a copy (06, cross-cutting 1).

### Whether anything inside main calls the engine in the first build (01, 04, 08)

01 (`engine-inputs`) and 08 (`who-made-each-change`) assume an agent in main can run `silo` and call
the engine, as `what-an-agent-inside-may-do` (taken) allows in time. 04 says the first build's only
client is `silo` on your Mac and nothing inside main calls the engine. The proposal does not say. This
is yours to settle for the first build. Either way, the two edits are cheap and can be taken now: a
bound per source and the author "a computer" cost little and are needed the day `silo` runs inside.

### Network: transport, fallback, and saying the network is open (04, 06, 12)

- 04 and 06 agree: the SSH entry goes through a `ProxyCommand` of Silo's to a Unix socket, never a
  TCP port on your Mac. 06 adds `ProxyUseFdpass` and reading which app opened each connection. Take
  both; with 04's fallback to Apple's NAT, the same `ProxyCommand` reaches the guest's address, so
  your SSH settings do not change when the stack arrives.
- 06 and 12 both note that main's network is open from the first day, which `reaching-a-computer`'s
  "nothing else is open" and `channel-question`'s "none exists by default" (both open) do not allow
  for. **Say it once,** in the first build's own feature: main's network is open from when you make
  it, with your yes, and shown as a channel.

### Costs and their reference (02, 06, 07, 09)

- `record-speed-cost` (02) and `record-overhead-limit` (09) both say "checkpoints in both arms" and
  "the longest wait of one write". Put the method in `record-speed-cost` (modes named, the write-wait
  program) and the numbers in `record-overhead-limit` (5 percent median, 100 ms).
- `own-cost-targets` (02) and `silo-is-light` (06) both name a reference machine. Name it once, in
  `own-cost-targets`' notes; `silo-is-light` says only that targets are read against what an idle
  computer costs there. 06 suggests a 16 GB Mac as a second reference.
- The checkpoint's own target (`speed-budgets`, taken) is "the longest a write in the computer waits
  because of a checkpoint" (07).

### `computer-image` and `first-run-time-macos` (05, 10)

05 and 10 agree: keep the installed system before its first start. Take 05's text for the macOS half,
and add 10's sentence that the provisioned first start, until SSH answers, is part of what is timed.

### `checkpoint-per-system` (05, 07, 12)

05 and 12 present DiskImageKit overlays as a possible checkpoint mechanism; 12 says an overlay per
checkpoint "may avoid" the stall. 07 read UTM's source: the layers of an attached disk cannot change
while it runs. **Adopt 07**: a layer per boot is the only overlay form open, and it is mitigation 3 of
07's list. The extra measurement row 12 asks for should measure that form.

### `core-simulator` (taken by agents; 01, 03)

01 wants the simulator to run the loop that feeds the core over fakes, since crashes matter there and
not in the core. 03 wants liveness properties checked after faults stop, not after every step.
**Combine into one replacing decision**, which agents may take with a judge: "The core and the loop
that feeds it reach the disk, the clock and the drivers only through interfaces given at start, so a
simulator drives them with millions of random sequences of inputs, crashes included. It checks every
safety property after every step, and every liveness property within its number once its faults
stop."

### Conflicts with decisions in force

Checked with `plan show`:

- `engine-reconciles` as written conflicts with `liveness-has-a-number` (taken): its property
  `wanted-and-seen-agree` has no number. 03's edit fixes it. `multi-step-operations` (open, not in
  the 56) has the same fault in `crash-finishes-or-rolls-back`.
- `events-until-the-recorder` with 02's clock sentence goes against `automatic-checkpoints` (taken);
  see above.
- `fakes-share-contract-tests` (taken) already asks each real driver to pass its fake's contract
  tests, so 03's proposed driver contract test is in force already; only its "upgrade test" half is
  new.
- No other recommendation conflicts with a taken decision. Several files call an entry "in force"
  when it is open: `engine-under-failure` (02), `checkpoint-storage` (09 calls it taken),
  `file-changes-from-outside` (11). For those, the change is an ordinary edit, not a challenge.

## New entries proposed for the plan

| Entry | What it says | From | Whose call |
|---|---|---|---|
| Which macOS your Mac must run | Apple silicon and macOS 27 or later; on an older Mac the first run says so and starts nothing | 05, 10 | polarzero (a price the user pays) |
| How a new macOS computer gets its user and SSH | Apple's guest provisioning on the first start; then Silo sets the key and turns password login off over SSH; nothing typed, nothing written to the disk | 10 | agents |
| Retention in the first build | Keep every automatic checkpoint for at least 24 hours, then remove the oldest; never the save before a restore or an uncompared checkpoint | 03, 07, 08, 09, 12 | polarzero (what you can go back to) |
| Space the engine keeps for itself | A reserve file only the engine deletes, checkpoints stop before the volume is full; or bring `low-disk` into the first build with that added | 02, 07 | polarzero |
| Where Mac measurements run before a merge | Hosted macOS runners cannot start a VM; a named Mac runs them | 02 | polarzero (it is your Mac or a bought one) |
| `repositories-read-as-bytes` | Silo never runs `git` on the host against a computer's repository; its own reader runs nothing the repository names | 09 | agents (needed before `task-gets-one-commit`, not in the first build) |
| One operation at a time per computer | Checkpoint, restore, stop and start never overlap on one computer; later ones wait in a fixed table | 03 | agents (in `multi-step-operations` or its notes) |
| Upgrade test | Every release is tested on an upgrade from the one before, with computers running | 03 | agents |
| Swift rules for the helper | Which engine rules hold in Swift and how they are checked, before the first Swift file | 01 | agents (01 folds it into `tigerstyle-by-place`) |
| The engine restarts the recorder | Nothing in the plan says who restarts it | 02, 09 | agents |
| The disk reader runs apart | A separate process with no right to write the record, under a Seatbelt profile; later a small Linux computer | 08, 11 | agents |
| Main's network in the first build | Open from when you make main, with your yes, shown as a channel | 06, 12 | polarzero (part of the first build's feature) |
| What `silo changes` hides by default | A measured list of paths macOS writes by itself, with a flag to show all; lines merged by folder | 09, 12 | polarzero |
| What the first build says plainly | Checkpoints are not backups; what an agent does outside main is not undone; after a restore you may sign in again and the undone hours' conversation is only in the saved checkpoint | 06, 08, 12 | polarzero |
| `engine-outlived-by-computers` in the first build | A VM ends with its process; else every engine crash is a power cut of main, which the first build must then say | 05 | polarzero (order of features) |
| main comes back after your login | `engine-matches-wanted` says that after a Mac restart main waits for your login | 10 | an edit of that feature |
| The network stack reports its own faults | Stalls, refused connections and restarts as facts, so they are not read as the agent's doing | 04 | polarzero, second build |
| Credential files in main as facts | List which credential files exist in main, from outside; no protection promised | 12 | polarzero; 12 does not recommend it for the first build |
| A minimum gap between checkpoints | A precaution firing on every tool call would stall main on every call | 07 | later, with opinions |

## Challenges to decisions in force

Each stays in force until you change it.

- `core-simulator` (agents): crashes matter in the loop, not the core; liveness needs a quiet phase.
  Combined text above. (01, 03)
- `engine-client` (agents), notes: drop "ids with a generation"; with ids from a counter that only
  grows, one way of naming a row is enough. (01)
- `what-an-agent-inside-may-do`: "asks the user" over the `silo` command is answered by whatever runs
  it, possibly an agent on your Mac; it needs a channel an agent cannot drive, such as Touch ID or the
  app. (04)
- `idle-memory` and its property `idle-computer-gives-memory-back`: a macOS guest may not answer
  Apple's balloon, so the first thing your Mac would show false is this; consider "a Linux computer".
  (05)
- `where-it-runs`: Apple's licence allows macOS VMs in a cloud only as whole Macs leased to one user for
  24 hours; a cloud of macOS mains as a shared service is outside it. (05)
- `agents-drive-the-screen`: on macOS 27 a program started over SSH cannot get Accessibility or
  Screen Recording without SIP off; drive the screen from outside, through the framework. (10, as
  corrected by 13)
- `fuzz-untrusted-parsers`: for a filesystem reader the dangerous bug reads a different tree than
  macOS, without crashing; add "rejects any input another program could read differently". (11)
- `whole-computer-grain`: consistent the way a disk after a power cut is; keep it, but test
  `claude --resume` on a checkpoint taken mid-write. (07)
- `what-a-fact-carries`: a file fact also needs its stretch and its source's version; add them in
  `kinds-of-lines` and `sources`, not here. (08)
- `what-silo-is-not`: keep it; the line users feel is "your folder is mounted or not", which
  `device-not-shared` holds. (12)
- `device-not-shared`: keep it; it costs a live editor on your Mac, so make the editor over SSH work
  on the first day. (12)

## The Mac tests, merged

Deduplicated across the files and the proposal's eight. The first twelve can change a ruling or the
first build's design; the rest measure numbers for targets. "Estimate" marks a time no file gives.
The Mac scripts are inline in the files named. The Linux experiments' scripts are kept outside the
repository, in the project folder `research/2026-11-24-first-build-decisions-scripts/`.

| # | Test | What it decides | Time | Script or steps |
|---|---|---|---|---|
| 1 | First run on macOS 27.2: download, install, provisioned first start, timed; provision once with your Mac's login name and once with another (`try`, not `try?`) | `first-run-time-macos`; whether your user name works (`moving-in-on-macos`) or the setup-user workaround is needed | Download 34 min at 100 Mbit/s, then install and first start (unmeasured) | Steps in 10, "The steps" and test 1 |
| 2 | Claude Code and Codex login over SSH: log in, disconnect, reconnect, restart the computer, restore a checkpoint | Whether the first build's "start Claude Code, go to bed" holds; where the login lands (`fork-and-sign-in`) | About 15 min (estimate) | Steps in 10, test 6; 05, cross-cutting 3 |
| 3 | APFS clone time against fragmentation, raw and ASIF, with a writer, and deleting clones | Whether checkpoint stalls are a non-issue or need 07's mitigations; the checkpoint's target | About 20 min, no VM, 40 GB free | 07, script part A |
| 4 | Is a clone of a disk a running VM writes a moment in time, in each of 18 caching, sync and guest-flush combinations | The mode main's disk is attached in; whether a checkpoint needs a pause | About 30 min, needs Tart | 07, script part B |
| 5 | `F_LOG2PHYS_EXT` on two clones of an image: does it give which blocks differ, matching a byte comparison, and how fast | The comparer's check from the Mac's side; the "how much changed" of a gap; catching writes past APFS | Minutes (estimate) | No script; 07's `h.py extents` is the start; 11, test 8 |
| 6 | Cost of `fsync`, `F_BARRIERFSYNC`, `F_FULLFSYNC`, grouped and not, quiet and under a writer; overwrite after a clone takes new space | The engine's latency target; whether the space reserve is needed with no snapshot; 03's whole-state option | About 5 min | 02, end of file |
| 7 | gvproxy with a macOS guest: DHCP, MTU, `ssh main` through a Unix socket forward, 2,000 connections, CPU after sleep; `git clone` and `npm install` against Apple's NAT; from main, 192.168.127.254 and the Mac's LAN address reach nothing of the engine's | `network-stack`: gvproxy now, or Apple's NAT shown as no boundary | About an hour (estimate) | No Mac script; Linux relay in `netexp/` of the scripts folder; 04, cross-cutting 6 |
| 8 | Does the Codex app list hosts from an `Include`d file; do Claude desktop and the Codex app connect through `ProxyCommand` with `ProxyUseFdpass` | `reaching-a-computer` (Silo may have to write `Host` lines into `~/.ssh/config`); the SSH transport | About 15 min (estimate) | 06, cross-cutting 7 |
| 9 | Start a VM from a launchd user agent and from a system daemon with nobody logged in; restart a Mac with FileVault on | Where the engine runs; `engine-matches-wanted` waiting for your login | About 30 min (estimate) | 10, test 8 |
| 10 | Stop a computer 20 s into its first start, start it again with the same options | Whether the never-started copy is needed for retries | Minutes | 10, test 5 |
| 11 | `ls ~/Documents` and `ls ~/Desktop` over SSH on a provisioned computer | Where agents' work can live over SSH | Minutes | 10, test 4 |
| 12 | 20 stops of a busy macOS 27 guest: how many shut down within 30 s, does a place leak | `what-stop-ends`; handling a refused start (`neutral-core`) | About 30 min (estimate) | 05, `what-stop-ends` |
| 13 | Ten fresh computers from the installed copy: time to SSH, failures; and one made with a new machine identifier | The time limit before a retry; `computer-image` | About an hour (estimate) | 10, tests 2 and 7 |
| 14 | What provisioning leaves: FileVault, secure token, `softwareupdate`, first-login dialogs | The checks before main is called ready | Minutes | 10, test 3 |
| 15 | A real macOS computer under agent-like work, a checkpoint per step, a write-wait probe inside; a mid-run checkpoint restored, booted and checked | How fast a real guest fragments; the longest write wait; restore boots clean | 2 to 3 hours | 07, script part C (its probe is the write-wait program 02 and 09 ask for) |
| 16 | An idle main for a night: memory, disk, processor, and which files macOS changes by itself | The reference for every cost target (06); the default filter of `silo changes` (12, 08) | One night | No script |
| 17 | 100 clones of the image while `fio` writes inside: how often the newest APFS checkpoint is broken | How often the reader's fallback runs | About an hour (estimate) | 11, test 7 |
| 18 | Leaf-address diff against a full walk on about 3 million files | The real cost of a comparison (11's estimate) | After the reader exists | 11, test 6 |
| 19 | Raw writes from inside with SIP on; FileVault turned on inside; 100 local snapshots and an added volume | What the reader must report as a gap or a new volume | About an hour (estimate) | 11, tests 9 to 11 |
| 20 | The engine's socket: a caller of another user is refused; the peer pid walked to the app that started ssh; `LOCAL_PEERTOKEN` after `exec` (only if a signature check is ever considered) | `who-can-call-the-engine`; showing which app opened the way in | Minutes | 04, 06 |
| 21 | Whether Claude Code and Codex trust the macOS keychain's authorities | How model traffic is read in a macOS computer (second build) | Minutes | Proposal, test 8 |

## What was not verified

- Nothing was run on a Mac. Every macOS behaviour comes from Apple's pages, Tart's, Lima's and UTM's
  source, forums or published measurements.
- File 07's numbers are Linux (XFS, Btrfs) measurements; nothing is
  measured on APFS.
- The Zig claims in 01 come from documentation; downloading Zig was refused, so nothing was compiled.
- `F_FULLFSYNC` costs on Apple SSDs (about 20 ms on an M1, 3.1 to 3.8 ms on an M5) are third-party
  reports (02). Whether APFS overwrites unshared data in place is unsettled (02).
- gvproxy's throughput was measured on Linux with a relay; its behaviour with a macOS guest (DHCP,
  MTU) and on an Apple CPU is not known (04).
- The two-place leak after a stop is reported on macOS 26.5; whether macOS 27 fixes it, and whether a
  paused VM holds a place, is not known (05).
- That provisioning clears every Setup Assistant screen, what a cut-off first start does, and that
  `guestProvisioningInvalidUsername` is what the same-name case throws (10, 13).
- Whether Claude Code's login over SSH lands in the file or fails (05, 06 and 10 disagree).
- Whether the Codex app follows `Include`, and whether a known-hosts file of Silo's skips Claude
  desktop's first-connection question (06).
- `LOCAL_PEERPID` on a Mac (06), and whether the audit token's pid version changes on `exec` (04).
- `F_LOG2PHYS_EXT` on APFS clones, raw writes with SIP on, and the leaf diff's speed on millions of
  files (09, 11). Whether FileVault can be turned on in a guest: 09 cites Eclectic Light that it can,
  08 and 11 left it open.
- Hosted macOS runners lacking nested virtualization rests on community reports (02).
- TigerBeetle not logging clock ticks and its two-phase liveness checks are from memory (03).
- Codex's stream timeout and retry defaults (09); Fly Sprites shipping agents preinstalled (13: not
  on Fly's pages); the Cowork escape rests on one secondary source (13); some incident details come
  only from mirrors and press summaries (12).

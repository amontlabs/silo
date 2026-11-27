# Step 5: the record, and the morning

Part of the build specs in this folder ([README](README.md)). It builds
`file-changes-in-the-record`, `a-path-from-a-checkpoint` and `files-imported-from-your-mac`, and
the rest of `record-of-a-computer`, whose format and recorder step 1 already built (#348). It needs
step 4. The formats are in [formats.md](formats.md), "The disk reader" and "A file fact".

## What you can do when it is done

At 06:58, after a night of three agents in main:

```
$ silo
main · running · 8 h since you last looked (Mon 22:58)

Since Mon 22:58
  96 checkpoints · 7,412 files changed · 3,412 deleted · disk +2.1 GB
  The Mac stayed awake. Main did not restart.

Outside your repositories (~/work/app, ~/work/client-b)
  deleted   ~/work/shared-fixtures/                       3,412 files  −1.2 GB   02:40 → 02:45
  changed   ~/.gitconfig                                                         02:20 → 02:25
  changed   ~/.claude/CLAUDE.md                                                  04:10 → 04:15
  new       ~/Library/LaunchAgents/homebrew.mxcl.postgresql@17.plist             03:40 → 03:45

next: silo changes --since 22:58 · silo look 02:40 ~/work/shared-fixtures

$ silo look 02:40 ~/work/shared-fixtures
main at 02:40 · read only
  ~/work/shared-fixtures/    3,412 files · 1.2 GB
    payments/   1,904 files     changed Mon 17:20
    auth/         880 files     changed Mon 17:20
    README.md     2 KB          changed Fri 11:02
    … 3 more
Now: ~/work/shared-fixtures/ is not in main.

$ silo put-back 02:40 ~/work/shared-fixtures
This copies 3,412 files (1.2 GB) from 02:40 into main, at ~/work/shared-fixtures.
Nothing is at that path now, so nothing in main is replaced. Main keeps running.
Silo saves a checkpoint of now first.
Put back? [y/N] y
  checkpoint 07:01 saved, kept until you remove it
  copying   3,412 / 3,412 files   1.2 GB   14 s
Put back ~/work/shared-fixtures from 02:40.
```

Claude Code's night in the other repository is untouched. The command shapes follow #354's
`details/cli.md`, and "Beside the morning review prototype" below says how they line up with #371.

## The pieces

| Piece | Where | What it does |
|---|---|---|
| The disk reader | `engine/reader/`, a program of its own, `silo-reader` | Reads a macOS disk image as bytes, metadata only unless asked for a file's contents. Strict: what it cannot read for sure, it refuses |
| The comparer | `drivers/compare.zig` | After each checkpoint, runs the reader on it and its parent, and sends the file facts to the recorder |
| The command's views | `cli/` | `silo` with no argument, `silo changes`, `silo look` |
| Put-back | `core/operations.zig` and `drivers/put_back.zig` | An operation of steps: a checkpoint of now, then a copy into the running computer over SSH |
| Import | `drivers/import.zig` | A copy from your Mac into the computer over SSH, once |
| Shipped data | `engine/data/` | What macOS writes by itself, folders that a build makes again, the places people worry about |

### The disk reader

#352 file 11 is the design, and its prototype the start: a reader of Silo's own, never a mount,
because a mount hands a disk an agent wrote with `sudo` to the Mac's kernel, and the readers that
exist fail on disks main makes in normal use.

- **What it reads.** The GPT, the APFS container and its checkpoints, the object maps, the
  volumes, the snapshot tree, and the file-system tree: inodes, directory records, the names of
  extended attributes, extents. Not file contents, except for `look` and `put-back` (below).
- **Strict.** It checks every rule of the format that the comparison depends on, with a budget of
  blocks, a limit of depth and a check for cycles. A disk that breaks a rule gives no comparison:
  it gives a gap that names the rule.
- **The newest checkpoint that reads cleanly.** A clone of a running disk can end on an APFS
  checkpoint not fully written. The reader opens the newest one whose objects all check, as macOS
  does, and says which it read and which it skipped. #359's test 17 says how often that happens.
- **By leaf address.** APFS never writes a node back where it was, so a leaf of the file-system
  tree at the same block in both checkpoints holds the same records. The comparison reads the
  index nodes and only the leaves whose addresses differ. For each checkpoint the reader keeps its
  list of leaves (`ckpt/<id>/leaves`, about 3 MB for 3 million files), so the next comparison
  reads only the new checkpoint's side. The prototype matched a full walk on 711 of 711 pairs.
- **Where it runs.** A process of its own with no rights: it gets the two image files opened read
  only and a pipe for its output, under a sandbox profile (`sandbox_init`) that denies every other
  file, the network and every other process. The engine reads its output as untrusted, with the
  strict format in [formats.md](formats.md). A later build moves the same program into a small
  Linux computer of its own (#352 file 11).
- **Language.** Zig, `ReleaseSafe`, under the rules `tigerstyle-by-place` gives the helper and the
  recorder: a memory cap fixed at start, fuzzed (`fuzz-untrusted-parsers`).

What it gives for each case main meets, from #352 file 11:

| In main | The reader gives |
|---|---|
| Ordinary work | The paths created, changed and deleted |
| A clone taken during a write | The newest clean APFS checkpoint, and the one skipped |
| `tmutil localsnapshot` many times, a new volume | Read, and the new volume named |
| FileVault turned on inside | A gap: "files of volume Data not read: encrypted", with how many nodes changed |
| A disk that breaks a rule | A gap naming the rule; the next comparison is against the last checkpoint that read cleanly |
| A file created and deleted between two checkpoints | Nothing: the record says its file facts have the grain of two checkpoints |

### File facts

The comparer runs after each checkpoint, once the checkpoint is renamed into place, at background
priority (`QOS_CLASS_BACKGROUND`), one at a time. It compares the checkpoint with its `parent`
(step 4): after a restore, that is the checkpoint restored to, as Git compares a commit with its
parent.

- Each changed path is one file fact ([formats.md](formats.md), "A file fact"): created, changed
  or deleted, the type, the size before and after, and for a change which of content, mode, owner,
  link target and extended attributes changed (#352 file 08).
- A rename is written as a delete and a create, as `file-changes-in-the-record` says. Each line
  carries the file's APFS object id, which APFS never gives to another file, so a later reader can
  join the two (#352 file 11 challenges this; the README lists it).
- Each pair ends with one closing line, `compared`, with the count of its file facts, or with a
  gap. A pair with neither is a pair the comparer has not finished, and `silo changes` says so
  (#352 file 08).
- File facts go from the comparer to the recorder directly, as a source (`sources-number-their-lines`),
  never through the core (`record-path-and-core`). The core gets one input per pair, `compared` or
  `gap`, which lets thinning remove the checkpoint.
- **Falling behind.** When checkpoints come faster than the comparer, it compares the newest with
  the last one it compared, skipping those between, and its lines name the two it compared. It
  loses nothing while both exist (#352 file 09). The skipped checkpoints stay `uncompared` only
  until a later pair covers them.
- **A pair of a million changes** is written as a million lines; the comparer is not live, so it
  never needs to merge. The command folds them when it prints.

### The command

`silo` with no argument, `silo changes`, `silo look` read the record and the checkpoints. They
never change anything. Their folding is the command's, not the record's: the record keeps every
line (#352, on `kinds-of-lines`).

- **Since you last looked.** `silo` with no argument starts from the time it last printed this
  screen to you, kept in `<data>/views/last-looked`. It is not a fact and not in the record. No
  entry has it yet (#351, record-ux, part 4.1).
- **Your repositories** are the folders of main that hold a `.git` folder in the newest
  checkpoint, read by the reader. A filter on paths, a fact, recomputed each time.
- **What macOS writes by itself** (Spotlight, caches, logs) is a list of paths shipped in
  `engine/data/macos-noise.zon`, made from #359's test 16, an idle main for a night. `silo changes`
  shows those paths in a group of their own, folded; `--all` unfolds it.
- **Folders a build makes again** (`node_modules`, `.venv`, `target`, `DerivedData`) are folded
  when the lock file beside them says so, from `engine/data/rebuildable.zon`. A fact about paths,
  not an opinion; `--all` unfolds them.
- **No path is hidden.** Every changed path is in exactly one group, and the totals add up
  (#351, record-ux, sketch d).
- **Times** are local, and a change is shown as the window between two checkpoints,
  `02:40 → 02:45`, never as one moment (#351, record-ux, part 4.14).
- **No "unattributed" on every line.** The first build has no probe, so no file change has an
  author. `silo changes` says once: "The first build does not know which program changed a file."
  (#352 file 08).
- **Text an agent wrote** is printed marked, never bare: a file name or a file's contents that
  `look` prints goes between `«` and `»`, and control characters are escaped, so a file named with
  terminal escapes cannot drive your terminal.

| Command | Prints |
|---|---|
| `silo` | What needs you, then what changed since you last looked, outside your repositories first |
| `silo changes [computer] [--since T] [--until T] [--all] [--json]` | The changed paths between two moments, grouped: outside your repositories, each repository, rebuildable folders, macOS itself |
| `silo look [computer] <time> <path>` | A folder as it was: its entries, sizes and times. A file: its metadata, then its contents if it is text under 1 MB |
| `silo put-back [computer] <time> <path>…` | Copies the paths back into the running computer, after a checkpoint of now |
| `silo import <path on your Mac> [<path in main>]` | Copies a file or a folder in, once |
| `silo record [computer] [--json]` | Every line, as stored (step 1) |
| `silo verify [computer]` | Checks every chain of the record: `48,211 lines checked · every chain holds`, or names the first line whose `prev` does not match (#371) |

With `--json` each command prints one object per line with `"v":1`; `silo changes --json` prints
the file facts themselves, as stored.

### Put-back

`putting-back-a-path` (#348): you name the paths, Silo saves a checkpoint of now, and copies them
from the checkpoint into the running computer. #365 tried the rule on Linux and found how it has to
be done.

| Step | What happens |
|---|---|
| 1. Read | The reader lists each path in the checkpoint. A path not there is refused before anything happens |
| 2. Show | What will be copied, and what it replaces now: from the newest file facts, and from a listing over SSH of the same paths in main |
| 3. Save | A checkpoint of now, reason `before-put-back`, kept until you remove it |
| 4. Copy | The reader streams the paths' contents as a `tar` stream; `silo proxy` carries it into main, where `sudo tar -x` writes it beside the target, in `<parent>/.silo-put-back-<id>/` |
| 5. Swap | In main, the current path, if any, is moved aside and the copy renamed into its place; then the old one is removed. A program never reads half a file (#365) |
| 6. Record | A control fact: you put back these paths from this checkpoint, so many files and bytes (`putting-back-a-path`) |

- **It writes through the computer.** Main's own kernel has its disk mounted, so nothing may write
  the image from outside while it runs (#365).
- **A write between step 2 and step 5 is lost** for the paths put back. #365 proposed pausing the
  computer to close that window, but a paused computer cannot run the copy. The checkpoint of step
  3 holds the state before the swap, so the lost write is in it, and the next comparison shows it.
- **Contents.** For `look` and `put-back` the reader also reads a file's data: its extents, and
  files compressed by APFS (zlib, LZVN, LZFSE in `decmpfs`). LZVN and LZFSE come from Apple's
  open-source `lzfse` library, pinned and linked into the reader only, which `tigerstyle-by-place`
  allows outside the core. A file it cannot read (encrypted, or compressed in a way it does not
  know) is named and not copied, and the rest goes on.
- **Owners, modes and extended attributes** come back as they were: `tar` carries them, and
  `sudo tar -x --same-owner` keeps them.

### Import

`files-imported-from-your-mac`: `silo import ~/Downloads/data.csv` copies the file into main at
`~/Imports/data.csv`, or at the path you give, over SSH. It is copied once; nothing stays shared
(`device-not-shared`). The record holds a control fact: you imported this path, so many bytes. A
file brought out of main later lands in one folder on your Mac with the quarantine flag (#352 file
05); that is not in the first build.

## Beside the morning review prototype

#371 tried the whole morning on Linux, with real Claude Code and Codex runs, a probe, a boundary
and classifiers. The first build has none of those three, so its morning is the part of #371 made
of checkpoints and file facts. Where the two differ:

| | #371 | These specs | Why |
|---|---|---|---|
| Files of a computer's record | three, each with its chain: facts, reports, opinions | one chain for facts and reports; opinions, from a later build, in a file and chain of their own | `what-the-record-holds` keeps opinions apart. A fact and a report are never mixed in one line (`line-never-mixes`), so one chain can hold both, in the order the recorder wrote them |
| Keys of a line | `id` `main:f12`, `kind` fact or report, `line` file or `model call`, `at` local time with offset, `source` a string, `by` a string | `seq`, `is`, `kind` with `model-call`, `time` in UTC, `source` with its count, `author` with the person | `kinds-of-lines` calls file and control kinds of lines. A source needs its count to show a gap (`sources-number-their-lines`). A person is in every control fact made by you (#362). UTC is a full time with its offset; the command prints local time |
| `v` on each line | no | yes | `formats-carry-versions`: scripts parse `silo --json` |
| File contents in the record | `sha_before`, `sha_after`, into a store by hash | not in the first build | A hash needs every changed file read; the first build's reader reads metadata only, and reads contents only for `look` and `put-back`. Adding hashes later is a new key, not a new format |
| Who writes the record | the engine and `silo putback` both wrote one file, which broke the chain, and `verify` caught it | only the recorder writes; a put-back's fact is an event of the engine | #371's finding is the reason for one writer |
| Put back | `silo putback <alert>`, from the checkpoint before the alert; refuses a path that changed since | `silo put-back <time> <path>…`; shows what a path replaces now, saves a checkpoint, and asks | The first build has no alerts. With no runs, Silo cannot tell an agent's later edit from the damage, so it shows and asks rather than refuses. Once alerts exist, `silo put-back <alert>` names the time and the paths |
| A diff of two moments | `silo diff`, with books that balance | `silo changes`, with the same groups and totals | #354 names it `changes`; `disk-place` asks for the groups. Either name is fine; one has to be chosen |
| Chain, receipt, timeline, network, why | yes | not in the first build | They need runs, the probe and the boundary |
| `verify` | `silo verify` | `silo verify` | The same |

#371 also challenges `the-chain`: a report is matched to a tool call by the command's text, not by
time. The first build writes no report, so nothing here depends on it.

## What happens on a crash

| What stops | What happens |
|---|---|
| The reader | The pair has no closing line. The comparer runs it again; a reader that crashes twice on one pair gives a gap naming the crash, and the next pair compares with the last good checkpoint |
| The comparer, or the engine | Lines it sent and the recorder acknowledged are kept; the pair is compared again, and the recorder writes each line once |
| The recorder | The comparer keeps its unacknowledged lines and sends them again |
| A put-back, during the copy | `.silo-put-back-<id>/` is left in main; the operation removes it and copies again |
| A put-back, during the swap | The operation reads in main which of the two names holds what, and finishes |

## Cost targets

| Measure | Target |
|---|---|
| A comparison of a pair with 1,000 changed files, on a disk of 3 million files | under 5 s, reading under 50 MB (#352 file 11 estimates 26 to 50 MB) |
| A comparison with nothing changed | under 1 s |
| The reader's memory | under 256 MB, fixed at start |
| Main's writes during a comparison | no wait longer than the checkpoint's target: the reader reads clones, never the live image |
| `silo` with no argument, after a night | under 2 s |

## Tests

| Test | Checks | Where |
|---|---|---|
| The reader on every disk of dissect.apfs's test set | The same files and metadata as a second reader | CI |
| Leaf-address comparison against a full walk, on the 511 neighbouring and 200 random pairs of #352's disk | Equal results | CI |
| The disk with a broken newest APFS checkpoint | Opens the one before, and says which it skipped | CI |
| One planted fault per rule (keys out of order, a cycle, a node at the wrong level, a newer object, a leaf twice, a hidden file) | A gap naming the rule, never a comparison and never a crash (`check-must-catch-a-fault`) | CI |
| Fuzzing each parser and the whole reader, with a block budget so a hang fails | `fuzz-untrusted-parsers` | CI on fixed seeds, nightly longer |
| The sandbox: the reader asked to open another file, a socket, or to start a process | Each refused by the system | polarzero's Mac |
| #365's put-back scenario on a macOS computer: a folder of 20,000 files deleted by one agent while another edits 2 of them | The folder comes back; the 2 edits are shown, not overwritten without saying | polarzero's Mac |
| `silo changes` against a full walk of both checkpoints, on main after a night | Equal; its cost follows what changed | polarzero's Mac |
| FileVault turned on inside main | A gap, never "nothing changed" | polarzero's Mac |
| #359 tests 05, 16, 17, 18, 19 | Blocks two clones differ in, what macOS writes by itself, broken checkpoints, the comparison at scale, what the reader meets inside a real computer | polarzero's Mac, before step 5 |
| The record's chain over a night | `shasum` of each line matches the next line's `prev` | CI and polarzero's Mac |

## Done when

- the reader is fuzzed, and refuses any disk another program could read differently;
- on main after a night, `silo changes` matches a full walk of both checkpoints, and costs what
  changed, not what exists;
- an agent that turns FileVault on inside main produces a gap, never "nothing changed";
- `silo put-back` brings back a deleted folder while another agent keeps working, and the record
  says who put back what;
- by default `silo changes` folds what macOS writes by itself, from the list test 16 measured.

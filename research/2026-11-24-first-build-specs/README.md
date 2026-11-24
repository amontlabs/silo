# Build specs for steps 1 to 3 of the first build

An agent's work, 24 November 2026, for polarzero and for the agents who will build. #348 cuts the
first build into six steps, numbered 0 to 5 in its `research/2026-11-24-first-build-milestones.md`:
the Mac tests, the engine, main runs, a night, the way back, the record. These pages specify
steps 1 to 3 well enough to start building them the day their features are taken: the engine, main
runs, and a night.

Nothing here is a decision. No entry of the plan gets a verdict, and nothing may be built from
these pages before polarzero takes the features they serve (`AGENTS.md`). Where a page rests on an
entry that is still open, it says which option it assumes, and the table below says what changes
under each other option. The same notes are on those entries in the Workshop.

## The pages

| Page | Holds |
|---|---|
| [formats.md](formats.md) | Every file and message of steps 1 to 3: where Silo keeps its files, the engine's saved state, the engine's protocol, the helper's protocol, the record with its hash chain and its person, and how a source sends lines |
| [1-engine.md](1-engine.md) | Step 1: the engine, the recorder and the `silo` command, with no computer yet. The core's tables and inputs, the access model, the loop, start, crashes, the Zig layout, cost targets, tests |
| [2-main-runs.md](2-main-runs.md) | Step 2: the Swift helper, the network, the way in over SSH, the first run as one operation of steps, stop and pause, a restart of the Mac |
| [3-a-night.md](3-a-night.md) | Step 3: `ssh main.keep`, the Mac kept awake, its sleeps and wakes in the record, main running on while the engine restarts |

Read `formats.md` first: the three steps point to it and repeat none of it.

## What these specs rest on

- The plan at `1b8f191` and the drafts of 24 November: #348 (the first build and its new entries),
  #352 (second opinions on the 56 decisions, with the proposed text this spec follows), #359 (the
  Mac kit, whose test numbers are used everywhere here), #362 (the record's hash chain and person),
  #364 (the reconciliation of the drafts), #365 (the checkpoint engine's prototype and catalog),
  #366 (Linux hosts), and #347's boundary checks (`research/2026-11-24-boundary-checks.md` in the
  project's files).
- Where #352 proposes an edit to an open entry, the spec follows the edit, and says so. Where #364
  proposes a position between drafts, the spec follows #364.
- Nothing here has run. Every step names the #359 tests it needs on a Mac before it starts or
  before it is done.

## What these specs change in #348's steps

Five findings from writing the specs move something in #348's plan of steps. Each is a proposal.

1. **The recorder belongs in step 1.** `events-until-the-recorder` keeps the engine's events in
   a table of fixed size until the recorder has them. With no recorder, that table fills, and the
   engine has to refuse calls or drop events. Step 3 also writes the Mac's sleeps in the record.
   So step 1 builds the recorder and the record's format, and step 5 adds the file facts and the
   commands that read them. This builds part of `record-of-a-computer` in step 1, so that feature
   has to be taken with step 1's.
2. **A closed network is not possible on Apple's NAT.** A macOS computer with no network device
   has no SSH either, so `network-before-rules`' "or closed" works only once the patched gvproxy
   carries main's network. Until then `silo setup` offers open only, and says why.
3. **Open on Apple's NAT reaches your local network,** and services on your Mac that listen on every
   address. #364 says so; `network-before-rules`' "never your Mac, your local network" holds only
   with the patched gvproxy. Step 2's first question says it in those words.
4. **Each helper is a launchd job of its own**, not a child of the engine. That is what lets a
   computer outlive the engine (step 3), and it needs nothing else: no re-parenting and no double
   fork. #359's test 09 checks that a user agent can start a macOS computer.
5. **The command needs `silo install` in step 1**, to put the engine and the recorder in launchd,
   since there is no app to do it. `silo setup` in step 2 runs it if it was not run.

## The open entries these specs assume

polarzero's call unless "agents" is written. "What changes" is the part of the specs that moves
under the other options.

| Entry | The specs assume | Other options, and what changes |
|---|---|---|
| `network-stack`, `network-before-rules` | Apple's NAT until a patched gvproxy passes #359 test 07, then gvproxy (#364, section 8) | gvproxy from the first day: step 2 waits for the patch and test 07; the helper's attachment and `silo proxy`'s dial change, nothing else. macOS 27's vmnet networks: a private network with no route to the Mac, still no rule per request; only the helper's attachment changes |
| `mac-silo-needs` | macOS 27 or later, Apple's guest provisioning | macOS 26 too: steps 6 and 8 of the first run become a keystroke script or writing Apple's private formats onto the disk; every other part stays |
| `computer-image` | Keep the installed system before its first start (#352's macOS half) | Keep a set-up system instead: every new computer is then a fork of it and needs `fork-identity` first |
| `when-the-engine-stops`, `processes` | A helper per computer, which outlives the engine | No helper: the engine holds main, a crash of the engine ends main, and step 3's `engine-outlived-by-computers` cannot be built |
| `engine-takes-back-helpers` | The engine reconnects to each helper's socket, and both sides prove they hold the computer's key | It cannot be done on a Mac: the engine stops main at its own start, and step 3 promises only that main runs while the engine is down |
| `engine-saved-state`, agents with #352's edit (#352 file 02 says it is polarzero's, since the engine stopping on a failed flush is seen) | A log and two state places, `F_FULLFSYNC`, a stop on any failed write | A save of the whole state after each input, no log (#352 file 03): `engine/log` goes, each input writes a state place; three to four times the cost per input on Linux |
| `call-ids` | Sessions, as TigerBeetle (#352 file 04) | Random ids kept in a table: `register` goes, and an id too old for the table can no longer be told from a new one |
| `numbered-events` | A history id beside the number (#352 file 04) | No history id: the field goes from the protocol and the saves; a client that misses a rebuild shows a state the engine no longer has |
| `formats-carry-versions` | A version on every file, a version per connection, `"v"` on each line of `silo --json`, a drain before an update | A version on every message: a field in every header, nothing else |
| `who-can-call-the-engine` | Same user, by `getpeereid` on a Unix socket | Also Silo's signature: an XPC path on the Mac beside the socket. A person confirms what grants more: a prompt, which is a choice about what you see |
| `events-until-the-recorder`, agents | Events wait in the engine's ring; a call that would leave too little room is refused with `record_behind`. Checkpoints the clock starts (step 4) never wait for the recorder (#352, the combined position) | Checkpoints wait for room in the ring: a recorder down at 01:00 stops the night's checkpoints. Not in steps 1 to 3, but it sizes the ring |
| `record-of-a-computer` (a feature) | Its format and the recorder are built in step 1 | Not taken with step 1: the recorder waits for step 5, step 1 needs a stub that acknowledges events it drops, and step 3's sleeps are not written until step 5 |
| `record-edits-show` (#362) | Every line carries the hash of the line before; the head is rewritten every hour | Dropped: the `prev` key goes. Added later: a new line format, and records written before it have no chain |
| `person-in-a-control-fact` (#362) | Every control fact made by you names your person, `p1` | Dropped: the `person` key goes, and a shared host later needs a new format |
| `who-made-each-change` | Adds an author `device` for what the Mac does by itself, such as a sleep | The sleep is written with author `silo` and the cause `mac-sleep`, which says Silo did what it did not. Or a new kind of line, which `kinds-of-lines` keeps short and fixed |
| `kinds-of-lines` | `control`, `gap` and `silence` in steps 1 to 3, and `connection` for the way in | |
| `what-stop-ends` | 30 seconds after `requestStop()`, then `stop()` (#352 file 05) | Another grace: one number in the helper |
| `pause-every-computer` | A paused computer whose helper ended comes back stopped (#352 file 05) | |
| `mac-kept-awake` (#348) | On power only, and say what a closed lid does | On battery too: one condition in the helper |
| `runs-outlive-the-terminal` (#348) | `main.keep` opens tmux | Nothing: the second entry goes, and you start tmux yourself |
| `main-set-up-for-agents` (#348) | The list is data, installed by each vendor's installer, after your yes | A blank main: step 9 of the first run goes |
| `space-the-engine-keeps` (#348) | A reserve file made in step 2, 4 GiB until #359 test 06 measures it | |
| `model-of-access-first` (taken; #348 challenges it) | The access model is the first code, over channels, secrets and hosts, as `access-model-checked` says | #348's replacement: the grants of the second build written down first, then the model against them. The exhaustive test's alphabet changes |
| Calls from inside main in the first build (#352: files 01 and 08 against 04) | None. The core has a place for the source `computer`, and nothing feeds it | Yes: the helper passes a call from inside on, tagged with its computer; it needs a way out of main that is not the network, such as vsock |
| `checkpoint-per-system` | The disk's caching and sync modes chosen by #359 test 04 | The NBD attachment (#365): only the helper's disk attachment changes in step 2 |
| `engine-limits`, agents | The table sizes in `1-engine.md` | Other sizes: one file, `core/limits.zig` |
| `how-the-engine-is-built` | The first files of `core/` are read line by line by the people polarzero names | polarzero has to name them before step 1 starts |

## The Mac tests, by step

From #359's kit. A step starts when the tests named "before" have run on polarzero's Mac, and is
done when the tests named "done" pass with Silo's own code.

| Step | Before | Done |
|---|---|---|
| 1 | 06 (the cost of `F_FULLFSYNC`), 20 (the engine's socket) | the engine killed 1,000 times with a request in flight (`1-engine.md`) |
| 2 | 00, 01, 10, 14 (first run and provisioning), 07 (gvproxy), 09 (launchd), 04 (the disk's mode) | 02, 08, 11, 12, 13, the hostile test, #347's checks 6 to 10 |
| 3 | 02 inside tmux | 16 (an idle night), the night's own tests (`3-a-night.md`) |

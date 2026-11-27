# Step 1: the engine, with nothing to run yet

Part of the build specs in this folder ([README](README.md)). It builds `access-model-checked`,
`engine-program-and-clients` and `silo-command` (#348). The formats are in
[formats.md](formats.md).

## What you can do when it is done

```
$ silo install
Silo will add two jobs to launchd: its engine and its recorder. They start when you log in.
Go on? [y/N] y
The engine is running. History 5b1e…, event 3.

$ silo status
Engine    running since 14:02, history 5b1e…, event 3, saved 14:02
Recorder  running, holds every event up to 3
Computers none. `silo setup` makes main (step 2)

$ kill -9 $(pgrep silo-engine); sleep 11; silo status --json
{"v":1,"engine":{"state":"running","history":"5b1e…","event":3,"since":"2026-11-25T14:02:31Z"},"recorder":{"state":"running","acked":3},"computers":[]}
```

The engine survives `kill -9` in the middle of a call, on a Mac, with the same state and the same
event numbers. That, and the access model checked on every short history, is the whole step.

## The processes

| Process | Language | Started by | Holds | When it stops |
|---|---|---|---|---|
| `silo-engine` | Zig | launchd, job `com.amontlabs.silo.engine`, `RunAtLoad` and `KeepAlive` | The core, its saved state, the drivers | launchd starts it again after 10 s; it replays its log |
| `silo-recorder` | Zig | launchd, job `com.amontlabs.silo.recorder`, the same | The record's files | launchd starts it again; it follows the engine from its last acknowledged event |
| `silo` | Zig | you, or a script | Nothing | Nothing is lost: a call it made is in the engine's log or was never accepted |

Both jobs are user agents in your login session, not system daemons: Apple's framework is not safe
in a daemon (#352 file 10), and step 2's helpers must be started from the same session. #359's
test 09 checks whether a user agent can start a macOS computer. Each job sets
`ProcessType Interactive` so App Nap never slows it, and `AbandonProcessGroup true`, so that launchd
stopping the engine never takes a helper with it (step 3).

`silo install` writes both jobs to `~/Library/LaunchAgents` and loads them with
`launchctl bootstrap gui/<uid>`, after your yes. On Linux it writes two systemd user units with
`Restart=always` instead. The app, later, registers the same jobs with `SMAppService`.

## The core

The core is a function. It takes the state and one validated input, changes the state, and puts
out events, effects and at most one answer. It reads no clock, no file and no socket, starts no
thread and allocates nothing after start (`deterministic-core`, `engine-limits`,
`tigerstyle-by-place`).

```zig
// engine/src/core/core.zig
pub fn apply(state: *State, input: *const Input, out: *Outputs) void;

pub const Outputs = struct {
    events: Bounded(Event, limits.events_per_input),
    effects: Bounded(Effect, limits.effects_per_input),
    answer: ?Answer,
    next_deadline_awake_ns: ?u64, // the core asks for the clock only then
};
```

`Input` is built in one file only, `core/input.zig`, by `validate`, which caps every field and
returns a named error for anything out of range (`engine-inputs`). The source check refuses any
other file that builds an `Input`, by a literal, `undefined`, `std.mem.zeroes` or a cast
(`engine-source-checks`).

### The tables

Flat tables of extern rows, each of fixed size, that refer to each other by typed ids. An id comes
from a counter that only grows and is never a position in a table, so a removed row's id never
names another row (#352 file 01, `engine-state`).

| Table | Rows (first build) | One row holds |
|---|---|---|
| `sessions` | 64 | session id, the number and the answer of its last request, the event number of that answer (`call-ids`) |
| `people` | 8 | person id, name, how the person is recognised (`local-user` and the uid) |
| `computers` | 8 | step 2 |
| `grants` | 256 | computer, resource (channel, secret or host, by id), who granted it, the event that did |
| `operations` | 64 | step 2 |
| `effects` | 256 | effect id, key, driver, kind, the awake time it times out, the room it keeps (`how-effects-work`) |
| `events` | 16,384 | the ring of recent events. A row the recorder has not acknowledged is never overwritten (`events-until-the-recorder`) |
| `counters` | 1 | the next id of each kind, the history, the last input and event numbers |

These numbers live in `core/limits.zig`, the one structure the core is built from. A `comptime`
check refuses limits that cannot work together, such as fewer effect rows than the effects one
input can put out. With these rows `State` is about 1.5 MiB.

When a table is full the input that needed a row is refused with `table_full`, and the refusal is a
control fact like any other. The events ring is the one table that fills without a call: when the
recorder is down, events wait in it. A call that would leave less room than the effects under way
may still need is refused with `record_behind`, and `silo status` says the recorder is behind. At
one event per call and a few per operation, 16,384 rows hold several days of the first build.

### The inputs

| Source | Its place in the queue | Inputs |
|---|---|---|
| `device` | 64 | a request of a session on your Mac |
| `computer` | 16 per computer | none in the first build; the place exists so that a call from inside a computer later cannot fill your place (#352 file 01) |
| `driver` | one per effect under way, kept when the effect is issued | a result, an observation |
| `clock` | 1 | the time, when the core asked for it |
| `recorder` | 1 | `ack` up to an event number |

The loop takes the places in turn. A request whose place is full is refused with `source_full`
before it reaches the core.

### The access model

`access-model-checked` is the first code of the core (`model-of-access-first`). It is
`core/access.zig`, a few hundred lines, and holds only:

- `grant(computer, resource, author)` and `take_back(computer, resource, author)`;
- the rule of who may do it: you, through a request from your Mac, or a rule you set
  (`rule` authors exist in the type and nothing produces them before rules exist);
- a request that arrives from a computer may take back what that computer holds and may grant
  nothing (`what-an-agent-inside-may-do`); it is refused with `not_allowed` and the refusal is a
  control fact.

A resource is `channel` (`way_in`, `network`, `import`), `secret` or `host`, each with an id. The
first build grants only channels: step 2 grants main `way_in` and, with your yes, `network`. Secrets
and hosts are in the type so that the exhaustive test covers them from the first day, as the
feature asks. #348 challenges this order (`model-of-access-first`); see the README.

### Effects and events

An effect asks a driver to act and has no return value. Its result, or its timeout, comes back as
an input. An effect is safe to issue twice, or it carries a key that names what it makes, such as
`c1/boot/5`, so that asking again after a timeout is the same effect. A timeout leaves the effect's
outcome unknown until the engine has looked, never failed. Timeouts count awake time only
(`how-effects-work`, as #352 file 03 proposes). Step 1 issues no effect; its drivers arrive in
step 2.

Every change of state puts out an event: a numbered, typed record of what changed, under the
current history. Clients follow events (`numbered-events`), and the recorder turns each into a
line.

## The loop around the core

`engine/src/loop/` feeds the core. It is ordinary code, but it reaches the disk, the clock, the
sockets and the drivers only through interfaces it is given at start (`core-dependencies`, as
#352 file 01 proposes), so the simulator runs this same loop over fakes.

Each turn of the loop:

1. Wait for a socket, a driver result, or the deadline the core asked for.
2. Read every complete message. Validate each into an `Input` or refuse it. Put it in its
   source's place.
3. Take every waiting input, in turn across places. Apply each to the core.
4. Write their log slots in one write and make one flush (`F_FULLFSYNC` on a Mac). A write or a
   flush that fails stops the engine with `write_failed`.
5. Only now send the answers, the events to followers, and the effects to the drivers. Copy the
   state for reads.
6. Save the state when the log is full enough ([formats.md](formats.md), "The log").

If an assertion fails, the loop writes its ring of the last 256 inputs and effects to
`engine/crash/` and exits; launchd starts it again (`assertions-stay-on`). An input that stopped
the engine twice is set aside at the next start and refused with `stopped_the_engine`
(`inputs-never-stop-the-engine`).

### Start

1. Take `engine/lock` with `flock`. If it is held, another engine runs: exit with
   `engine_already_running`.
2. Check `<data>`'s layout version, its owners and modes, and every socket path's length.
3. Load the newer good state place, replay the log ([formats.md](formats.md)).
4. Listen on `run/engine.sock`. Accept the recorder first and ask for its last event number; stop
   with `record_ahead_of_engine` if it is past the engine's.
5. From step 2: connect to every helper and reconcile (`engine-matches-wanted`).
6. Serve.

## The recorder

`silo-recorder` is its own process (`recorder-process`) so that the record does not depend on the
engine staying up, and so that a fault in the record's code does not stop the engine.

- It connects to the engine as a client with the role `recorder`, follows the events after the
  last one it holds, writes each as a line, flushes, and acknowledges.
- From step 3 it also serves `run/recorder.sock` for other sources.
- It writes a segment per computer and one more, `record/engine/`, for what concerns no computer,
  such as the people table and the engine's own restarts.
- It writes in its own hash chain per computer, its head every hour, and its index.

The recorder in step 1 is a build of `record-of-a-computer`'s format ahead of its feature, and the
README says what that needs from polarzero.

## The `silo` command

| Command | Step | Prints |
|---|---|---|
| `silo` | 1 | What needs you: in step 1, whether the engine and the recorder run and are installed |
| `silo install` | 1 | Installs and starts the two jobs, after your yes |
| `silo status [--json]` | 1 | The engine, the recorder, and from step 2 each computer, its network and its channels |
| `silo record <computer> [--json] [--since T] [--head]` | 1 | The record as stored |
| `silo setup`, `start`, `stop`, `pause`, `resume`, `proxy` | 2 | |

- With `--json`, one line per item, each with `"v":1`, the version of the command's output
  (`formats-carry-versions`). Text output is for people and may change.
- Exit codes: 0 done, 1 refused (the named error is printed), 2 wrong use, 3 the engine could not
  be reached.
- A command that waits for an operation follows events. When the engine restarts under it, it
  reconnects, sees the history, and repeats its request with the same session and number, which
  gives the first answer (`call-ids`).

## The Zig layout

One folder, `engine/`, beside `website/` and `tools/`. Names follow `one-name-per-concept`: a
file is named by the word of the plan it holds.

```
engine/
  build.zig  build.zig.zon         Zig 0.15.2, pinned; the build of #365's prototype
  src/
    core/                          the core: no I/O, no allocation, the strict checks
      core.zig                     apply()
      limits.zig                   every limit, one struct, checked at comptime
      input.zig                    Input and validate(): the only file that builds an Input
      state.zig                    State and its tables
      ids.zig                      a distinct type per kind of id
      checked.zig                  the one helper allowed to narrow and to wrap
      access.zig                   the access model
      sessions.zig                 call-ids
      events.zig                   numbered events and the ring
      effects.zig                  effects under way
      computers.zig  operations.zig  images.zig    step 2
    loop/                          the loop around the core
      loop.zig  places.zig  log.zig  state_file.zig  crash_ring.zig
    io/                            the interfaces the loop is given: disk, clock, sockets
      disk.zig  clock.zig  net.zig    with a real and a fake of each
    drivers/                       step 2: helper.zig, catalog.zig, image.zig, setup.zig, ssh_entries.zig
    protocol/                      engine.zig (the engine's protocol), helper.zig, sources.zig
    client/                        client.zig, and c.zig, the C interface (engine-client)
    recorder/                      recorder.zig, segment.zig, chain.zig, index.zig, render.zig
    cli/                           silo.zig: the command
    main_engine.zig  main_recorder.zig
  sim/                             the simulator: the core and the loop over fakes
  test/                            access_exhaustive.zig, log_replay.zig, recorder_chain.zig
  check/                           the source check, and planted/: a file per refused form
  macos/                           step 2: the helper, in Swift
```

The source check (`engine-source-checks`, as #352 file 01 words it) is a Zig program in
`engine/check/`. It reads `src/core/` and refuses: a bare `unreachable`; `@setRuntimeSafety`;
`@truncate`, `@ptrCast`, `@bitCast` and a narrowing `@intCast` outside `checked.zig`; `+%`, `-%` and
`*%` outside `checked.zig`; an `@import` outside the list of `core-dependencies`; a print or a log;
`TODO`, `FIXME`, or `@panic` with no named reason; and building an `Input` outside `input.zig`. In
all of `engine/` it refuses what `zig fmt --check` refuses. Each refused form has a file in
`check/planted/`, and the check's own test fails unless the check refuses every one of them.

`tools/ci/check.sh` gains a part `engine`: the source check, `zig build test`, the exhaustive
access test, and the simulator on a fixed list of seeds. It runs on Linux, as CI does. What needs
a Mac is in the test plan below.

## What happens on a crash

| What stops | When | What you see | What the engine does |
|---|---|---|---|
| The engine | between two inputs | `silo` reconnects | launchd starts it; it replays, with the same history and numbers |
| The engine | after an input's slot is written, before its flush | the request gets `outcome_unknown` or nothing | The slot is either on the disk or dropped as the last write. Repeating the request gives its real answer |
| The engine | a flush fails | the same | It stops with `write_failed` and never applies a request it refused (#352 file 02) |
| The engine | its state places are both damaged | `silo status`: `state_damaged`, and the path of both files | It does not start. What you can do then is `when-the-engine-stops`' question |
| The recorder | any time | `silo status`: the recorder is behind | Events wait in the ring; launchd starts the recorder, which follows from its last acknowledged event and writes every line once |
| The recorder | the ring is full | calls refused with `record_behind` | Nothing is decided that cannot be recorded (`enforcing-and-recording`) |
| The Mac | power cut | | Only flushed inputs exist. The recorder's check stops the start if the record holds more than the engine |

## Cost targets

Written before the code, as `own-cost-targets` asks, on the Mac polarzero names as the reference
(#352 asks for one). The flush's own cost is #359's test 06.

| Measure | Target |
|---|---|
| Disk writes of an idle engine | none in an hour |
| Memory of the engine at idle | under 32 MB |
| A request, from `silo` to its answer, p99 | one forced flush plus 2 ms |
| Start, with a full log to replay | under 300 ms |
| Memory of the recorder at idle | under 32 MB |

## Tests

| Test | Checks | Where |
|---|---|---|
| Source check, with every planted file refused | `engine-source-checks` | CI |
| `access_exhaustive.zig`: every sequence of up to 5 requests among 2 computers, 2 channels, 1 secret and 1 host, from your Mac and from a computer | Nothing is granted except by you or a rule; a computer never grants (`access-model-checked`) | CI |
| Simulator: millions of random inputs, with the engine killed at every point, torn and failed writes, a slow and a dead recorder, sessions evicted | `repeated-call-does-nothing-twice`, `no-effect-twice`, `effect-completed-once`, `app-shows-only-real-states`, `refused-input-changes-nothing`, `nothing-grows-without-limit`, `accepted-call-never-forgotten` (all proposed, none in force yet) | CI on fixed seeds; random seeds nightly |
| The simulator with a planted bug per property (an answer sent before its flush, a reused id, an unchecked repeat) | Each property's check catches its fault (`check-must-catch-a-fault`) | CI |
| `log_replay.zig`: every log written by the simulator is replayed from its file into the same state | The file layout | CI |
| The same on LazyFS, which drops what was not flushed | That only flushed inputs count | Linux, by hand |
| `recorder_chain.zig`: a record edited, cut or reordered by one byte is found by the chain | `record-edits-show` (#362) | CI |
| `engine-kill-mac.sh`: 1,000 rounds of a request in flight and `kill -9`, with `F_FULLFSYNC` | That the engine on a Mac keeps what it answered | polarzero's Mac, once per change to the loop |
| #359 test 06 | The cost of `F_FULLFSYNC`, grouped and not, which sets the request target | polarzero's Mac, before step 1 |
| #359 test 20 | Who connected to the engine's socket, and what `exec` changes | polarzero's Mac |

The boundary's checks of #347 (`research/2026-11-24-boundary-checks.md` in the project's files) do
not apply to step 1: nothing in it reaches a network.

## Done when

- every test above passes, and the Mac ones passed on polarzero's Mac;
- `silo status` and `silo status --json` print what the engine has, after any number of `kill -9`;
- `silo record engine --json` checks with `shasum` as [formats.md](formats.md) shows;
- the first files of `core/` were read line by line by the people `how-the-engine-is-built` names.

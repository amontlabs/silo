# The first build's data formats

Part of the build specs in this folder ([README](README.md)). Every file Silo writes to read later,
and every message between its processes, for steps 1 to 3 of the first build. Each format is said
here once; the step specs point to it.

All integers are little endian. Silo runs on arm64 and x86-64 only, which are both little endian,
and a reader refuses a file whose magic it does not recognise. Every format starts with its
version, as `formats-carry-versions` asks, and a reader names the versions it reads and refuses the
others with `version_unsupported`.

## Where Silo keeps its files

One folder, called `<data>` below: `~/Library/Application Support/Silo` on a Mac, and
`$XDG_DATA_HOME/silo` on Linux (`engine-on-linux`). Every folder in it is mode 0700 and every file
0600. The engine refuses to start when `<data>` or any folder in it belongs to another user or is
open to others.

```
<data>/
  LAYOUT                       one line: "silo-data 1". The layout's version
  reserve                      the space the engine keeps for itself (space-the-engine-keeps)
  engine/
    lock                       flock() held by the one running engine
    state.a  state.b           the two places of the saved state
    log                        the inputs since the last save, a ring of fixed size
    crash/                     the ring of recent inputs and effects, printed at a stop
  record/
    engine/                    what concerns no computer: people, the engine's restarts
    c1/                        the record of computer c1
      000001.silorec           segments, append only
      000001.idx               the index of a segment, rebuilt when missing
      head                     the hash of the last line, rewritten every hour
  images/
    macos-27.2.1-26C5032/      one per restore image build
      restore.ipsw.part        a download under way, resumed from its length
      installed/               the installed system, never started (computer-image)
        disk.img aux.img hardware-model.bin config.json
  computers/
    c1/
      live/                    the computer as it runs
        disk.img aux.img hardware-model.bin machine-id.bin config.json
      live.new/                a start being prepared; removed at the engine's start
      ckpt/                    checkpoints, from step 4 (#365's catalog)
      ssh/
        id_ed25519  id_ed25519.pub  known_hosts
      helper.plist             the launchd job of this computer's helper
  ssh/
    config                     the file of SSH entries (reaching-a-computer)
  run/
    engine.sock  recorder.sock
    helpers/c1.sock
```

- A computer's folder is named by its id, never by its name. Main is `c1`, and its name `main` is
  a field of its row. A name can change; an id never does (`engine-state`).
- A Unix socket path must be under 104 bytes on macOS. The engine checks every path it will bind
  when it starts, and stops with `socket_path_too_long` naming the path. With a user name of 20
  letters the longest, `run/helpers/<id>.sock`, is 76 bytes.
- `live.new/` is what a start or a restore prepares. #365's catalog swaps it with `live/` in one
  call (`renamex_np` with `RENAME_SWAP` on a Mac, `renameat2` with `RENAME_EXCHANGE` on Linux), so
  a disk never comes back with the auxiliary storage of another moment. Step 2 uses it once: to
  make main from a clone of the installed system. Steps 4 and 5 use the rest of #365's layout
  (`ckpt/<id>/`, `.tmp-<id>/`, `.trash-<id>/`, `next-id`) unchanged.
- Nothing else is written outside `<data>`, except the one `Include` line in `~/.ssh/config`
  (with your yes, `reaching-a-computer`) and the launchd jobs in `~/Library/LaunchAgents`.

## The engine's saved state

`engine-saved-state` and #352 file 02 give the design: a write-ahead log, two places for the
state, a forced flush, and a stop on any failed write. This is its layout.

### A place of the state: `state.a`, `state.b`

Both files are made at their full size when the engine is set up. A save writes the place that
holds the older save, flushes it, and from then on the log follows it.

| Offset | Field | Type | Meaning |
|---|---|---|---|
| 0 | `magic` | `[8]u8` | `SILOSTAT` |
| 8 | `format` | `u16` | 1 |
| 10 | `reserved` | `[6]u8` | zero |
| 16 | `checksum` | `[32]u8` | SHA-256 of the header with this field zeroed, then the body |
| 48 | `core_build` | `[32]u8` | The hash of the core's source the build was made from |
| 80 | `history` | `u128` | The history this state belongs to (`numbered-events`) |
| 96 | `save` | `u64` | The number of this save, one more than the save before |
| 104 | `last_input` | `u64` | The number of the last input this state includes |
| 112 | `last_event` | `u64` | The number of the last event it put out |
| 120 | `body_size` | `u64` | The size of the body |
| 128 | body | | The core's `State`, as its bytes |

The body is the core's `State` struct itself: fixed arrays of extern rows with no pointer, so its
bytes are the save. A test asserts `std.meta.hasUniqueRepresentation(State)`, so no padding byte
carries memory, and a second test checks that two equal states give equal bytes.

At start the engine reads both places and takes the one with the higher `save` whose checksum
holds. Two places with a checksum that fails, or none, stop the start with `state_damaged`. The
engine never builds a state by itself.

### The log: `log`

A ring of 4,096 slots of 4 KiB each, 16 MiB, made at full size at setup. One input fills one slot;
an input is never larger than one slot (validation caps every field so that it fits).

| Offset | Field | Type | Meaning |
|---|---|---|---|
| 0 | `checksum` | `[32]u8` | SHA-256 of the slot with this field zeroed, up to `size` |
| 32 | `format` | `u16` | 1 |
| 34 | `source` | `u8` | `device`, `computer`, `driver` or `clock` (`engine-inputs`) |
| 35 | `kind` | `u8` | The kind of input |
| 36 | `size` | `u32` | The bytes used in the slot |
| 40 | `input` | `u64` | The input's number, one more than the one before |
| 48 | `save` | `u64` | The save this input follows |
| 56 | `history` | `u128` | |
| 72 | `wall_ns` | `i64` | The time of day, in nanoseconds since 1970 |
| 80 | `awake_ns` | `u64` | The time the device has been awake, in nanoseconds |
| 88 | body | | The input, as its extern struct |

- **Writing.** The loop takes every input waiting at that moment, applies them to the core, writes
  their slots with one `pwrite`, and makes one flush: `fcntl(F_FULLFSYNC)` on a Mac, `fdatasync`
  on Linux. Only then do the events, effects and answers those inputs produced leave the engine.
  It never waits for more inputs to group them.
- **Saving.** When the slots after the last save reach 4,000, the engine saves the state to the
  older place, flushes it, and the next input goes into slot 0 under the new save number.
- **Replaying.** At start the engine applies, in order, every slot whose `save` is the loaded
  save's and whose `input` is the next number. It stops at the first slot that is not.
- **Damage.** A slot whose checksum fails, followed by no valid slot of the same save with a
  higher number, was the last write before the stop and is dropped: nothing it produced left the
  engine. A slot whose checksum fails with a valid later slot behind it stops the start with
  `log_damaged`.
- **The recorder's check.** Before it serves anything, the engine asks the recorder for the last
  event number it holds for this history. A number past the engine's `last_event` means an input
  that had been flushed was lost, and the start stops with `record_ahead_of_engine`.
- **A failed write or flush** stops the engine with `write_failed`, whatever the input was: after
  a failed flush nobody knows what reached the disk (#352 file 02, hole 2). launchd starts it
  again, and the replay says what was kept.
- **Cost.** The log is 16 MiB and the state places are each the size of `State` (the limits below
  give about 1.5 MiB). With no input there is no write: the clock enters only when the core asks
  for it (`deterministic-core`, as #352 proposes), so an idle engine makes no flush.

### The core build and updates

`core_build` is in every save and every slot. An engine replays only slots whose `core_build` is
its own. Before an update replaces the engine, the running engine saves with no input after it
(`formats-carry-versions`, as #352 file 04 proposes). A new engine that finds slots of another
build stops with `log_of_other_build`. The first build has no updater, so this is one check at
start, and the drain is written down for the first update.

## The engine's protocol

Between the engine and every client: the `silo` command, the app later, the recorder, and in a
later build a computer's helper passing on a call from inside. One Unix stream socket,
`run/engine.sock`. The engine reads the caller's user with `getpeereid` and closes the connection
of any other user before reading a byte (`who-can-call-the-engine`).

### Framing

Every message is a 64-byte header and a body.

| Offset | Field | Type | Meaning |
|---|---|---|---|
| 0 | `magic` | `[4]u8` | `SILO` |
| 4 | `version` | `u16` | The protocol version agreed in the handshake |
| 6 | `kind` | `u16` | The kind of message |
| 8 | `body_size` | `u32` | At most 64 KiB |
| 12 | `body_crc` | `u32` | CRC-32C of the body |
| 16 | `session` | `u64` | 0 before `registered` |
| 24 | `number` | `u64` | The request's number in its session; 0 for a message that is not a request |
| 32 | `reserved` | `[32]u8` | zero |

A message with a wrong magic, a size over the limit, a CRC that fails or a kind the version does
not have closes the connection. Nothing in it reaches the core.

### The messages

| Kind | From | Body | Meaning |
|---|---|---|---|
| `hello` | client | the versions it speaks, its role (`command`, `app`, `recorder`) | First message |
| `welcome` | engine | the version chosen, the history id, the last event number | Or `error` `version_unsupported` |
| `register` | client | nothing | Opens a session (`call-ids`). A new process always opens a new one |
| `registered` | engine | the session id | |
| `request` | client | one operation, below | Carries `session` and `number` |
| `reply` | engine | `accepted` with the operation id, or `refused` with a named error | Sent once the input is flushed. A repeat of the same number gets the same reply |
| `read` | client | what to read | Not an input: answered from the state as of the last flushed input |
| `state` | engine | the state at event number N, in parts of 64 KiB | |
| `follow` | client | history H, event number N | Asks for every event after N |
| `events` | engine | events in order | Or `resync`, when N left the ring or H is not the engine's history |
| `ack` | recorder | history H, event number N | The recorder holds every event up to N on its disk |
| `error` | engine | a named error | Then the connection closes |

A read never shows a state the engine could still lose: the loop answers it from the copy of the
state made after the last flush, never from the core's state before its inputs are on the disk.

### Operations of steps 1 to 3

| Operation | What it asks | Named refusals |
|---|---|---|
| `setup` | Make main: the network open or closed, the list of what to install | `not_this_mac`, `disk_too_small`, `already_set_up` |
| `start` | Want a computer running | `unknown_computer`, `operation_under_way` |
| `stop` | Want it stopped | same |
| `pause`, `resume` | Want it paused, or running again | same |
| `pause_all`, `resume_all` | `pause-every-computer` | |
| `grant`, `take_back` | A channel of a computer (`access`, step 1) | `not_allowed` |
| `cancel` | Ends an operation under way, at its next step | `nothing_to_cancel` |

Every refusal also exists for every operation: `session_evicted`, `number_too_low`,
`content_differs`, `source_full` (the place of this source is full, `engine-inputs`), `table_full`,
`log_full`, `outcome_unknown` (sent only when the engine stopped before it could answer; repeating
the request after the restart gives the real answer).

## The helper's protocol

Between the engine and the helper of one computer (`processes`, `engine-takes-back-helpers`). The
helper is Swift (`macos-vm-driver`), so this link uses JSON, which Swift reads with `Codable`, and
the engine's side turns each message into a validated input before the core sees it.

- The helper listens on `run/helpers/<id>.sock`. The engine connects; the helper accepts only its
  own user (`getpeereid`).
- Each message is a 4-byte length, at most 64 KiB, then one JSON object.
- **Handshake.** The engine sends `{"hello":{"versions":[1],"nonce":"<32 bytes, hex>"}}`. The helper
  answers with its version and HMAC-SHA-256 of the nonce under the computer's key, a 32-byte file
  `computers/<id>/helper.key` the engine made with the computer. The engine then does the same the
  other way. A helper that fails it is not this computer's helper, and the engine says so and
  leaves it alone.

| Message | Direction | Fields |
|---|---|---|
| `start` | engine to helper | `effect`, `key`, `provision` (first start only: user, full name, password, `autologin`, `remote_login`), `network` |
| `stop` | engine to helper | `effect`, `key`, `grace_s` (30, `what-stop-ends`) |
| `pause`, `resume` | engine to helper | `effect`, `key` |
| `observe` | engine to helper | asks for `state` now |
| `state` | helper to engine | `value` (`stopped`, `starting`, `running`, `pausing`, `paused`, `resuming`, `stopping`, `error`), `since_awake_ns`, `boot` (a counter of this computer's starts) |
| `result` | helper to engine | `effect`, `ok`, or `error` with Apple's `domain`, `code` and the `VZError.Code` name |
| `place_refused` | helper to engine | Apple refused to start a macOS computer for lack of a place (`neutral-core`) |

These are the events #359's `silovm run` prints, so the kit's helper is the first draft of this
one.

The same socket answers one more message, from `silo proxy` (the `ProxyCommand` of the SSH
entries): `{"connect":{"port":22}}`. The helper opens a connection to the computer's SSH server and
sends the connected socket back with `SCM_RIGHTS`; `silo proxy` hands it to ssh
(`ProxyUseFdpass`). No port of the Mac listens for SSH, and the helper sees each connection and
the program that asked for it (#352 file 06).

## The record

`record-file-format` (taken by agents) sets append-only files, compressed, with an index. #362's
`record-edits-show` and `person-in-a-control-fact` ask for a hash chain and a person in each
control fact from the first version, because both change the format. Steps 1 to 3 write only
control facts, gaps and silences; the format holds every kind of `kinds-of-lines` from the start.

### A segment: `record/<id>/NNNNNN.silorec`

A 128-byte header, then frames. A segment is closed at 64 MiB or at local midnight, whichever
comes first, and never written again.

| Offset | Field | Type | Meaning |
|---|---|---|---|
| 0 | `magic` | `[8]u8` | `SILOREC` and a zero byte |
| 8 | `format` | `u16` | 1 |
| 10 | `compression` | `u8` | 0, none. `measuring-the-store` chooses a compression; a closed segment can then be written again compressed, as a new file with its own value here, since the hashes are over the uncompressed lines |
| 11 | `reserved` | `[5]u8` | zero |
| 16 | `computer` | `u64` | The computer's id; 0 for `record/engine/` |
| 24 | `segment` | `u32` | One more than the segment before |
| 28 | `reserved` | `[4]u8` | |
| 32 | `first_seq` | `u64` | The `seq` of its first line |
| 40 | `prev` | `[32]u8` | The hash of the last line of the segment before, zeros for the first |
| 72 | `reserved` | `[24]u8` | |
| 96 | `checksum` | `[32]u8` | SHA-256 of the first 96 bytes |

A frame is a `u32` length, a `u32` CRC-32C of the payload, and the payload: one line of the record,
UTF-8 JSON on one line, with no newline. A frame whose CRC fails at the end of the last segment was
being written when the recorder stopped. The recorder cuts it off at start. Nothing is lost: the
recorder had not acknowledged that line, so its source sends it again.

### A line

One line of the record is one line of `silo record --json`, byte for byte (`word-line`). The
recorder writes its keys in this order and nothing else, so the bytes it stored are the bytes it
prints.

```json
{"v":1,"seq":1042,"prev":"9f2c…e1","time":"2026-11-25T02:41:07.123456789Z","computer":"c1","source":{"id":"engine","count":"h:5b1e…","n":311},"is":"fact","kind":"control","what":"state","from":"running","to":"stopped","author":{"by":"silo"},"cause":{"what":"helper-ended","boot":4}}
```

| Key | Meaning |
|---|---|
| `v` | The line format, 1 |
| `seq` | The line's number in this computer's record, one more than the one before, given by the recorder |
| `prev` | The hash of the line before, in hex: SHA-256 of its exact bytes. The first line of a computer's record has 64 zeros |
| `time` | When the source saw it, UTC, in nanoseconds |
| `computer` | |
| `source` | Where it was seen (`sources`): the source, the id of its count and the line's number in that count (`sources-number-their-lines`) |
| `is` | `fact` or `report`. A line is one of the two, never both (`line-never-mixes`) |
| `kind` | One of `kinds-of-lines`: `control`, `process`, `file`, `connection`, `model-call`, `screen`, `meter`, `gap`, `silence` |
| `author` | Control facts only: `by` is `you`, `rule`, `precaution`, `extension` or `silo` (`who-made-each-change`), and `person` the id of the person when `by` is `you` |
| the rest | The kind's own keys |

Anyone can check the chain with ordinary tools. `silo record c1 --json` prints the lines as stored,
so the SHA-256 of one printed line, without its newline, is the `prev` of the next:

```sh
silo record main --json | head -2 | { read -r a; read -r b; \
  [ "$(printf %s "$a" | shasum -a 256 | cut -d' ' -f1)" = "$(printf %s "$b" | jq -r .prev)" ] && echo ok; }
```

### How a source sends its lines

A source is the engine, or a helper (from step 3), and later the probe and the boundary. The engine
is a source through its own protocol: the recorder connects to `run/engine.sock` as a client,
follows the events, renders each as a line, and sends `ack` for the last event whose line is on its
disk (`events-until-the-recorder`). The engine's count is its history, so `source.count` is `h:`
and the history id, and `source.n` the event number.

Every other source connects to `run/recorder.sock`, with the framing of the helper's protocol (a
4-byte length, then one JSON object):

| Message | Direction | Fields |
|---|---|---|
| `hello` | source | `versions`, `source` (`helper:c1`), the last count id and number it sent, if any |
| `count` | recorder | the id of the count to use from now (`sources-number-their-lines`, as #352 file 02 proposes) |
| `line` | source | `n`, `time`, `is`, `kind` and the kind's keys |
| `last` | source | `n`, sent every 60 seconds when there is nothing to say |
| `ack` | recorder | the highest `n` whose line is on the recorder's disk |

The recorder writes a gap when a number is missing, a restart when a line comes under a new count,
and a silence when a source sends nothing for 3 minutes. A source keeps every line it sent until
the recorder acknowledges it, in a ring of fixed size, and sends them again after a reconnect; a
line that arrives twice under one count is written once. A source whose ring is full loses the
oldest unacknowledged line and says so in its next line, which the recorder writes as a gap with
the count.

### The head

`record/<id>/head` holds `{"v":1,"computer":"c1","seq":…,"hash":"…","time":"…"}`. The recorder
rewrites it every hour and when it stops (a temporary file, a flush, a rename). `silo record main
--head` prints it. `record-edits-show` has it written "to a place the user chooses"; the first
build writes it here only, and copying it elsewhere is a later feature.

### The person

Each control fact made by you names a person, `p1` in the first build. The engine's `people` table
has one row per person: an id, a name, and how the person is recognised. On a Mac with one user,
the person is recognised by the user id of the socket (`getpeereid`), and the row says so
(`"by":"local-user","uid":501`). A host shared by several people adds rows recognised by a key of
their own (#362), and the format does not change. The row is made at setup, and its making is the
first control fact of main's record.

### The index: `NNNNNN.idx`

Rebuilt from its segment when missing or older. It holds the offset of every 1,024th line, by
`seq` and by `time`, so `silo record main --since 02:40` seeks instead of reading every segment. It
is not part of the record and is never trusted over the segment.

## What each step writes

| Format | Step 1 | Step 2 | Step 3 |
|---|---|---|---|
| `<data>` layout | `engine/`, `record/`, `run/` | `images/`, `computers/`, `ssh/`, `reserve` | |
| Saved state | yes | adds the computer, image and operation rows | |
| Engine protocol | `hello` to `ack`, `grant`, `take_back` | `setup`, `start`, `stop`, `pause`, `resume` | |
| Helper protocol | | yes | |
| Sources' protocol | the recorder follows the engine | | each helper is a source, for sleeps and wakes |
| Record | control facts of the access model, gaps, silences | computer state and setup steps | sleeps and wakes of the Mac |

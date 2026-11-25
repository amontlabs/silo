# The record, first half: second opinions for the first build

An agent's second opinion, 24 November 2026, on six decisions of the part `record` that the first
build needs. Nothing under `plan/` was changed. Every quoted answer is read from `plan show <id>`
at the tree of this date.

The first build's record holds very little: control facts from the engine (main was made,
started, paused, stopped; a checkpoint was taken; a restore happened) and file facts from comparing
two checkpoints of main's disk. There is no probe, no network source, no model traffic and no app.
Each section below asks whether the proposed words and kinds fit that case, and whether they would
have to be reshaped when the second build adds processes, connections and forks.

| id | Recommendation | Confidence | Reason in one line |
|---|---|---|---|
| `word-line` | Take as written | high | The word fits every line the first build writes, and one line of the record is one line of `silo` JSON output |
| `word-gap` | Take with an edit | medium | "Lost" does not cover the first build's likeliest hole: a pair of checkpoints that could not be compared (unreadable disk, comparison not finished before thinning) |
| `sources` | Take with an edit | medium | The `silo` command needs the exact source on every line, and a source must name its version so a fixed bug in the disk comparer can be told apart in old lines |
| `kinds-of-lines` | Take with an edit | medium | The file row cannot say what changed (in a trial, 92% of "changed" lines had a size change of 0), facts that other decisions already write have no kind, and five rows freeze detail that later features will redo |
| `who-made-each-change` | Take with an edit | high | The list of authors leaves out a computer, though an agent inside main may call the engine (`what-an-agent-inside-may-do`, taken) |
| `changes-to-the-record` | Take as written | high | Its two properties are exactly what the first build must keep; the removal clause is idle until the bin exists and blocks nothing |

---

## `word-line`: What is one fact or one report in the record called?

- **Recommendation:** Take as written. The word names what a line holds, it collides with nothing
  in `words.json`, and it matches the output format already decided.
- **Confidence:** high. It rests on reading `words.json`, `record-queried-with-ordinary-tools` and
  the names other systems use.
- **What it decides, on one real case:** In the morning you run `silo changes main --since cp-0412`.
  Each JSON line printed is one line of the record: one file deleted between two checkpoints. The
  checkpoint taken at 03:10 is another line, of the kind control.
- **Options:**
  - "Line": one fact or one report (current answer). `record-queried-with-ordinary-tools` prints
    "facts and reports as JSON, one line each", so a line of the record and a line of output are
    the same thing. JSON Lines tools (jq, `grep`, DuckDB's `read_json`) count the same unit.
  - "Event": what CloudTrail, event sourcing, macOS Endpoint Security and OpenTelemetry call it.
    It is already taken in Silo: `numbered-events` uses "event" for the engine's numbered changes,
    which are a different stream (`events-until-the-recorder`). Reusing it would put one word on
    two things.
  - "Record": what auditd calls one row (several records make one event). Silo uses "record" for
    the whole.
  - "Entry": what Claude Code's transcripts are usually called. The plan uses "entry" for its own
    files.
- **Evidence:**
  - auditd: an event is "all records that have the same host (node), timestamp, and serial
    number", so one auditd event is several rows joined by a serial
    ([linux-audit wiki](https://github.com/linux-audit/audit-documentation/wiki/SPEC-Writing-Good-Events)).
    Verified. This is the shape `line-never-mixes` already chose: the file fact and the report of
    which process wrote it are two lines that point to each other.
  - Claude Code's transcript in this container (`~/.claude/projects/-home-user/<session>.jsonl`)
    has one JSON object per line, with `uuid` and `parentUuid`. Verified by reading the file.
- **Risks and what would change this:** none found. A merged line under a flood
  (`record-under-a-flood`) holds many changes, but it is still one fact at a coarser grain, so the
  meaning holds.
- **For the first build:** does not block anything once taken. The builder needs nothing more.

## `word-gap`: What does "gap" mean?

- **Recommendation:** Take with an edit. The first build's most likely hole is not a line lost on
  the way, it is a stretch the disk source could not see. "Lost" does not clearly cover it, and
  `every-loss-is-a-gap` would then not apply to it.
- **Confidence:** medium. It rests on reading the first build's sources (one engine, one disk
  comparer) and on how the comparison can fail. Whether FileVault can be turned on inside a macOS
  virtual machine was not checked.
- **What it decides, on one real case:** Checkpoints of main are taken every few minutes. At 03:00
  the comparison of `cp-0411` and `cp-0412` fails: the disk reader refuses a structure it does not
  understand, or an agent with `sudo` encrypted the Data volume, or the Mac restarted while
  comparing and `cp-0412` was then thinned before it could be compared again. In the morning,
  `silo changes main --since cp-0410` prints nothing for that stretch. Without a gap, you read
  "nothing changed", which is the wrong answer the first-build proposal itself warns about ("must
  show as a gap, not as 'nothing changed'").
- **Options:**
  - Only what was lost (current answer). Clear for a stream that drops lines under load (40,000
    process reports, 3,000 lost). It does not clearly cover facts that were never produced because
    the source could not look. `sources-number-their-lines` only catches lines that were numbered
    and then lost; a comparison that never ran numbers nothing.
  - What should be in the record and is not, whether lost on the way or never seen (proposed). It
    keeps the exclusion the notes want: what Silo does not look at by design (reads of files, a
    file created and deleted between two checkpoints) is still not a gap, because it was never
    meant to be there.
  - Everything Silo did not see (rejected in the notes, and rightly: a gap everywhere means
    nothing).
- **Evidence:**
  - CloudTrail's digest files are delivered every hour even when no log file was delivered, so one
    can "assert positively that no log files were delivered to your account during a given period
    of time"
    ([AWS](https://docs.aws.amazon.com/awscloudtrail/latest/userguide/cloudtrail-log-file-validation-intro.html)).
    Verified. It is the same distinction: "nothing happened" has to be written, or it cannot be
    told from "nothing was looked at".
  - auditd counts `lost`: "how many event records that have been discarded due to the kernel audit
    queue overflowing" ([auditctl(8)](https://man7.org/linux/man-pages/man8/auditctl.8.html)).
    Verified. It counts only transport loss, like the current answer.
  - Endpoint Security gives each message a `seq_num` per event type and a `global_seq_num` per
    client, so a client can compute how many messages the kernel dropped
    ([Apple, `seq_num`](https://developer.apple.com/documentation/endpointsecurity/es_message_t/seq_num),
    [`global_seq_num`](https://developer.apple.com/documentation/endpointsecurity/es_message_t/global_seq_num)).
    Verified through Apple's documentation JSON. Again transport loss only.
  - The disk comparer is a batch source, unlike all three: given two checkpoints that still exist,
    it can run again and produce the same lines. A failed comparison is a gap only once one of its
    two checkpoints is gone or the disk cannot be read. Reasoned, not tested.
- **Risks and what would change this:** if `file-changes-from-outside` is edited to say that a
  comparison that fails is written as its own kind of fact, the word can stay as it is. Splitting
  it that way costs a second word for "missing" and a second place every view must check.
- **Proposed text:**
  - answer: "Facts or reports that should be in the record and are not, because they were lost or
    because a source could not see, recorded as missing."
  - sets: `{"in":"words","word":"gap","means":"Facts or reports that should be in the record and are not, because they were lost or because a source could not see, recorded as missing."}`
  - Keep the note: what Silo never records by design, such as reads of files, is not a gap.
- **For the first build:** blocks `record-of-a-computer` (through `kinds-of-lines`). The builder
  needs one rule from it: each pair of checkpoints ends with either its file facts and a closing
  count, or a gap. See "Cross-cutting findings".

## `sources`: Does each line of the record say where it came from?

- **Recommendation:** Take with an edit. The answer is right for the app, but the first build has
  no app, and the source must carry its version.
- **Confidence:** medium. It rests on how other systems name the writer of a line, and on one real
  failure (a bug in the comparer) the current text does not handle.
- **What it decides, on one real case:** Silo 0.3's disk comparer does not see changes of extended
  attributes. Silo 0.4 fixes it. A month later you compare two weeks of `silo changes` output and
  see more "changed" lines after the update. If each line says only `"source": "disk"`, you cannot
  tell that the jump comes from the fix and not from the agents.
- **Options:**
  - As written: each line says its source; the app shows fact or report at a glance and the exact
    source on opening. For the `silo` command it says nothing, and the builder will guess.
  - With an edit (proposed): the `silo` command prints the exact source on every line; each source
    names its version, written once per stretch of lines it wrote and not on every line.
  - Source on every line in the app too (the other option of the notes). Rightly rejected for the
    app: it is screen clutter. It costs nothing in JSON.
- **Evidence:**
  - OpenTelemetry's log record has an `InstrumentationScope` field beside `Resource`
    ([data model](https://opentelemetry.io/docs/specs/otel/logs/data-model/)), and the scope is a
    name and a version in the OpenTelemetry specification. The field is verified; that it holds a
    version was not read on the page fetched.
  - Every line of Claude Code's transcript carries `version`, the version of the program that wrote
    it. Verified in this container's transcript.
  - CloudTrail lines carry `eventSource` and `eventVersion`; `userAgent` names the caller's tool
    and version ([AWS example](https://docs.aws.amazon.com/awscloudtrail/latest/userguide/cloudtrail-event-reference-user-identity.html)).
    Verified.
  - `formats-carry-versions` already puts a version at the start of every record file. That is the
    version of the format, not of the program that saw the fact. A fix to the comparer changes no
    format. Reasoned.
- **Risks and what would change this:** if record files are cut per Silo version, the file's
  version could serve. `measuring-the-store` decides how files are cut, so that cannot be relied on
  yet.
- **Proposed text:** answer: "Each line says its source: where it was seen, or who told it. A source
  also names its version, once for each stretch of lines it wrote. The `silo` command prints the
  exact source of every line. The app tells a fact from a report at a glance, and names the exact
  source when you open the line." `sets` unchanged.
- **For the first build:** two sources only, `engine` and `boundary: disk`. The sources table in the
  notes fits them without change.

## `kinds-of-lines`: Which kinds of facts and reports does the record hold?

- **Recommendation:** Take with an edit. Keep the nine names and the rule that an extension adds
  none. Change three things: what a file line says, where the record's own facts go, and which rows
  are taken now.
- **Confidence:** medium. The file row's weakness is measured (below). The rest rests on reading
  the other decisions of the part.
- **What it decides, on one real case:** An agent runs `sudo chmod -R go-rwx /opt/homebrew` and
  deletes `~/work/app/data`. Homebrew stops working. You open `silo changes main`. Under the current
  row ("Created, changed or deleted: path and change in size"), the thousands of files of
  Homebrew show as "changed, 0 bytes": the line is true and says nothing of what broke.
- **Options:**
  - As written: nine kinds, the table in the notes in force, all of it.
  - Nine kinds, with three edits (proposed):
    1. **The file row says what changed.** "Created, changed or deleted between two checkpoints,
       which it names. Path, type (file, folder or link), size before and after, and for a change,
       which of content, mode, owner and link target changed." The two checkpoints belong in the
       line itself because after a restore the checkpoint before is not the one before in time
       (see "Cross-cutting findings"), and when one checkpoint is thinned before comparing, the
       pair says which stretch the line covers. `file-changes-from-outside` already says the fact
       names its two checkpoints; the row should say the same.
    2. **The record's own facts get a row.** Four decisions already write facts that fit no kind of
       the list: a source that started again with the last number received
       (`sources-number-their-lines`), a number that arrived twice with two different lines (same),
       bulky content removed to make room (`record-size-limit`, `conversation-text-kept`), and the
       record of a computer removed by you (`changes-to-the-record`). Add them to the row of gap
       and silence, or to control. Control is simpler, since each of these has an author (Silo, or
       you).
    3. **A call that was refused is a control fact too.** `restore-saves-first` says a restore that
       cannot save first does not happen "and Silo says why". The row of control lists only what
       was done. Add: "a call that was refused, with its named error and who made it".
  - Take only the names now and every row with its feature (narrower). `kind-before-design` wants
    the kinds named before features are designed, which the names do. The rows of process,
    connection, model call, screen and meter hold detail (the secret by name, refusals and
    reroutes, what a provider says is left of a limit) that their own features will weigh again.
    Taking them now does not block anything, but it puts detail in force that nobody has designed.
    I would take the names, the fact-or-report column, and the rows of control, file, gap and
    silence, and say that the other rows are proposals until their feature is taken.
  - A list that extensions add to (rejected in the notes). Claude Code's transcripts show the cost:
    this one file holds seven types of line (`assistant`, `user`, `attachment`, `queue-operation`,
    `tools-used`, `atis-latch`, `last-prompt`), several undocumented and three without a
    timestamp. Verified by reading this container's transcript.
- **Also say what "fixed" means.** "Fixed" should mean that no extension adds a kind. A later Silo
  will add one, as auditd and CloudTrail add types and categories over versions. Write it: "A kind
  is added only by a release of Silo, with a new version of the format. Whatever reads the output
  of `silo` skips a kind it does not know." Without it, the first jq script polarzero writes will
  break the day screen lines appear.
- **Evidence:**
  - Trial, run here (Linux container, Python, script kept outside the repository). A copy of
    `/usr/share`, 29,943 entries, was listed, changed, and listed again: `chmod -R go-rwx` on one
    folder, `rm -rf` on another, a folder renamed, 50 files appended to, one symbolic link made.
    The comparison gave 648 lines. Of the 616 "changed" lines, 567 had a size change of 0: 480
    files and 87 folders whose mode alone changed. With the fields of the current row, those 567
    lines say "changed, 0" and nothing more. With the proposed fields, each says `"changed":["mode"]`.
    Measured.
  - Size of a line, same trial: 158 bytes of JSON per line with the current fields, 210 with the
    proposed ones. Compressed with gzip, 8.9 and 14.2 bytes per line; with xz, 7.4 and 11.8. The
    added fields cost about 5 bytes per line on disk. The comparison itself took 0.04 s for 30,000
    entries already listed. Measured.
  - rsync's `--itemize-changes` reports, for each file, which of checksum, size, time, permissions,
    owner, group, ACL and extended attributes changed (`rsync(1)`). Git's diff writes
    `old mode 100644` / `new mode 100755` for a mode-only change. Both known from their manuals;
    not fetched here.
  - osquery's file events carry an action, such as `ATTRIBUTES_MODIFIED`, and hashes computed "if
    possible" ([osquery FIM](https://osquery.readthedocs.io/en/stable/deployment/file-integrity-monitoring/)).
    Verified for those two; the full list of actions was not on the page.
  - CloudTrail records failed calls with an `errorCode`, and auditd records `success=no`; both keep
    a refusal as a line. Known; the CloudTrail part not fetched here.
- **Risks and what would change this:** extended attributes are left out of the proposal on
  purpose. macOS sets attributes such as `com.apple.quarantine` and `com.apple.provenance` on its
  own, and listing them could make most lines noise. Whether to add `xattrs` to "what changed" is
  best measured on a real main, with the first comparison of two checkpoints.
- **Proposed text:**
  - answer: "Control, process, file, connection, model call, screen, meter, gap and silence. The
    list is short and fixed: an extension adds no kind, and a kind is added only by a release of
    Silo, with a new version of the format. Whatever reads the output of `silo` skips a kind it does
    not know. The rows of control, file, gap and silence in the notes are in force. The other rows
    are proposals until the feature that writes them is taken. A process line also gives the
    processor and memory that a run's processes use while they run, sampled, as a report."
  - In the notes, the file row: "Created, changed or deleted between two checkpoints, which it
    names. Path, type (file, folder or link), size before and after, and for a change, which of
    content, mode, owner and link target changed. A rename is a delete and a create
    (`file-changes-from-outside`)."
  - In the notes, the control row, added at its end: "A call that was refused, with its named error.
    A source that started again, with the last number received from it. A number that arrived twice
    with two different lines. Bulky content removed to make room. The record of a computer
    removed."
- **For the first build:** blocks `record-of-a-computer`. The builder needs the file row and the
  control row only. Process, connection, model call, screen and meter are not written by anything
  in the first build.

## `who-made-each-change`: Does the record say who made each change?

- **Recommendation:** Take with an edit. The list of authors leaves out the most likely caller
  after you: an agent inside main.
- **Confidence:** high on the missing author, which follows from two taken or answered entries.
  Medium on the wording.
- **What it decides, on one real case:** Claude Code in main, told to "clean up", runs
  `silo restore main cp-0398` from inside main. `what-an-agent-inside-may-do` (taken) gives agents
  inside "anything the user could do in the app", scoped to their computer, and
  `who-can-call-the-engine` says every call "carries where it comes from". The restore is a
  control fact. Its author is none of "you, a rule, a precaution, an extension or Silo". The
  builder will either write "you", which is false, or break the property.
- **Options:**
  - As written. Five authors. Wrong for a call from inside a computer.
  - Add "a computer" and say the author is taken from how the call arrived (proposed). This is
    what `who-can-call-the-engine` already requires of the engine, so it costs nothing new. It also
    keeps "you" honest. An agent on your Mac outside every computer that runs `silo restore` arrives
    as the command line on your Mac, and the engine cannot tell it from you
    (`mac-agent-not-the-main-path` says that path is not the main one, not that it cannot happen).
    The line should then say "the command line on your Mac", which the app may show as "you".
  - Also name what started an act of Silo. A checkpoint "by Silo" can come from the interval, from
    an ask, or from a restore that saves first (`restore-saves-first`). The first build has the
    first and the third. Saying which costs one field and lets you tell "Silo took this on its own"
    from "Silo took this because you restored". The-chain already wants lines tied to what caused
    them; this is that tie for control facts.
- **Evidence:**
  - CloudTrail's `userIdentity.type` takes `Root`, `IAMUser`, `AssumedRole`, `AWSService` and
    others, and `invokedBy` names the AWS service that made the call; the type `Unknown` exists for
    an identity CloudTrail cannot determine
    ([AWS](https://docs.aws.amazon.com/awscloudtrail/latest/userguide/cloudtrail-event-reference-user-identity.html)).
    Verified. The author is the kind of caller as AWS saw the call, not a person.
  - auditd keeps `auid`, "the account used to login with", beside the `uid` that `sudo` changes
    ([linux-audit wiki](https://github.com/linux-audit/audit-documentation/wiki/SPEC-Writing-Good-Events)).
    Verified. It is the same split: who is acting now, and on whose way in.
  - On files, the current answer is right and matches `line-never-mixes`: a file fact carries no
    author. "Unattributed" is computed from the absence of a report that points to the fact; it is
    not a field to write into the file fact. In the first build there is no probe, so every file
    change is unattributed.
- **Risks and what would change this:** if `computer-reads-its-record` or a later decision takes
  restore away from agents inside, the case narrows, but checkpoint and pause still reach the
  engine from inside, so "a computer" is still needed.
- **Proposed text:**
  - answer: "For a computer's rules, secrets, channels and state, always. Every control fact says
    who did it, as the engine saw the call arrive: you (the app or the command line on your Mac), a
    computer (a call from inside it), a rule, a precaution, an extension or Silo. When Silo did it,
    the fact says what started it, such as the interval or a restore. For files, when it is known.
    A file change with no known author is shown as \"unattributed\"."
  - sets: `{"in":"record/properties","id":"every-control-fact-has-its-author","says":"Every control fact says who did it, as the engine saw the call arrive: you, a computer, a rule, a precaution, an extension or Silo.","kind":"safety","check":["simulator"]}`
- **For the first build:** blocks `record-of-a-computer`. The builder needs three authors (you on
  your Mac, main, Silo with the interval or the restore) and the rule that the author comes from
  the way the call arrived. In the first build, `silo changes` should not print "unattributed" on
  every line: with no probe nothing can be attributed, and one sentence saying so is clearer.

## `changes-to-the-record`: Can anything change what the record already holds?

- **Recommendation:** Take as written. Its two properties are exactly what the first build has to
  keep, and the removal clause blocks nothing.
- **Confidence:** high. It rests on the first build's case and on how other append-only logs
  behave.
- **What it decides, on one real case:** At 08:00 you restore main to `cp-0398` from 02:00. The
  disk, and Claude Code's transcripts in `~/.claude/projects` inside main, go back to 02:00. The
  record keeps the file facts written from 02:00 to 08:00, the checkpoint that `restore-saves-first`
  took at 08:00, and the restore itself. A week later you can still read which files the six
  hours of undone work touched, even after thinning has removed the 08:00 checkpoint.
- **Options:**
  - As written: nothing inside a computer changes a line; you alone remove the whole record of a
    computer that has left the bin. In the first build there is one computer, main, which never
    goes to the bin, so the removal is never offered and nothing has to be built for it.
  - Nobody, ever (rejected in the notes, for a disk that only grows).
  - Removal of one line by you. Event sourcing systems face this with personal data and answer with
    keys that are destroyed ("crypto-shredding") rather than edited events. Silo already keeps
    message text apart as bulky content (`bulky-content-once`), which can be removed without
    touching a line, so the case that needs it is covered. Known practice; no source fetched.
- **Evidence:**
  - Claude Code's own record is the counter-case. Its transcripts and its file snapshots live in
    the same home folder the agent works in, and its snapshots are deleted "about 30 days after the
    session last saved one"; its checkpoints track only changes made through its file editing
    tools, and "A file that only Bash commands changed isn't tracked"
    ([Claude Code, Checkpointing](https://code.claude.com/docs/en/checkpointing)). Verified. Inside
    main, a restore puts those transcripts back too. Silo's record outside the computer is the only
    account of those six hours that survives it.
  - Codex writes its own record to `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`, lines of
    `{timestamp, type, payload}` with types such as `session_meta`, `turn_context`, `response_item`
    and `event_msg`. Not verified in Codex's source; read from third-party parsers
    ([toolpath-codex](https://docs.rs/crate/toolpath-codex/0.5.0/source/src/types.rs),
    [a write-up](https://dev.to/milkoor/reverse-engineering-codex-cli-rollout-traces-3b9b)). Same
    position: inside the computer, editable by the agent, rolled back by a restore.
  - CloudTrail makes deletion and editing on its side detectable: a SHA-256 hash of every log file,
    an hourly digest file that lists them, signed, each digest carrying the signature of the one
    before ([AWS](https://docs.aws.amazon.com/awscloudtrail/latest/userguide/cloudtrail-log-file-validation-intro.html)).
    Verified. Silo does not need this in the first build, since nothing inside main can reach the
    record on the Mac (`device-not-shared`). It is the known answer if polarzero later wants
    removal by hand on the Mac to leave a trace, which the notes name as a cost.
- **Risks and what would change this:** one real way for a computer to change the record exists in
  the first build, and it is not in the plan. The disk comparer on the Mac reads a disk that a
  hostile agent wrote. If that reader runs in the same process that holds the record open for
  writing, a crafted disk that breaks the reader can edit lines. The property holds only if the
  reader of a computer's disk cannot write the record: a separate process with no write access to
  the record's files, sending its lines over a channel to the recorder like any other source.
  `fuzz-untrusted-parsers` covers the parser; this covers what a broken parser can reach.
- **For the first build:** does not block once taken. The builder needs: the record lives outside
  main's disk and outside the folder a restore replaces; the simulator checks that no sequence of
  calls from main removes a line; the disk reader runs apart from the recorder.

---

## Cross-cutting findings

1. **File facts must name their two checkpoints, and "the one before" is the one the disk came
   from, not the one before in time.** This touches `file-changes-from-outside` and
   `kinds-of-lines`. After a restore from `cp-0412` to `cp-0398`, with `cp-0413` saved first, the
   next checkpoint `cp-0414` has to be compared with `cp-0398`. Compared with `cp-0413`, every file
   the restore put back shows as changed with no author, which `books-balance` would then list as
   unattributed, while it was you. Git solves the same case with parents: a commit is compared with
   its parent, and history is a tree. The same rule serves forks in the second build: a fork's
   first checkpoint is compared with the checkpoint it was made from. Nothing needs reshaping
   later if this is written now. It also changes the command: `silo changes main --since cp-0398`
   has to say whether it follows main's current line of checkpoints or time. Following the line,
   as `git log A..HEAD` does, is the one that gives a true answer after a restore.

2. **"Nothing changed" and "not compared" must look different.** Each pair of checkpoints should
   end with a closing control fact that gives the number of file facts written for it, 0 included,
   or with a gap. This is what CloudTrail's hourly digest does for empty hours. Without it, a
   comparison that crashed and a quiet night both print nothing.

3. **Comparison and thinning race.** Comparing takes 3 to 22 s for 200,000 files in the trials of
   `file-changes-from-outside`, and a real macOS main holds a few million. With checkpoints every
   few minutes, comparisons can fall behind. Thinning (`thinning-old-checkpoints`) must not remove
   a checkpoint whose comparisons are not written, or the file facts of that stretch are lost for
   good. Either thinning waits for the comparison, or the comparison skips to the next checkpoint
   and the wider pair says so in its lines (finding 1 makes that self-describing). Neither is in the
   plan.

4. **A disk source can run again; a live stream cannot.** `sources-number-their-lines` says a
   restart "is never read as a count with no loss". That is right for the probe and the network,
   where lines that were not sent are gone. For the disk comparer, both checkpoints are still there
   after a crash, and the comparison can run again and give the same lines. Applying the restart
   rule to it writes a gap that need not exist. The fix is one sentence in
   `sources-number-their-lines`: a source that can produce its lines again from what Silo keeps
   does so after a restart, and writes a gap only for what it can no longer produce.

5. **A file fact's time is the pair of checkpoints, never the file's own times.** `what-a-fact-carries`
   (taken) says a fact carries "its time". For a file fact the change happened somewhere between two
   checkpoints. The file's modification time is written by the agent and can be set by root inside
   main, as the trials note. OpenTelemetry keeps two times for this reason: `Timestamp`, "when the
   event occurred measured by the origin clock", which may be missing, and `ObservedTimestamp`,
   "when the event was observed by the collection system"
   ([data model](https://opentelemetry.io/docs/specs/otel/logs/data-model/), verified). The record
   should write the time it saw the fact (the Mac's clock) and the two checkpoints, and treat the
   modification time, if kept at all, as part of what the disk says, like a file's content.

6. **The first build keeps no conversation outside main.** The first build has no model traffic, so
   `conversation-text-kept` writes nothing. After a restore, the conversation of the undone hours
   lives only in the checkpoint that `restore-saves-first` took, and thinning removes it in time.
   This is consistent with `what-a-restore-leaves`, but polarzero may expect the record to hold the
   conversation. It does from the second build on. The first build should say so in `silo restore`'s
   output.

7. **Challenge to a decision in force, `what-a-fact-carries` (taken).** "It carries its time, its
   computer and its source" is right but leaves out the two things that finding 1 and finding 5
   show every file fact needs: the stretch it covers, and the version of the source (`sources`
   above). They can be added where they belong (`kinds-of-lines`, `sources`) without touching the
   taken entry, so I would not replace it. I name it because a builder who reads only the taken
   entry will make one time field and reshape it later.

8. **Measurements still owed, for these six entries.** None of the six is held up by a Mac
   measurement. Two facts would sharpen them: how many "changed" lines macOS itself makes on an
   idle main between two checkpoints (Spotlight, caches, extended attributes), which decides
   whether extended attributes belong in "what changed", and how long a comparison of two
   checkpoints of a real main takes, which decides how often finding 3 happens.

## Not verified

- That FileVault, or encryption of a single APFS volume, can be turned on inside a macOS virtual
  machine on Apple silicon.
- Codex's rollout format in Codex's own source; only third-party parsers were read.
- That the OpenTelemetry instrumentation scope holds a version (true in the specification as I know
  it; the page fetched does not say).
- The trial ran on ext4-like Linux metadata from `os.lstat`, not on APFS. Its counts show the shape
  of the problem; APFS numbers will differ.

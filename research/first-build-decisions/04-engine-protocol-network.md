# How clients talk to the engine, and what carries a computer's network: a second opinion

An agent's review, 24 November 2026, for polarzero to rule on five decisions of the part `engine`:
`numbered-events`, `call-ids`, `formats-carry-versions`, `who-can-call-the-engine` and
`network-stack`. It changes nothing in `plan/`. Each entry was read with `plan show`, with the
entries it needs and the decisions in force that bind it.

"Verified" means I read the source code, the document, or ran the measurement myself today. "Not
verified" means it comes from a search summary or from memory, and has to be checked before
anything rests on it.

The first build's case, used throughout: main is a macOS computer on polarzero's Mac. The `silo`
command on the Mac is the only client. Claude Code and Codex run inside main with every check off.
The engine takes a checkpoint every few minutes, restores one on request, and the record lists
which files changed between two checkpoints.

| id | Recommendation | Confidence | One-line reason |
|---|---|---|---|
| `numbered-events` | Take with an edit | high | Kubernetes and etcd follow state this way. A number alone does not survive a rebuilt or damaged engine state, which `formats-carry-versions` and `engine-saved-state` both allow, so a number needs the id of its history |
| `call-ids` | Take with an edit | high | TigerBeetle's sessions answer the open question, but they only cover a retry by the same process. "An agent's script that retries" starts a new process and makes a second fork. The table of ids also has to be in the saved state |
| `formats-carry-versions` | Take with an edit | medium | Right for the record. As written it also covers the engine's log, and a newer engine cannot replay an older engine's inputs. The output of `silo`, which scripts parse, is not covered at all |
| `who-can-call-the-engine` | Take with an edit (narrowed) | medium-high | On a Mac, nothing tells your own call from an agent on your Mac running the signed `silo`. 1Password's CLI shows what can be told apart: a person who confirms. A Unix socket and the caller's user are enough for the first build |
| `network-stack` | Take with an edit | medium | gvproxy already speaks Apple's datagram protocol, used by Podman and Lima. Measured here: 0.4 Gbit/s at MTU 1500, 2.3 Gbit/s at 65520. Out of the box it lets a computer reach your Mac's 127.0.0.1, which Apple's NAT does not |

The three findings that matter most:

1. **The engine's socket must never be TCP, and gvproxy's defaults must be changed.** gvproxy
   translates the address 192.168.127.254 to your Mac's 127.0.0.1 unless told otherwise. My
   experiment below reached a server on the host's 127.0.0.1 from the guest this way. An engine,
   or any tool of yours, that listens on localhost TCP is then reachable from inside main. Apple's
   NAT does not reach 127.0.0.1. Moving to Silo's stack without that change gives a computer more
   reach on your Mac than it has today.
2. **A Mac cannot tell your call from an agent's on your Mac.** Code signing proves which program
   connected, not who asked. Any process of your user can run the signed `silo`. Only a person who
   confirms, through Touch ID or a click in the app, separates the two. The property
   `every-call-authenticated` can hold, but it must not be read as more than "this call came from
   your user account, or from that computer".
3. **Engine numbers and call ids have to outlive a restart, an update and a rebuild.** Three
   entries together leave this open. Events need the id of their history. The table of call ids
   needs to be in the saved state. An update needs a rule for the log an older engine left.

---

## `numbered-events`: How does a client follow the engine?

- **Recommendation:** Take with an edit. The design is the known one, but a number on its own
  cannot tell a client that the engine's history started over, and two other entries allow that.
- **Confidence:** high. It rests on the Kubernetes and etcd designs, read today, and on reading
  `engine-saved-state` and `formats-carry-versions` side by side.
- **What it decides, on one real case:** You run `silo restore main <checkpoint>` and the command
  waits for the restore to finish. It holds the state at number 4,120 and follows the events after
  it. Your Mac sleeps for 20 minutes. When it wakes, the command asks for the events after 4,120.
  If those are still in the ring, it applies them and prints that the restore finished. If not, it
  takes the whole state at the current number and reads the restore's outcome from it.

  The case the answer misses: during the sleep, the engine found its save damaged and started from
  a rebuilt state, which `formats-carry-versions` allows ("Silo's own working state may be
  rebuilt"). The new history is at number 37. The command asks for "after 4,120". Compared by
  number alone, the engine has no events after 4,120 and sends nothing. The command then shows a
  state the engine no longer has. That breaks the property this entry sets.
- **Options:**
  - Numbered events with resync, as written. Kubernetes watch and etcd work this way. The cost is a
    reload now and then, plus the ring.
  - The same, with a history id beside the number (recommended). One more field per answer.
    Kubernetes needs no such id because etcd revisions never go backwards in one cluster. Silo's
    engine can start over, so it needs one. Kafka pairs offsets with a leader epoch for the same
    reason (not verified today).
  - Poll the whole state on a timer. It is simpler, but it misses what came and went between two
    polls.
- **Evidence:**
  - Kubernetes keeps 5 minutes of history by default. A client that asks for an older
    `resourceVersion` gets `410 Gone` and "must handle the case by recognizing the status code
    `410 Gone`, clearing their local cache, performing a new get or list operation, and starting the
    watch from the `resourceVersion` that was returned." Kubernetes also sends `BOOKMARK` events
    that only advance a client's number, so a quiet client does not fall out of the window.
    Verified: <https://kubernetes.io/docs/reference/using-api/api-concepts/>.
  - `events-until-the-recorder` says applying the saved inputs again "gives every event back under
    its number". That holds within one history only. `engine-saved-state` leaves "what it then
    starts from" open after damage. Verified by reading both entries.
  - `03-engine-shape.md` (this folder) shows that replaying an older engine's log with a newer core
    is where Temporal and TigerBeetle both broke. An update is a second way for numbers to shift.
- **Risks and what would change this:** If `engine-saved-state` decides that the engine never
  starts from anything but its own saved history, and an update always replays nothing, the history
  id is never needed. The plan currently says neither.

  A second, smaller gap: the app shows the state at N while it is cut off, after sleep for example.
  That state was real, so the property holds, but it is old. A line saying "as of 14:02, not
  connected" costs little and avoids a wrong reading. That belongs to the app, not here.
- **Proposed text:**

  > answer: Events are numbered within one history of the engine. A history has an id, which
  > changes whenever the engine starts from anything other than its own saved inputs. A client is
  > the state at number N of history H, plus every event after N. The engine keeps recent events in
  > a ring of fixed size, and never waits for a client. A client that asks for the events after N
  > when they have left the ring, or whose history is not the engine's, is told to start again from
  > the whole state at the current number, and is never handed later events with a hole before
  > them.

  The property `app-shows-only-real-states` stays as it is.
- **For the first build:** It does not block the build, and the build needs only a little of it.
  The `silo` command reads the state once for `silo checkpoints`. It follows events for the
  commands that wait, such as `silo restore`. A ring of a few hundred events is enough. Without
  the history id, the first time the engine rebuilds its state, every waiting command misreads it.

---

## `call-ids`: What happens when a caller repeats a call?

- **Recommendation:** Take with an edit. Name TigerBeetle's sessions as the mechanism. Put the
  table of ids in the engine's saved state. Answer a call when the core applies it, not when its
  work ends. Say plainly that a new process repeating a command is not a repeat.
- **Confidence:** high. It rests on TigerBeetle's documentation and source, read today, and on
  Stripe's documentation.
- **What it decides, on one real case:** You type `silo restore main <checkpoint>`. The engine
  writes the call to its log, applies it, and crashes before the answer leaves. The command
  reconnects and sends the call again with the same session and number. The restarted engine
  replays its log, so its table of ids again holds that call, and the command gets the first
  answer. Without the table in the saved state, the replayed engine would not know the call, and
  would restore a second time. A crash between applying a call and answering it is the most likely
  way to lose an answer on one Mac.

  The case the answer gets wrong: an agent's script runs `silo fork main`, the command is killed
  before it prints, and the script runs `silo fork main` again. The second run is a new process
  with a new session. It is not a repeat, and it makes a second fork. Today's notes say the
  opposite: "The same holds for an agent's script that retries".
- **Options:**
  - Random ids kept in a table of fixed size (the answer as written). Stripe works like this. The
    weak point: once an id leaves the table, a random id looks exactly like a new one. Stripe says
    so: "We generate a new request if a key is reused after the original is pruned." The answer's
    "an id too old for the table is refused as too old" cannot be done with random ids.
  - Sessions with numbered requests (recommended). TigerBeetle works like this. A client
    registers and gets a session. Each request carries the session and a number one higher than
    the last. The engine keeps, per session, the last number and its answer. A lower number is old.
    The same number gets the kept answer. An evicted session is told it was evicted, and nothing it
    sends runs again. One request is in flight per session. The app can open several sessions. The
    cost is a register call per `silo` process, one more write to the log.
  - Names as the key for actions that make something (a complement, not an alternative).
    `silo fork main --name try-parser` run twice finds `try-parser` already there and answers
    "exists". TigerBeetle tells applications to do exactly this for restarts: "Handling
    application crash recovery safely requires using `id`s to idempotently retry events." This is
    the only way a script that reruns a command can avoid a second fork.
- **Evidence:**
  - TigerBeetle: "A client session may have at most one in-flight request", sessions are limited
    to `clients_max` (64 by default), the session that committed longest ago is evicted, and "When
    a client restarts ... it does not resume its old session — it starts a new session".
    Verified: `docs/reference/sessions.md` and `src/constants.zig` in
    <https://github.com/tigerbeetle/tigerbeetle>.
  - TigerBeetle, "The App or Browser Should Generate the ID", persisted before the request.
    Verified: `docs/coding/reliable-transaction-submission.md`.
  - Stripe keeps keys at least 24 hours, refuses a reused key whose parameters differ, and treats a
    request that "conflicts with another request that's executing concurrently" as a retryable
    error. It does not wait and hand back the first result. Verified:
    <https://docs.stripe.com/api/idempotent_requests>.
  - The answer's "a repeat that arrives while the first is still under way gets that same result
    when it is ready" assumes a call's result arrives when its work ends. A fork or a restore takes
    many effects and minutes. If the answer waits for them, the table holds open answers for
    minutes, and a command you stop with Ctrl-C leaves one behind. In `engine-shape` the core
    answers each input when it applies it, and the work goes on as effects and events. The answer
    to "restore main" should therefore be "accepted, operation 81" or a named refusal. The command
    then follows operation 81 through `numbered-events`. With that, "still under way" only covers
    the short time before the call is written to the log. It is one in-flight request per session,
    as in TigerBeetle. Reasoned from the entries, not tested.
- **Risks and what would change this:** If the plan wants scripts to retry without names, every
  `silo` command that changes something would need an id from the caller, such as
  `--call-id <uuid>`, kept for a set time as Stripe does. That moves the cost onto every script
  author and brings back the problem of the pruned key. Names are cheaper where they exist. Restore
  has no name to key on. Run twice by mistake, it restores twice. `restore-saves-first` makes that
  recoverable, because the second restore first saves where you were.
- **Proposed text:**

  > answer: Every client opens a session, and every call carries its session and a number one
  > higher than that session's last. The engine keeps, for each session, the number and the answer
  > of its last call, in its saved state, so the table survives a crash of the engine. A call is
  > answered when the core applies it: accepted, with the id of the operation it started, or
  > refused with a named error. The operation's progress and end follow as events. Repeating a
  > call returns its first answer, and nothing is done again. A call with a lower number, the same
  > number with different content, or a session that was evicted, is refused with a named error,
  > never carried out. The engine keeps sessions in a table of fixed size and evicts the one whose
  > last call is oldest. A new process is a new session. A call that makes something with a name
  > the caller chose answers "exists" when it is repeated, so a script that runs a command again
  > does not make it twice.

  The property `repeated-call-does-nothing-twice` stays. Its simulator check should include a crash
  between applying a call and answering it.
- **For the first build:** Needed. It is small, about one table and one register call. The
  first build has `silo restore`, which runs one at a time per computer, and automatic checkpoints,
  which are not calls. It can leave names out until forks arrive in the second build.

---

## `formats-carry-versions`: How does Silo read what an older version of it wrote?

- **Recommendation:** Take with an edit. Keep "read forever, never rewritten" for the record. Give
  the engine's saved state its own rule, because it holds inputs that only the version that wrote
  them can replay. Version messages per connection, not per message. Cover the `silo` output that
  scripts parse.
- **Confidence:** medium. The record part is high. The engine part rests on TigerBeetle's upgrade
  design, read today, and on the replay problem already raised in `03-engine-shape.md`.
- **What it decides, on one real case:** On Thursday the engine crashes. Its log ends with three
  inputs not yet in a save. On Friday Silo updates itself before the engine runs again. The new
  engine finds a log written by the old one. The answer says the reader "states the versions it
  reads". Reading the bytes of those three inputs is easy. Applying them is the problem.
  `engine-saved-state` relies on "the core is deterministic, so this gives the state it had, with
  the same events under the same numbers". That holds only for the same core. If the new core
  handles one of those inputs differently, it gives a state the old engine never had, with event
  numbers that clients already hold for other events.
- **Options:**
  - As written: every format carries a version, and newer readers read every older one. This is
    right for the record, the checkpoints' list and any file read years later. For the engine's
    log, it promises something it cannot deliver.
  - Old binaries inside the new one, as TigerBeetle does. Each binary carries past releases, and a
    replica replays an old log with the release that wrote it. The cost is a larger binary and a
    complex build.
  - Drain before the update (recommended for Silo). Before the update replaces the engine, the
    running engine makes a new save with no inputs after it. The new engine reads only saves,
    never older inputs. If it finds an older engine's inputs anyway, after a crash for example, it
    refuses to start with a named failure. It keeps the previous engine binary to replay them,
    which is the one case where it needs TigerBeetle's idea, reduced to one version back. The cost
    is that the save format, not the log format, must be read by every newer engine.
  - Converted on update: rejected in the notes, and rightly so for the record.
- **Evidence:**
  - TigerBeetle bundles "multiple underlying TigerBeetle binaries of different versions, into a
    single binary" so that "a replica [can] crash after the binary has been upgraded, and still come
    back online". Verified: `docs/internals/upgrades.md` in the TigerBeetle repository.
  - `engine-saved-state` already lists "what a newer engine does with an older format" as to find
    out. This entry appears to answer it, but it only answers the reading, not the replaying.
    Verified by reading both.
  - `record-queried-with-ordinary-tools` says "the record's own files are not the way in, because
    their format can change from one version to the next", and that the `silo` command's JSON lines
    are. So the JSON lines are the format other people's scripts depend on, and this entry does not
    cover them. Verified by reading the entry.
  - On messages: the app, the command and the engine ship in one bundle. They differ in version
    only during an update, or when a remote host runs an older engine. A version stated once when a
    connection opens covers both cases. A version on every message adds bytes and no protection.
    The notes of `engine-client` already say "unknown fields are refused". That means a newer
    client must know which version the engine speaks before it sends a field, so a handshake is
    needed in any case. Reasoned, not tested.
- **Risks and what would change this:** If the engine's saved state counts as "working state" that
  may be rebuilt, the update problem becomes a rebuild problem. A rebuild loses whatever only the
  engine knows: the table of call ids, the event numbers, and the operations under way. If
  polarzero accepts losing those at each update, the simpler rule holds. Then `numbered-events`
  needs the history id above.

  Downgrading is a real case too. After a bad update, an older Silo refuses the newer files with a
  named error, as the answer says. The result is that the engine does not start after a downgrade.
  That should be said, so nobody reads it as a bug.
- **Proposed text:**

  > answer: Any newer Silo reads the records of any older version and never rewrites them.
  > Everything Silo writes to read later starts with the version of its format: record files,
  > checkpoint lists and the engine's saves. The lines the `silo` command prints carry the version
  > of their format, and a newer Silo keeps printing an older version when asked. A connection
  > between the engine and a client starts by stating the versions each side speaks, and goes on in
  > one of them. Before an update replaces the engine, the running engine makes a save with no
  > inputs after it. A newer engine reads every older save, and replays inputs only of its own
  > version. A reader refuses any version it does not read with a named error.

  The property `old-records-stay-readable` stays.
- **For the first build:** Needed before the first file is written, as the notes say. The first
  build writes three formats: the engine's save and log, the checkpoint list, and the file-change
  lines. The drain-before-update rule only matters at the first update after release. It can be
  written down now and built then. The version field cannot wait.

---

## `who-can-call-the-engine`: Who can call the engine?

- **Recommendation:** Take with an edit, narrowed. Keep the first four sentences. Replace "to find
  out" with what was found. Do not promise that the engine tells your call from an agent's on your
  Mac. No mechanism on macOS does that without a person confirming.
- **Confidence:** medium-high. It rests on XNU's source, Apple's documentation, 1Password's and
  Tailscale's published designs, all read today. Nothing was run on a Mac.
- **What it decides, on one real case:** Three callers, each tried against the answer.
  1. An agent inside main runs `silo` there and asks to loosen main's rules. Its call reaches the
     engine through main's helper, over the computer's own channel (virtio-vsock or the SSH way
     in). The helper stamps it "from main". Nothing the call says can change that. This is the case
     the answer is built for, and it holds. The origin comes from the transport, and it is cheap.
  2. Another user of your Mac runs a script that connects to the engine's socket. The engine checks
     the caller's user with `getpeereid` and refuses it before any handler runs. This holds too.
  3. Claude Code runs on your Mac, outside every computer, as your user. `mac-agent-not-the-main-path`
     says this is not the main path, but it happens. It runs `silo` and asks to loosen main's
     rules. The call arrives on the same socket, from the same user, from the same signed program
     as your own. The answer's "every call proves where it comes from" suggests the engine can tell
     the difference. It cannot. A code-signing check proves which program connected, and any
     process of yours can start that program.
- **Options:**
  - Same user, by the socket (recommended for the first build). A Unix socket in a directory only
    you can open, and the caller's user checked with `getpeereid` or `LOCAL_PEERCRED`. Docker
    Desktop, OrbStack and the Tailscale CLI all stop here: any process of your user can drive them.
    It costs almost nothing.
  - Same user and Silo's own signed program. On top of the above, the engine checks the caller's
    code signature against Silo's team. Over XPC, macOS checks every message against the
    requirement. Over a Unix socket, the check is racy, as the evidence below shows. 1Password's
    CLI uses XPC with code signatures on macOS. The cost: an XPC path on the Mac beside the Unix
    socket on Linux, unsigned development builds that cannot connect, and still no defence against
    case 3.
  - A person confirms what grants more. A call that would let a computer do something it could not
    do before (`granting`) waits for Touch ID or a click in the app, for a limited time. 1Password
    does this per terminal session, for 10 minutes. This is the only option that separates case 3
    from you. It is a product choice, because it adds a prompt you see. It belongs with
    `what-an-agent-inside-may-do` ("anything that would grant it more asks the user"), and should be
    put to polarzero there, not settled here.
  - Localhost TCP with a token in a file, as Tailscale does on macOS. Rejected. A computer reaches
    your Mac's 127.0.0.1 through gvproxy's default settings (see `network-stack`).
- **Evidence:**
  - macOS has `LOCAL_PEERCRED`, `LOCAL_PEERPID`, `LOCAL_PEEREPID`, `LOCAL_PEERUUID` and
    `LOCAL_PEERTOKEN` (the audit token) on Unix sockets. Verified: `bsd/sys/un.h` in
    <https://github.com/apple-oss-distributions/xnu>.
  - In XNU, `LOCAL_PEERCRED` returns the credentials saved when the connection was made.
    `LOCAL_PEERPID` and `LOCAL_PEERTOKEN` work differently. They look up `last_pid`, the last
    process that used the peer's socket, at the moment you ask, and `LOCAL_PEERTOKEN` reads that
    process's audit token then. A process that sends a call and then runs `exec` on the signed
    `silo` keeps its pid and its socket. A check made after the bytes arrived then sees the signed
    program. Verified by reading `bsd/kern/uipc_usrreq.c` (the `LOCAL_PEERTOKEN` case). Whether
    the audit token's pid version changes on `exec` was not checked today. Samuel Groß's "Don't
    Trust the PID" (2018) describes this class of bug for XPC (not verified today).
  - XPC checks the requirement on every message. `xpc_connection_set_peer_requirement` (macOS 26)
    says: "All messages received on this connection will be checked to ensure that they come from a
    peer who satisfies the requirement." `xpc_peer_requirement_create_team_identity` accepts
    Developer ID on macOS. Verified: Apple's documentation (JSON of
    <https://developer.apple.com/documentation/xpc/xpc_connection_set_peer_requirement>). The older
    `xpc_connection_set_peer_code_signing_requirement` (macOS 12) was not reached today.
  - 1Password: on macOS "Authenticity of both is confirmed by verifying the code signature" over
    XPC. On Linux the app checks the caller's group ID. Authorization is scoped to the terminal
    session, expires "after 10 minutes of inactivity", and has a hard limit of 12 hours. It says
    apps with Accessibility permission "may be able to circumvent the authorization prompt".
    Verified: <https://www.1password.dev/cli/app-integration-security/>.
  - Tailscale on macOS: a CLI that is not the app reads a port and a token from a "sameuserproof"
    file written to `/Library/Tailscale`. That proves only the same user. Verified:
    `safesocket/safesocket_darwin.go` in <https://github.com/tailscale/tailscale>.
  - Docker Desktop: the socket is `~/.docker/run/docker.sock`, and `/var/run/docker.sock` is an
    optional link to it. Docker's documentation treats the Linux virtual machine as the boundary,
    not the socket. Not verified at the source (search summary of
    <https://docs.docker.com/desktop/setup/install/mac-permission-requirements/>).
  - A remote Linux host needs no new credential. An SSH forward of a Unix socket
    (`ssh -L /local.sock:/run/silo/engine.sock host`) reaches the remote engine as the SSH user,
    and the engine reads that user with `SO_PEERCRED`. Your SSH key is the proof. Docker reaches a
    remote engine over SSH the same way (`DOCKER_HOST=ssh://`). Known practice, not tested today.
  - A Unix socket's path on macOS must be shorter than 104 bytes. vfkit checks this
    (`maxUnixgramPathLen = 104`, verified in `pkg/vf/virtionet.go`). A path under
    `~/Library/Application Support/` with a long user name can come close. The engine should
    refuse a path that is too long with a named error, not cut it.
- **Risks and what would change this:** If polarzero wants case 3 refused, the change goes to
  `what-an-agent-inside-may-do` and to the app: a granting call from the Mac waits for a person.
  Then XPC with a code-signing requirement becomes worth its cost, because the prompt must come
  from Silo's own app and not from a look-alike. Until then, a code-signature check costs a
  second transport and protects only against programs that could run `silo` anyway.
- **Proposed text:**

  > answer: Every call to the engine is authenticated and carries where it comes from. Where a
  > call comes from is taken from the way it arrived, never from what the call says about itself.
  > A call that is not authenticated is refused before any handler runs. A call from inside a
  > computer arrives through that computer's helper, which marks it as that computer's, and it can
  > grant the computer nothing more. On the Mac, the app and the command line reach the engine
  > through a Unix socket in a directory only your user can open, and the engine refuses a caller
  > of any other user. That proves the call came from your user account, and nothing more: a
  > program you run, an agent on your Mac included, can call the engine as you do. On a remote
  > host, the socket is reached over SSH, and the proof is your SSH login. The engine never listens
  > on a network port.

  The property `every-call-authenticated` stays. Its hostile test can check: a caller of another
  user is refused; a call that claims to come from the app but arrives through main's helper is
  treated as main's; nothing listens on a TCP port.
- **For the first build:** Needed, and the narrowed form is enough. The first build has one
  client, the `silo` command on the Mac, and nothing inside main calls the engine. It needs the
  socket, its directory's permissions, the `getpeereid` check and the path-length check. Calls
  from inside a computer arrive with the probe or with `silo` inside main, in a later build.

---

## `network-stack`: What carries a computer's network at the boundary?

- **Recommendation:** Take with an edit. Silo's own stack is right, and gvproxy makes it within
  reach for the first build. The answer needs three changes. Name the first driver. Say that the
  computer reaches neither your Mac's loopback nor its local network through it, which gvproxy does
  not ensure by default. Allow the first build to fall back to Apple's NAT, shown as having no
  boundary, only if the Mac test below fails.
- **Confidence:** medium. gvproxy with vfkit and Lima is verified in source, and throughput was
  measured on Linux. A macOS guest behind gvproxy was not tried, because this container has no
  Mac.
- **What it decides, on one real case:** In main, Claude Code runs `npm install` on a project with
  1,200 packages, and later `git clone` of a 2 GB repository. With Apple's NAT, the packets leave
  through macOS's own NAT, with no Silo code in the path. With gvproxy, every frame from main goes
  over a datagram socket into a process of Silo's. That process runs gVisor's TCP/IP stack, opens
  a connection of its own on the Mac for each connection main opens, and copies the bytes between
  the two. If that process stops, main's network device loses its other end, and main has no
  network.
- **Options:**
  - Apple's NAT (`VZNATNetworkDeviceAttachment`). No code, no entitlement. Tart and UTM use it by
    default (not verified today). It cannot see a connection, hold a request or keep a secret
    outside. One way around that exists: `pf` rules on the Mac's `bridge100`. They need root,
    filter only by address, and clash with VPN software. A computer reaches your local network and
    every service your Mac exposes on that bridge, but not your Mac's 127.0.0.1.
  - gvproxy (gvisor-tap-vsock) behind `VZFileHandleNetworkDeviceAttachment` (recommended as the
    first driver). Podman machine on macOS uses it with vfkit, and Lima runs it inside its own
    process for `vz` machines. It decides per connection, at the moment main opens it: the
    forwarder dials out before it completes main's handshake. A rule by address and port goes
    there. A rule by host name needs its DNS server, which it has. Secrets kept outside need a TLS
    proxy on top, which gvproxy does not do. It is written in Go, and the helper is Swift
    (`macos-vm-driver`). It either runs as a process of its own per computer, or is linked into the
    helper as a C archive. The plan says the helper holds the boundary
    (`which-process-holds-the-boundary`), so the C archive fits, or that entry should name the
    stack's process.
  - A stack written for Silo, as OrbStack did (OrbStack claims up to 45 Gbit/s between macOS and
    its Linux machine, not verified). It is the most work, and nothing in the first build needs
    it.
  - passt, as Podman uses on Linux. It works on Linux only, so it is not an option for a macOS
    computer. passt claims "4 to 50 times IPv4 TCP throughput" of similar tools, depending on MTU
    (verified at <https://passt.top/passt/about/>, with no absolute numbers given).
- **Evidence:**
  - Apple's protocol: "A network device that transmits raw network packets and frames using a
    datagram socket." The MTU is 1500 by default, 1500 at least and 65535 at most (macOS 13). The
    receive buffer must be "at least double" the send buffer, and "four times" is recommended. No
    entitlement is needed. The attachment of a running machine can be changed (macOS 12): "Setting
    this property results in an attempt to change the network device attachment". So a restarted
    stack can be attached again without stopping main. Verified: Apple's documentation (JSON) for
    `VZFileHandleNetworkDeviceAttachment`, its `maximumTransmissionUnit` and
    `VZNetworkDevice.attachment`.
  - vfkit opens a `SOCK_DGRAM` Unix socket bound to a path, sets the send buffer to 1 MiB and the
    receive buffer to 4 MiB, and sends the 4 bytes `VFKT` so gvproxy learns its address. gvproxy
    reads that with `MSG_PEEK`. Verified: `pkg/vf/virtionet.go` in
    <https://github.com/crc-org/vfkit> and `pkg/transport/unixgram_unix.go` in
    <https://github.com/containers/gvisor-tap-vsock>.
  - gvproxy's default: `config.Stack.NAT = { HostIP: "127.0.0.1" }`, so 192.168.127.254 inside the
    computer is your Mac's 127.0.0.1. The default applies whenever the NAT table is left empty.
    Cloud metadata (169.254.0.0/16) is refused unless enabled. Verified: `cmd/gvproxy/config.go` and
    `pkg/services/forwarder/tcp.go`. Lima added `networkOpts.hostLoopback: false` "for a guest that
    runs untrusted code" (verified in Lima's `docs/config/network/user.md`).
  - gvproxy can forward a Unix socket on the Mac to a TCP port in the computer. `ssh main` can then
    go through a `ProxyCommand` to a socket, with no port opened on your Mac, and no guest address
    for your Mac to know. Verified: `pkg/services/forwarder/ports.go`.
  - gvproxy's DHCP server hands out the MTU (`OptionInterfaceMTU`). Verified in
    `pkg/services/dhcp`. Whether macOS as a guest applies that option was not checked.
  - Measured today, in this Linux container (4 virtual CPUs, no KVM). A Linux network namespace
    with a TAP device played the computer. A small relay moved frames between the TAP and
    gvproxy's vfkit socket, as Apple's framework would, with vfkit's buffer sizes. gvproxy is built
    from `main` at 197eea05 (13 November 2026). The guest downloaded from an HTTP server on the
    host's 127.0.0.1 through 192.168.127.254:

    | Path | One stream | Four streams | gvproxy CPU |
    |---|---|---|---|
    | Host loopback, no gvproxy | 27 to 33 Gbit/s | | |
    | gvproxy, MTU 1500 | 0.38 to 0.45 Gbit/s | 0.53 Gbit/s | about 60% of one core |
    | gvproxy, MTU 4000 | 1.0 to 1.2 Gbit/s | | |
    | gvproxy, MTU 9000 | 1.4 to 1.7 Gbit/s | | |
    | gvproxy, MTU 65520 | 2.2 to 2.5 Gbit/s | 2.86 Gbit/s | about 64% of one core |

    400 downloads of 64 KiB, each on a new connection, 16 at a time: 1.09 s on loopback,
    1.36 to 1.52 s through gvproxy. Each connection costs about 1 ms more.

    What this shows: the cost is per frame, so the MTU is the main lever, at about 5.5 times from
    1500 to 65520. These frames carry no offload, so the guest's kernel cuts every stream at the
    MTU. gvproxy's own README gives "between 1.6 and 2.3Gbits/s ... with a mtu of 4000 with QEMU on
    macOS", which matches. What it does not show: the speed on an Apple CPU, and the cost of
    Apple's framework moving frames. My relay adds a copy Apple does not, so the real number is
    likely higher. Scripts: `netexp/` of the project folder `research/2026-11-24-first-build-decisions-scripts/` (relay, server, three test
    scripts).
  - Apple's NAT: Lima measured 59 Gbit/s from the Mac into a `vz` guest through vzNAT on an M4
    Max. On the same Mac, Lima's port forwarding over SSH or gRPC measured 3.8 to 7.5 Gbit/s, and
    its `lima:shared` network (socket_vmnet) 2.4 to 3.5 Gbit/s. Verified: Lima's `docs/config/port.md`, "Benchmarks". That is traffic between the Mac and the
    computer. For `npm install` and `git clone` from the internet, the limit is your internet link,
    typically 0.1 to 1 Gbit/s. So gvproxy at MTU 1500 is close to most links, and at a larger MTU it
    is above them. The gap matters for traffic between your Mac and main: a 5 GB copy over SSH
    through gvproxy at 0.4 Gbit/s takes about 100 s.
  - Known faults of this stack, in production:
    - An open gVisor bug drops a SYN that reuses a 4-tuple still in TIME_WAIT. Behind
      `tcp.Forwarder` the connection then hangs for up to 60 s. Lima saw "8 to 10 stalls of about
      68 s per 500-connection batch". The fix, gvisor PR #15019 (now #17844), was still open on 24
      November 2026. Verified: <https://github.com/gvisor/gvisor/pull/15019>.
    - gvisor-tap-vsock up to 0.8.9 allowed 10 half-open outgoing connections at a time. The current
      code allows 128 (`defaultTCPMaxInFlight = 128`, verified in `pkg/services/forwarder/tcp.go`).
      That Lima releases still pinned the old version comes from a search summary, not verified.
    - After a Mac woke from sleep, gvproxy spun at 400% CPU in Podman, fixed in gvisor-tap-vsock
      0.8.9, May 2026. Verified: <https://github.com/podman-container-tools/podman/issues/27279>.
    - Each of these would hit agents running `npm install` all night.
- **Risks and what would change this:** The Mac test decides it:
  - main gets an address from gvproxy's DHCP, and `ssh main` works through a Unix socket forward;
  - the MTU inside main can be raised, with the speed of `git clone` and `npm install` through
    gvproxy and through Apple's NAT on the same Mac;
  - a burst of 2,000 new connections, watched for the TIME_WAIT stall;
  - CPU at idle and after sleep.

  If DHCP or the MTU fails for a macOS guest, the first build runs on Apple's NAT. In that case
  Silo shows that main's network has no boundary, and the stack comes in the second build, before
  any rule.

  One thing to design now in any case: the SSH entry Silo writes should go through a `ProxyCommand`
  of Silo's, not to a guest address. Then moving from Apple's NAT to the stack changes nothing in
  your SSH settings or your editor.
- **Proposed text:**

  > answer: Silo's network stack, on Linux and on macOS computers. The first driver is gvproxy
  > (gvisor-tap-vsock), reached through a datagram socket. Never Apple's NAT, which cannot apply a
  > rule or hold a secret at the boundary. Every packet of a computer goes through the stack. If
  > the stack stops, the computer has no network: nothing goes around it. Through the stack, a
  > computer reaches neither your Mac's loopback nor your local network unless a rule opens them.
  > What it costs in speed against a plain machine is measured on a Mac before it is relied on:
  > with `git clone` and `npm install`, at the MTU Silo sets, against Apple's NAT. Until that
  > measurement passes, a macOS computer may run on Apple's NAT, and Silo shows that it has no
  > boundary.

  If polarzero wants no exception at all, drop the last sentence. The first build then waits on the
  Mac test.
- **For the first build:** This blocks `computers-main-on-a-mac`. Narrowed as above, it does not
  block the build. The build needs gvproxy with the NAT to 127.0.0.1 removed, the SSH way in
  through a socket forward, and the MTU chosen. If gvproxy is used, the first build gets no rule
  and no host list. `open-limited-or-closed` and `device-out-of-reach` are later builds.

---

## Cross-cutting findings

**1. Engine numbers, call ids and saved state must agree on what survives.** Four entries each
assume something about the others:

- `numbered-events` assumes numbers never restart.
- `call-ids` assumes its table lives somewhere that survives a crash.
- `formats-carry-versions` lets working state be rebuilt.
- `engine-saved-state` leaves the start after damage and the update open.

Taken together, the case that breaks is an update or a damaged save. After either one, clients
hold numbers and ids from a history that is gone. The fix is small and belongs in three places: a
history id on events, the session table in the saved state, and drain before update. It is easier
to decide all three in one sitting than one at a time.

**2. The engine's socket and a computer's network are one question on a Mac.** The cheapest
authentication there, Tailscale's localhost TCP with a token file, would be reachable from inside
main through gvproxy's default NAT. A tool of yours that listens on 127.0.0.1, such as a local
database, a model server or an MCP server, is reachable the same way. Apple's NAT does not reach
127.0.0.1. Moving main onto Silo's stack without removing that NAT entry gives main more reach on
your Mac than it has today. Neither entry says so. The entry that should hold it is
`device-out-of-reach`. Its property `computer-cannot-reach-your-device` is not met by either
attachment out of the box, and it is not in the first build. I recommend that the first build
remove gvproxy's NAT to 127.0.0.1 anyway. It is one line of configuration, and without it the
first build is a step back on your Mac's loopback.

**3. Challenge to a decision in force: `what-an-agent-inside-may-do`, and how it reads beside
`mac-agent-not-the-main-path`.** The first says anything that would grant a computer more "asks
the user". On a Mac, asking the user over the `silo` command is answered by whatever runs that
command, which may be an agent on your Mac. 1Password solved this with Touch ID per terminal
session, and says that apps with Accessibility permission may still click through. If "asks the
user" is meant to be answered only by a person, it needs a channel an agent cannot drive. That
means Touch ID, or the app with a code-signed connection. The `silo` command alone is not such a
channel. I would add to that decision: "The user answers in Silo's app or with Touch ID, never by
a command." That is polarzero's call, because it adds a prompt you see.

**4. The output of `silo` is the format that lasts, and nothing versions it.**
`record-queried-with-ordinary-tools` makes the JSON lines the way in for scripts, extensions and
your agents. It also says the record's files may change. `formats-carry-versions` versions the
files and not the lines. The first build's main output, `silo changes main --since`, is exactly
such a line. A field that names the version of the line's format costs a few bytes now and cannot
be added later without breaking every script.

**5. New risk, not in the plan: the stack's own faults look like the agent's faults.** A
connection that hangs 60 s on TIME_WAIT, a cap of 10 connections, or gvproxy spinning after sleep
all show inside main as a slow or failing `npm install`. The agent then tries something else, such
as another registry, retries or changed settings, and the record shows its reaction, not the
cause. When the stack is in the path, it should report its own state as facts in the record:
connections refused, stalls, restarts. Otherwise the record misleads in the night-long case the
first build is for. This fits `facts-before-reports`: the stack sees these from outside.

**6. The Mac tests to add to the first build's list** (test 5 and test 6 of the proposal, made
precise):

- gvproxy with a macOS guest: DHCP, the MTU taken from DHCP or set inside, `ssh main` through a
  Unix socket forward, a burst of 2,000 connections, CPU after sleep.
- `git clone` and `npm install` of the same projects through gvproxy and through Apple's NAT.
- The engine's socket: a caller of another user is refused. A connection from inside main to
  192.168.127.254 and to the Mac's LAN address reaches nothing of the engine's.
- `LOCAL_PEERTOKEN` after `exec`: whether the audit token's pid version changes. This only
  matters if a code-signature check over the Unix socket is ever considered.

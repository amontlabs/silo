# What an agent inside a computer may do

Every call the engine answers carries who made it, set by the way the call arrived and never by
the program that sent it:

| Caller | Arrives through | May do |
| --- | --- | --- |
| **Owner** | the engine's own socket on this device (the app, the `silo` command line) | everything |
| **Device** | Connections, from another device of yours | the methods the Connections bridge serves (`remote::OWNER_METHODS`), as before |
| **Computer** | the agent inside a computer (today the in-guest client's bridge folder) | what this page lists |

The rule for a computer: **it can do anything that gives it no more than it already has; anything
that widens what it has asks you; it never touches a computer outside its own tree.**

Its tree is the computer itself and every computer it made, directly or through a computer it
made. "Made" is recorded by the engine when the computer's agent creates or forks a computer
(`<app data>/policy/computers.json`). Being forked from a computer is not enough: a fork you made
by hand from `main` is not `main`'s agent's to touch, because you may have given it more.

The table is the code's (`app/SiloUI/src-tauri/engine/src/policy/rules.rs`); a new method does not
compile until it is classified there.

## The rules

| Rule | Meaning |
| --- | --- |
| Free | always allowed |
| Its own jobs | allowed for a job the computer started |
| Its tree | the target is the computer itself or one it made; lists show only those |
| What it made | the target is a computer it made, never itself |
| Makes a computer | forks itself or one it made, or makes a new one; asks if what the new computer gets (access, CPUs, memory, storage) is more than the caller has |
| Asks you | becomes a request you allow or deny; the call answers `approval_required` with what is asked |
| Never | refused with `permission_denied` |

Every call is refused when it names another device (`deviceId`, `allDevices`, `deviceIds`).
Every answer to a computer is cut before it goes back, wherever the device's state sits in it (a
job's outcome holds the state after a fork, for one): lists of computers keep only its tree, and
the device's secrets, GitHub account, activities and pushes are left out. A job outcome too large
to answer in one piece cannot be cut, so it is withheld.

## Method by method

| Methods | Rule | Notes |
| --- | --- | --- |
| `engine.handshake`, `engine.state`, `engine.disconnect`, `events.unsubscribe` | Free | |
| `device.facts` | Free | The device's CPUs, memory and whether Remote Login is on; nothing about other computers. |
| `device.resources` | Free | What the device has left; see [Limits](#limits). Names only the computer and the computers it made. |
| `audit.list` | Free | Only rows about the computer and the computers it made: calls that name one of them, and calls one of them made. |
| `approvals.list`, `approvals.get`, `approvals.cancel`, `approvals.note` | Free | The handler shows only the approvals of the computer and the computers it made; only the computer that asked withdraws or writes a note. |
| `approvals.decide` | Never | Answering is yours. |
| `operations.job.get`, `.acknowledge`, `operations.cancel`, `guest.exec.get`, `guest.exec.input` | Its own jobs | |
| `operations.list`, `events.subscribe`, `notifications.list` | Never | They cover every computer and are not cut per caller yet. |
| `computers.access` | Its tree | Names and hosts only, never a value. |
| `computers.list`, `computers.tree` | Its tree | The answer holds only its tree; a parent outside it is not named. Only the lists are asked for. |
| `computers.credentials`, `steps.list`, `checkpoints.usage`, `changes.diff`, `logs.query`, `files.list`, `guest.account`, `guest.files.list`, `.read`, `.watch`, `desktop.status`, `computerUse.state`, `clipboard.read`, `network.urls`, `push.repositories`, `push.status` | Its tree | Reading or checkpointing itself gives it nothing new. |
| `checkpoints.create` | Its tree | On the caller's own computer the checkpoint is always a manual one and `keepNewest` is dropped, so retention never removes your automatic and step checkpoints; on a computer it made the parameters pass through (retention there only trims what its agent made). |
| `network.unpublish` | Its tree | Narrowing. |
| `changes.requests.list`, `changes.requests.show` | Its tree | Requests about forks in its tree. |
| `computers.start`, `.stop`, `.restart`, `.forceStop`, `.delete`, `.dismissError`, `macos.setup.retry` | What it made | A computer cannot stop or delete itself. |
| `guest.exec`, `guest.exec.start`, `guest.files.write`, `.remove`, `guest.ssh` | What it made | It already runs anything it likes in itself. |
| `checkpoints.restore`, `.delete`, `.abandonRestore`, `steps.record` | What it made | Its own checkpoints and steps are your history of it. |
| `desktop.action`, `computerUse.setup`, `clipboard.write` | What it made | |
| `changes.request`, `changes.requests.throwAway` | What it made | A review request can target only its tree. |
| `checkpoints.fork` | Makes a computer | Source: itself or one it made. The fork may have any secret the caller has (see [Secrets in a fork](#secrets-in-a-fork)), and a read-only copy of GitHub access the caller's own access covers; anything more asks you. Forking an earlier checkpoint of itself asks you, since it can hold what you have since taken away. The token file of the in-guest client is always scrubbed; a recipe hash is never passed on (it names a baseline other forks share). |
| `computers.create` | Makes a computer | The engine chooses a Linux computer's id. A container computer asks you: it shares the device's kernel. A computer with more CPUs, maximum CPUs, memory, maximum memory or storage than the caller has (a macOS one: CPUs, memory or disk) asks you, with the sizes in the request. Of a Linux computer's `desktop` settings only whether the desktop starts with the computer is passed on (Silo decides the rest). Names Silo reserves (`silo-build-`, `silo-baseline-`) are refused. |
| `changes.keep` | Asks you | Free when both the fork and the computer it merges into are ones it made; asks when it merges into itself; refused anywhere else. |
| `changes.requests.keep` | Asks you | Only for a request about a computer it made; free when the request merges into a computer it made too, asks when it merges into itself. |
| `changes.undo` | Asks you | Free on a computer it made; asks on itself. |
| `changes.promote` (replace main) | Asks you | Free when both the fork and the computer it replaces are ones it made; asks when it replaces itself (the request says it restarts that computer). |
| `network.publish` | Asks you | A port on your device. |
| `sshAccess.save` | Asks you | Turning SSH access off is free, and only the computer and the off switch are taken from the call: the port and address stay what is saved, so nothing it sends waits for you to turn access on. |
| `push.start` | Asks you | A push to GitHub through your device. |
| `github.configuration.save` | Asks you | Only when the computers whose GitHub access changes are in its tree; a configuration that drops or changes another computer's access, or changes nothing, is refused. Token access must name the token secret by id (an agent is never bound to a token by default) and the summary you approve names that token and says a personal token gives its full permissions. |
| `secrets.save` | Asks you | Adding a secret for computers in its tree. Changing a saved secret is yours only. The secret's value is never in the request's summary. A GitHub token has its own summary: it names the token and says it gives its full permissions to any computer that uses it. |
| `changes.capture`, `changes.baseline` | Never | Baselines are cached under keys other computers share. |
| `changes.requests.markKept`, `changes.requests.open` | Never | |
| `computers.configuration.*`, `computers.identities.*`, `computers.launch`, `computers.shell`, `activity.setup.read`, `macos.template.delete` | Never | They read or change every computer at once. |
| `files.upload`, `files.download`, `logs.export`, `clipboard.transfer`, `computerUse.approval`, `display.*`, `desktop.connect`, `macos.display.*`, `editor.*`, `terminal.command`, `guest.sshAlias.*`, `network.open` | Never | They reach your device's own files, clipboard, screen, windows or settings. |
| `guest.prepare` | Never | It adds a key to the one `authorized_keys` file every computer on the device shares. |
| `network.state`, `sshAccess.state`, `sshAccess.connection` | Never | They list every computer. |
| `secrets.list`, `secrets.remove`, `secrets.retry` | Never | A computer never reads a secret's value, and the list shows every computer's secrets. |
| `github.*` other than the configuration save | Never | Your GitHub account and sign-in. |
| `guestClient.*` | Never | Tokens are yours to give. |
| `settings.*`, `connections.*`, `exports.*`, `storage.*`, `updates.*`, `migration.*`, `preparation.*`, `recipes.*`, `engine.shutdown`, `engine.update.*`, `engine.credentials.*`, `device.dependencies` | Never | Engine settings, Connections, exports and imports, updates. |

## Asking you

A call that needs your approval answers at once with the error `approval_required`. Its details
carry the kind (`merge`, `replace_main`, `github`, `secret`, `network`, `port`, `ssh`, `access`,
`container`, `resources`), a summary the engine wrote from the call's parameters, never from the agent's
words, and the id of the approval that now waits for you. The call itself has not happened.

### The approval

One file per approval under `<app data>/approvals/`, readable by you only, written whole. It holds
which computer asked, the method and the exact parameters that will run, the summary, the
agent's note if it added one, the state, the times, and once allowed what the call returned (its
result, the job it started, or the error it ended with). Nothing a computer does changes the
method, the parameters or the summary after the approval is made. For a fork or a new computer
the parameters are the ones the engine checked and rewrote (the agent's token file scrubbed, the
new computer's id chosen by the engine); a secret's value is kept for the call but never shown.

- **Asking twice** for the same call (same computer, method and parameters) while the first is
  waiting gives back the same approval.
- **At most 20** approvals wait for one computer at a time; the 21st call is refused with a plain
  message.
- **Expiry:** a waiting approval nobody answers expires after 7 days; answered, withdrawn and
  expired ones are removed 30 days after they ended.
- **Deleting a computer** withdraws the approvals it was waiting for.
- **The agent's note** (`approvals.note`, at most 500 characters, only the computer that asked)
  explains why. It is shown as the agent's words, apart from the summary, and never in the
  notification.

### Answering

| Method | Who | What |
| --- | --- | --- |
| `approvals.list`, `approvals.get` | you; a computer | You see all. A computer sees the approvals it asked for and those of computers it made, with their state, your note and the outcome, which is how its agent learns the answer. Filters: `state`, `computerId`. |
| `approvals.decide` | you only | `allow` or `deny`, with an optional note the agent reads. Answering an approval that is not waiting is an error. |
| `approvals.cancel` | the computer that asked; you | Withdraws a waiting approval. |
| `approvals.note` | the computer that asked | Attaches or replaces its note while it waits. |
| `computers.access` | you; a computer for itself or one it made | What that computer holds: its secrets' names and hosts, its GitHub access (repositories, read or write), its published ports and its SSH access. Never a value, nothing about other computers. |

**On allow** the engine records the answer first, then checks the call again as if the computer
had just made it: the computer still exists, the computers it names are still its own or ones it
made, and the parameters still pass every check except the asking itself. Only then does it run
the stored call with your authority, write the audit row with the approval's id, and store the
outcome on the approval. If the check fails, nothing runs and the approval ends as allowed with an
error that says why. A job the call starts can be read by the computer that asked, as one it
started itself; a computer the call makes is recorded as made by the computer that asked, as when
it makes one without asking.

**Once only.** An approval leaves waiting once: two answers at the same time give one run and an
error for the other. If Silo stops after recording an allow and before storing what the call
returned, the next start ends that approval with an error saying it was interrupted; the call is
never run again on its own.

**Merges** show you the diff through a review request: see the merge part of "Review requests" in
[SiloUI-CHANGES.md](SiloUI-CHANGES.md). Replacing main (`changes.promote`) and undoing a merge
(`changes.undo`) are plain approvals.

### Telling the app

Every change of an approval publishes `approvals.changed` (`id`, `state`). A waiting approval keeps
one notice of category `approvals` (key `approval:<id>`), titled with the computer's name and the
summary; it is cleared when the approval is answered, withdrawn or expires
(`notifications.cleared` with that `key`). The settings switch `notifyApprovals` (on by default)
turns the system notifications off. Approvals are answered on the device whose computer asked, so
`approvals.changed` is not relayed to other devices.

## Secrets in a fork

A fork may carry any secret its maker holds, whatever that maker's agent was set up to share.
The old per-agent `fork_secrets` list no longer limits it, and the engine does not read it: it was
a setting of the friendly `silo` client, which an agent that wants more simply does not use. The
direction's rule is the limit instead: a computer hands on only what it has, and anything more
asks you.

## The audit log

`<app data>/audit.sqlite3` holds one row for every call that can change something (a command, a
job or a stream) and for every refused or held call: when, who (owner, device with its key
fingerprint when known, or computer with its id), the name the program gave, the method, the
computer it named, the outcome (`ok`, `error`, `refused`, `asked`), the error code and the job id.
Parameters and results are never stored. A row from a computer names a computer only when it is
that computer or one it made, so a probe at someone else's computer shows no target. A call that
repeats the same caller's newest row (same method, outcome, error and computer) adds to that
row's `count` and moves its `lastAt`, so a loop of refused calls fills one row. A call the
in-guest bridge refuses before the policy sees it (an unknown method, a cap) is recorded too.
Rows older than 90 days are trimmed; the owner's and devices' rows keep the newest 200,000 and
each computer's own calls the newest 20,000, so a computer cannot push out any other row.
`audit.list` reads it, newest first; what it returns to a computer leaves out the program name
and any device key.

A change another device makes through one of the Connections bridge's own methods (`runtime.*`,
`macos.*` and the other names in `remote::METHODS`) is recorded there too, with the device's key.

## Limits

There is no fixed number of computers an agent may make or run. It reads what the device has left
with `device.resources` and judges for itself. The answer holds:

- the device's logical CPUs, its memory in total and how much is available now, and the system's
  memory pressure where the system gives one;
- the free and total space of the disk that holds computers;
- how many computers are running, and the memory and CPUs they were set up with;
- the two floors below, as numbers, and a `state` with one sentence an agent can act on:
  `ok`, `low` or `critical`. A computer sees by name only itself and the computers it made; the
  totals count every computer on the device.

Silo prevents one thing: an agent freezing the device or filling its disk. While available memory
is below the **memory floor**, or free disk is below the **disk floor**, a computer's agent cannot
start, restart or create a computer, take or restore a checkpoint, or fork. The call fails before it
does anything, with the code `resources_low` and the facts ("This device has 3.1 GiB of memory
available; Silo keeps 6.4 GiB free. Stop a computer first, or ask the person who runs Silo."), and
is recorded in the audit log as refused.

| | Default | Smallest you can set |
| --- | --- | --- |
| Memory floor | the larger of 4 GiB and 10 % of the device's memory | 1 GiB |
| Disk floor | the larger of 10 GiB and 5 % of the disk | 2 GiB |

Between the floor and twice the floor the state is `low`: the call works, and the answer to
`device.resources` says starting another computer may slow the device or fill the disk. Available
memory counts free, inactive, purgeable and speculative pages on a Mac and `MemAvailable` on
Linux. Your own calls are never held back by the floors. The floors are the settings
`memoryFloorGib` and `diskFloorGib`; `null` returns to the default.

## Other limits

- When a computer is deleted, what it made becomes its maker's, so an agent keeps reaching the
  computers it could reach before.
- A replace main (promote) moves the record of who made which with the swap: the promoted fork is
  made by no one (or takes the old main's place under the computer that made it), and every other
  computer the old main made, with its pending computers and jobs, becomes the fork's, so the old
  main keeps nothing it could reach. An undo puts the record back exactly, and a promote or undo
  finished at engine start moves it the same way.
- The in-guest bridge caps how many of a computer's jobs are unacknowledged (16), which bounds
  the engine's own bookkeeping.
- The network policy is the same for every computer today, so a fork's inherited policy hands
  nothing over. When computers get policies of their own, a fork of a computer with a wider
  policy than its maker's must ask.

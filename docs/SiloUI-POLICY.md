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
| Makes a computer | forks itself or one it made, or makes a new one; asks if what the new computer gets is more than the caller has |
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
| `audit.list` | Free | Only rows about the computer and the computers it made: calls that name one of them, and calls one of them made. |
| `approvals.list`, `approvals.get`, `approvals.cancel`, `approvals.note` | Free | The handler shows only the approvals of the computer and the computers it made; only the computer that asked withdraws or writes a note. |
| `approvals.decide` | Never | Answering is yours. |
| `operations.job.get`, `.acknowledge`, `operations.cancel`, `guest.exec.get`, `guest.exec.input` | Its own jobs | |
| `operations.list`, `events.subscribe`, `notifications.list` | Never | They cover every computer and are not cut per caller yet. |
| `computers.access` | Its tree | Names and hosts only, never a value. |
| `computers.list`, `computers.tree` | Its tree | The answer holds only its tree; a parent outside it is not named. Only the lists are asked for. |
| `computers.credentials`, `steps.list`, `checkpoints.create`, `checkpoints.usage`, `changes.diff`, `logs.query`, `files.list`, `guest.account`, `guest.files.list`, `.read`, `.watch`, `desktop.status`, `computerUse.state`, `clipboard.read`, `network.urls`, `push.repositories`, `push.status` | Its tree | Reading or checkpointing itself gives it nothing new. |
| `network.unpublish` | Its tree | Narrowing. |
| `changes.requests.list`, `changes.requests.show` | Its tree | Requests about forks in its tree. |
| `computers.start`, `.stop`, `.restart`, `.forceStop`, `.delete`, `.dismissError`, `macos.setup.retry` | What it made | A computer cannot stop or delete itself. |
| `guest.exec`, `guest.exec.start`, `guest.files.write`, `.remove`, `guest.prepare`, `guest.ssh` | What it made | It already runs anything it likes in itself. |
| `checkpoints.restore`, `.delete`, `.abandonRestore`, `steps.record` | What it made | Its own checkpoints and steps are your history of it. |
| `desktop.action`, `computerUse.setup`, `clipboard.write` | What it made | |
| `changes.request`, `changes.requests.throwAway` | What it made | A review request can target only its tree. |
| `checkpoints.fork` | Makes a computer | Source: itself or one it made. The fork may have any secret the caller has, and a read-only copy of GitHub access the caller's own access covers; anything more asks you. Forking an earlier checkpoint of itself asks you, since it can hold what you have since taken away. The token file of the in-guest client is always scrubbed; a recipe hash is never passed on (it names a baseline other forks share). |
| `computers.create` | Makes a computer | The engine chooses a Linux computer's id. A container computer asks you: it shares the device's kernel. Names Silo reserves (`silo-build-`, `silo-baseline-`) are refused. |
| `changes.keep` | Asks you | Free when both the fork and the computer it merges into are ones it made; asks when it merges into itself; refused anywhere else. |
| `changes.requests.keep` | Asks you | Only for a request about a computer it made. |
| `changes.undo` | Asks you | Free on a computer it made; asks on itself. |
| `changes.promote` (replace main) | Asks you | Always; the request says it restarts the computer it replaces. |
| `network.publish` | Asks you | A port on your device. |
| `sshAccess.save` | Asks you | Turning SSH access off is free. |
| `push.start` | Asks you | A push to GitHub through your device. |
| `github.configuration.save` | Asks you | Only when the computers whose GitHub access changes are in its tree; a configuration that drops or changes another computer's access, or changes nothing, is refused. |
| `secrets.save` | Asks you | Adding a secret for computers in its tree. Changing a saved secret is yours only. The secret's value is never in the request's summary. |
| `changes.capture`, `changes.baseline` | Never | Baselines are cached under keys other computers share. |
| `changes.requests.markKept`, `changes.requests.open` | Never | |
| `computers.configuration.*`, `computers.identities.*`, `computers.launch`, `computers.shell`, `activity.setup.read`, `macos.template.delete` | Never | They read or change every computer at once. |
| `files.upload`, `files.download`, `logs.export`, `clipboard.transfer`, `computerUse.approval`, `display.*`, `desktop.connect`, `macos.display.*`, `editor.*`, `terminal.command`, `guest.sshAlias.*`, `network.open` | Never | They reach your device's own files, clipboard, screen, windows or settings. |
| `network.state`, `sshAccess.state`, `sshAccess.connection` | Never | They list every computer. |
| `secrets.list`, `secrets.remove`, `secrets.retry` | Never | A computer never reads a secret's value, and the list shows every computer's secrets. |
| `github.*` other than the configuration save | Never | Your GitHub account and sign-in. |
| `guestClient.*` | Never | Tokens are yours to give. |
| `settings.*`, `connections.*`, `exports.*`, `storage.*`, `updates.*`, `migration.*`, `preparation.*`, `recipes.*`, `engine.shutdown`, `engine.update.*`, `engine.credentials.*`, `device.dependencies` | Never | Engine settings, Connections, exports and imports, updates. |

## Asking you

A call that needs your approval answers at once with the error `approval_required`. Its details
carry the kind (`merge`, `replace_main`, `github`, `secret`, `network`, `port`, `ssh`, `access`,
`container`), a summary the engine wrote from the call's parameters, never from the agent's
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

## The audit log

`<app data>/audit.sqlite3` holds one row for every call that can change something (a command, a
job or a stream) and for every refused or held call: when, who (owner, device with its key
fingerprint when known, or computer with its id), the name the program gave, the method, the
computer it named, the outcome (`ok`, `error`, `refused`, `asked`), the error code and the job id.
Parameters and results are never stored. Rows older than 90 days, and the oldest beyond 200,000,
are trimmed. `audit.list` reads it, newest first.

A change another device makes through one of the Connections bridge's own methods (`runtime.*`,
`macos.*` and the other names in `remote::METHODS`) is recorded there too, with the device's key.

## Other limits

- When a computer is deleted, what it made becomes its maker's, so an agent keeps reaching the
  computers it could reach before.
- A replace main (promote) ends the promoted fork's standing as a computer its maker made: it now
  holds main's place.
- The in-guest bridge also caps how many computers one computer holds (32), runs (3) and how many
  of its jobs are unacknowledged (16).
- The network policy is the same for every computer today, so a fork's inherited policy hands
  nothing over. When computers get policies of their own, a fork of a computer with a wider
  policy than its maker's must ask.

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

## Method by method

| Methods | Rule | Notes |
| --- | --- | --- |
| `engine.handshake`, `engine.state`, `engine.disconnect`, `events.unsubscribe` | Free | |
| `device.facts` | Free | The device's CPUs, memory and whether Remote Login is on; nothing about other computers. |
| `audit.list` | Free | Only rows about the computer and the computers it made: calls that name one of them, and calls one of them made. |
| `operations.job.get`, `.acknowledge`, `operations.cancel`, `guest.exec.get`, `guest.exec.input` | Its own jobs | |
| `operations.list`, `events.subscribe`, `notifications.list` | Never | They cover every computer and are not cut per caller yet. |
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
| `github.configuration.save` | Asks you | Only when the computers whose GitHub access changes are in its tree; a configuration that drops or changes another computer's access is refused. |
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

A call that needs your approval answers at once with the error `approval_required`; its details
carry the kind (`merge`, `replace_main`, `github`, `secret`, `network`, `port`, `ssh`, `access`,
`container`) and a summary the engine wrote from the call's parameters, never from the agent's
words. Approvals are not stored yet, so for now such a call simply does not happen.

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

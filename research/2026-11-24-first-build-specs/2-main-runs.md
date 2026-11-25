# Step 2: main runs

Part of the build specs in this folder ([README](README.md)). It builds
`computers-main-on-a-mac`, `engine-matches-wanted`, `agent-programs-run-unmodified`,
`agent-programs-reach-every-computer` and `computers-stopped-and-paused` (#348). It needs step 1.
The formats are in [formats.md](formats.md).

## What you can do when it is done

```
$ silo setup
This Mac: Apple M4 Pro, macOS 27.2, 612 GB free. Main needs about 110 GB.

Main's network. Open reaches the internet, and also your local network and any service on
this Mac that listens on every address. Nothing an agent sends is checked, held or recorded
yet. Open it? [y/N] y

Main will get, each from its vendor's own installer:
  Command Line Tools, Homebrew, tmux, Claude Code, Codex
Remove any? (enter to keep all)

To reach main by name, Silo adds this line at the top of ~/.ssh/config:
  Include "~/Library/Application Support/Silo/ssh/config"
Add it? [y/N] y

Downloading macOS 27.2 (25.6 GB)    ████████░░░░░░  58%   about 6 min left
...
Main is ready. `ssh main` opens a shell, `ssh main.keep` a tmux session that outlives the terminal.

$ ssh main.keep
polarzero@main ~ % claude --dangerously-skip-permissions
```

Then, from your Mac:

```
$ silo status
Engine    running, history 5b1e…, event 214
Recorder  running, holds every event up to 214
main      running since 21:14   network open, not checked   way in: ssh main, ssh main.keep
```

## The pieces

| Piece | Where | What it does |
|---|---|---|
| The core's computer rows | `core/computers.zig`, `operations.zig`, `images.zig` | What is wanted and what is seen, for each computer and image; the steps of `setup` |
| The helper | `engine/macos/`, Swift, one process per computer | Holds the virtual machine. Starts, stops, pauses it. Opens SSH connections into it |
| The helper driver | `drivers/helper.zig` | Loads and unloads each helper's launchd job, connects to it, turns its messages into inputs |
| The image driver | `drivers/image.zig` | Runs `silo-helper fetch` and `silo-helper install` as child processes |
| The catalog driver | `drivers/catalog.zig` | Makes a computer's folder from the installed system, as #365's catalog does |
| The setup driver | `drivers/setup.zig` | Runs the steps of the first run inside main, over SSH with Silo's key |
| The SSH entries driver | `drivers/ssh_entries.zig` | Writes `<data>/ssh/config` and, with your yes, the `Include` line |

### The helper

One Swift program, `silo-helper`, signed with `com.apple.security.virtualization`, built with `-O`
and never `-Ounchecked` (`tigerstyle-by-place`, as #352 file 01 proposes). It has three commands,
taken from #359's `silovm`:

| Command | Started by | Does |
|---|---|---|
| `silo-helper run c1` | launchd, job `com.amontlabs.silo.helper.c1` | Builds the configuration from `computers/c1/live/`, listens on `run/helpers/c1.sock`, and waits for the engine's `start` |
| `silo-helper fetch` | the image driver | Asks `VZMacOSRestoreImage.latestSupported` for the URL and downloads it to `restore.ipsw.part`, resuming from its length. Prints progress as JSON lines |
| `silo-helper install` | the image driver | Installs the image into `images/<build>/installed/` with `VZMacOSInstaller`. Never starts the computer |

Each helper is a launchd job of its own, not a child of the engine. launchd ending the engine
then never ends a computer, which step 3 needs (`when-the-engine-stops`). The job has
`RunAtLoad false` and `KeepAlive false`: the engine decides when a computer runs, and after a
restart of the Mac the engine starts it again (below).

The configuration the helper builds holds no shared directory, no clipboard device and no USB
device: `directorySharingDevices`, `consoleDevices` beyond the serial log and `usbControllers` are
empty, and a test reads the configuration back to check it (`device-not-shared`,
`files-through-a-channel`).

The disk is `disk.img`, attached with `VZDiskImageStorageDeviceAttachment`, its caching and sync
modes set by #359's test 04: a clone of a running disk must be a moment in time for step 4. If the
NBD attachment wins step 4 (#365), only this line of the helper changes.

### The network

`network-stack` (open) says a computer's network goes through Silo's stack and never Apple's NAT.
`network-before-rules` (#348) says main's network is open, with your yes, before rules exist. #364
proposes: Apple's NAT until a patched gvproxy passes #359's test 07, and the first run says what
open means on it. This spec builds that:

| | Apple's NAT (first) | Patched gvproxy (once test 07 passes) |
|---|---|---|
| The helper attaches | `VZNATNetworkDeviceAttachment` | `VZFileHandleNetworkDeviceAttachment` to a gvproxy the helper starts, over a Unix datagram socket |
| Main reaches | the internet, your local network, your Mac's addresses on that network | the internet only: gvproxy's NAT table empty and its route to the Mac's `127.0.0.1` gone, its HTTP API removed (#358) |
| The way in | the helper connects to main's address, read from `/var/db/dhcpd_leases` by main's MAC address | gvproxy's own forward, on a Unix socket of the helper |
| Closed | not possible with the way in kept: a computer with no network device has no SSH. Setup offers open only, and says why | gvproxy with no outbound connection at all, SSH still in |
| What `silo status` says | `network open, not checked; reaches your local network` | `network open, not checked` |

`silo proxy` and the SSH entries do not change between the two, so your SSH settings stay the same
when the stack changes (#352 file 06).

### The way in

`<data>/ssh/config` holds two entries per computer:

```
Host main
  User polarzero
  IdentityFile "~/Library/Application Support/Silo/computers/c1/ssh/id_ed25519"
  IdentitiesOnly yes
  UserKnownHostsFile "~/Library/Application Support/Silo/computers/c1/ssh/known_hosts"
  StrictHostKeyChecking yes
  ProxyCommand "/usr/local/bin/silo" proxy c1
  ProxyUseFdpass yes
  ForwardAgent no
  ClearAllForwardings yes
  ServerAliveInterval 30
  ServerAliveCountMax 6

Host main.keep
  (the same lines)
  RequestTTY yes
  RemoteCommand /opt/homebrew/bin/tmux new-session -A -s main
```

`main.keep` is step 3's; it is written here with the others. The `Include` line goes first in
`~/.ssh/config`, as #352 file 06 found it must, after your yes. After writing, the driver runs
`ssh -G main` and checks that every line above is what ssh will use; a forwarding your own
`Host *` adds anyway is shown in `silo status` (#352 file 06).

`silo proxy c1` connects to the helper's socket, sends `{"connect":{"port":22}}`, receives the
connected socket and passes it to ssh. No port of your Mac listens.

### What the core holds for a computer

| Field | Values |
|---|---|
| `id`, `name` | `c1`, `main` |
| `system` | an opaque value the core can only compare (`neutral-core`) |
| `wanted` | `running`, `paused`, `stopped` |
| `seen` | `unknown`, `stopped`, `starting`, `running`, `paused`, `stopping`, `failed` with its reason; and the awake time it was seen |
| `boot` | how many times it was started; the key of each start is `c1/boot/<n>` |
| `helper` | `not_loaded`, `loading`, `connected`, `lost` |
| `place_refused` | whether Apple refused its last start for lack of a place |
| `operation` | the operation under way, if any: one at a time per computer (#352 file 03) |
| `retry` | the try count and the awake time of the next try |

`engine-reconciles`, as #352 file 03 proposes: while `wanted` and `seen` differ, the core issues
the effect that brings them closer, or says what it waits for. A try that fails is tried again
after 1, 2, 4, up to 60 seconds. `unknown` never counts as matching. An observation older than one
already applied changes nothing.

| Wanted | Seen | Effect |
|---|---|---|
| `running` | `stopped`, helper `not_loaded` | load the helper's job, then `start` |
| `running` | `stopped`, `place_refused` | none: wait, and say which computers Silo knows hold the two places |
| `running` | `paused` | `resume` |
| `stopped` | `running` or `paused` | `stop` with 30 seconds' grace |
| `paused` | `running` | `pause` |
| any | `unknown` | `observe` |

### The first run, as one operation

`silo setup` asks its three questions, then sends one `setup` request. The engine carries it out
as an operation of explicit steps, each safe to stop after (`multi-step-operations`). After a
crash, the engine finds the step from the operation's row and what the drivers say exists, and goes
on. `silo setup` run again follows the operation under way instead of starting another.

| Step | Effect | Kept when it stops | On failure |
|---|---|---|---|
| 1. Check the Mac | none; the request carries the facts `silo setup` read | | Not Apple silicon, or macOS before 27: `not_this_mac`, nothing done (`mac-silo-needs`). Too little disk: `disk_too_small` with how much is missing |
| 2. Reserve | write `reserve`, its size allocated on the disk (`F_PREALLOCATE`) | the file | |
| 3. Download | `silo-helper fetch`, with a power assertion held while it runs | `restore.ipsw.part` | resumes from its length |
| 4. Install | `silo-helper install` | `installed/` once complete, renamed into place | removed and done again |
| 5. Make main | the catalog driver clones `installed/` into `computers/c1/live.new/` with a new machine identifier, makes the SSH key and `helper.key`, then swaps it into `live/` | | `live.new/` removed at the next start |
| 6. First start | load the helper's job, `start` with `provision` | | Provisioning refused: the error is shown with Apple's code, and the fallback below |
| 7. SSH answers | `silo proxy` until the server answers, at most 10 minutes | | |
| 8. Secure the way in | over SSH with the provisioned password: add Silo's key to `authorized_keys`, turn password login off, give the user `sudo` with no password, turn sleep off inside main; pin the host key in Silo's `known_hosts` | | |
| 9. Install the list | one step per item, by its vendor's installer, over SSH with the key | each item done | the item is shown as failed with its output; you can run `silo setup` again to retry it |
| 10. Write the entries | the SSH entries driver | | |
| 11. Restore image | delete `restore.ipsw` once `installed/` is complete (`computer-image`) | | |

- **Provisioning** (`VZMacGuestProvisioningOptions`, macOS 27): your full name, your Mac's login
  name, a random password of 32 characters kept in the Keychain (service `Silo`, account `c1`),
  automatic login, and Remote Login on. A developer reported that provisioning fails when the user
  name is the Mac's own login name (FB23241619). If #359's test 01 shows it, step 6 provisions a
  user `silo-setup`, and step 8 creates your user with `sysadminctl` and deletes `silo-setup`
  (#352 file 10). The spec builds both; test 01 picks the default.
- **The key** Silo uses to reach main never changes for the life of main, so a restore in step 4
  can never bring back a wrong one (#352, "`what-a-restore-leaves`").
- **The list** is data, `drivers/setup_list.zon`: for each item its name, its vendor's documented
  install command, and the command that shows it is there. The first build follows each vendor's
  documentation as it reads on the day the list is written, and pins nothing:
  `claude-code-fetched-by-recipe` asks for a pinned version once recipes exist.
- **The power assertion** for the download and the install is `IOPMAssertionCreateWithName` with
  `kIOPMAssertPreventUserIdleSystemSleep`, held by the image driver's child and released when it
  ends (`first-run-time-macos`).

### Stop, pause, and a restart of the Mac

- **Stop** (`what-stop-ends`, as #352 file 05 proposes): the helper calls `requestStop()`, waits 30
  seconds, then `stop()`. The record says which ended it. A stop after which Apple refuses the next
  start for lack of a place sets `place_refused`, so a leaked place (FB22967193) shows as a fact,
  not a computer that never comes back.
- **Pause** (`pause-every-computer`): `pause()` and `resume()` on the virtual machine. A paused
  computer whose helper ends, because the Mac restarted or the helper crashed, comes back
  `stopped`, never running, and `silo status` says what was running is lost. `silo pause --all`
  pauses every running computer, and `silo resume --all` resumes those it paused, not one you
  paused by name.
- **A restart of the Mac.** Nothing of Silo runs until you log in (Apple's framework in your login
  session, #359 test 09). At login launchd starts the engine. It sees no helper, marks main
  `unknown`, observes `stopped`, and since main is wanted running, starts it. `silo status` then
  says `main running since 08:02, started again after the Mac restarted`.

### The record of step 2

Control facts, each with its author: `you` and your person for `setup`, `start`, `stop`, `pause`,
`resume` and each yes of the first run; `silo` for what reconciling did, with its cause
(`helper-ended`, `mac-restarted`, `place-refused`). Each setup step's start and end. The grant of
`way_in` and, with your yes, of `network` to main. A refused start, with Apple's error code.

## What happens on a crash

| What stops | What happens to main | What the engine does |
|---|---|---|
| The engine | Nothing, in steps 2 and 3: the helper is its own launchd job | At start it connects to `run/helpers/c1.sock`, checks the helper with the handshake, and asks its state |
| The helper | Main stops with it (`helper-on-broken-assumption`). What it wrote since its last flush may be lost | The socket closes. Main is seen `stopped`, with cause `helper-ended`. Wanted running, it is started again after the back-off |
| `silo-helper fetch` | | The step is retried; the download resumes |
| `silo-helper install` | | `installed/` is not renamed into place until complete; the step starts again |
| The Mac sleeps | Main is suspended with it | Timeouts do not run during sleep. Step 3 records it |

## Cost targets

| Measure | Target |
|---|---|
| `silo setup` with the image already downloaded, to `ssh main` | measured by #359 test 00 and set from it (`first-run-time-macos`) |
| `silo stop` of a busy main | 30 seconds at most, by construction |
| `silo start` of main, to SSH answering | measured by #359 test 12 |
| Memory of the helper, main idle | under 64 MB beyond the virtual machine's own |
| The helper's processor, main idle | under 1 percent of one core |

## Tests

| Test | Checks | Where |
|---|---|---|
| The simulator, with a fake helper, fake image and catalog drivers, crashes at every step of `setup`, Mac restarts, helpers dying, places refused | `crash-finishes-or-rolls-back`, `wanted-and-seen-agree` within the longest back-off plus the longest timeout, `no-effect-twice` | CI |
| Contract tests: the fake helper and `silo-helper` answer the same cases (`fakes-share-contract-tests`) | The fake is honest | CI for the fake; polarzero's Mac for the real helper |
| The configuration read back | No shared folder, clipboard or USB device | polarzero's Mac |
| `ssh -G main` after writing the entries, with a `Host *` that adds forwards | What ssh really uses | CI (no Mac needed) |
| #359 tests 00, 01, 10, 14 | The download and install, provisioning with your own name, a first start cut off, what provisioning leaves | polarzero's Mac, before step 2 |
| #359 test 02 | Claude Code and Codex stay signed in over SSH, after a reconnect and a restart of main | polarzero's Mac |
| #359 test 07 | gvproxy with a macOS computer against Apple's NAT: what of the Mac each reaches. Decides which network the first build ships | polarzero's Mac |
| #359 test 08 | Claude desktop and the Codex app reach main through the included entry with fd passing | polarzero's Mac |
| #359 test 09 | The engine and a helper as user agents start a computer; what happens logged out | polarzero's Mac |
| #359 test 11 | Documents, Desktop and Downloads in main over SSH: whether macOS asks a question nobody sees | polarzero's Mac |
| #359 tests 12, 13 | Twenty stops of a busy main within 30 s, no place leaked; ten fresh computers | polarzero's Mac |
| Hostile, as root in main | main cannot reach `run/engine.sock`, your Mac's `127.0.0.1`, gvproxy's API, or any file of the Mac outside the channels `silo status` lists. On Apple's NAT, what it reaches on your network is listed, and `silo status` says so | polarzero's Mac |
| #347's checks 6 to 10 against main's network | Byte for byte, the first byte of a stream, a slow request, gRPC, a restart of the stack. On Apple's NAT they measure the reference; on gvproxy they are its cost (`network-stack` asks that it be measured) | polarzero's Mac |

## Done when

- a Mac with nothing of Silo on it reaches `ssh main` with Claude Code and Codex installed, and you
  answered only the three questions of `silo setup`;
- a Mac on macOS 26 is refused before anything is downloaded;
- `silo setup` killed at any step and run again ends with the same main;
- `silo stop` ends main within 30 seconds and `silo start` brings it back;
- after a restart of the Mac and your login, main runs again, and the record says why;
- the hostile test passes, and `silo status` says what main's network reaches.

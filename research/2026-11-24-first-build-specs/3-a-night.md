# Step 3: a night

Part of the build specs in this folder ([README](README.md)). It builds
`agents-work-through-the-night` and `engine-outlived-by-computers` (#348). It needs step 2. Step 4,
the way back, does not need it and can be built beside it.

## What you can do when it is done

At 23:00:

```
$ ssh main.keep
polarzero@main ~ % cd ~/work/app && claude --dangerously-skip-permissions
  (a second tmux window: cd ~/work/api && codex --dangerously-bypass-approvals-and-sandbox)
```

You close the terminal. The tmux session in main goes on, and both agents with it. The MacBook is
on its charger with the lid open, so Silo holds it awake. At 02:10 the Wi-Fi drops for a minute; at
03:30 the engine crashes. Neither stops main.

At 08:00:

```
$ silo status
main      running since 21:14   network open, not checked
          the Mac stayed awake all night. The engine restarted once, at 03:30, and found main running

$ silo record main --since 23:00
23:00:12  way in       opened by ssh, from your Mac
03:30:02  silo         engine restarted; main found running, boot 4, nothing changed
08:00:31  way in       opened by ssh, from your Mac

$ ssh main.keep        # the two windows, as you left them
```

On a night when the Mac did sleep, the record says when and for how long, so a night with no work
shows why:

```
01:12:40  the Mac      went to sleep (lid closed, no external display)
07:58:02  the Mac      woke after 6 h 45 min
```

## The pieces

| Piece | Where | What it does |
|---|---|---|
| `main.keep` | the SSH entries of step 2 | `RemoteCommand /opt/homebrew/bin/tmux new-session -A -s main` attaches to main's tmux session or makes it (`runs-outlive-the-terminal`) |
| The power assertion | the helper | Holds the Mac awake while its computer runs and the Mac is on power (`mac-kept-awake`) |
| The power source | the helper | A source of lines for the recorder: the Mac's sleeps and wakes, power and lid, and each connection of the way in |
| The helper as its own job | step 2 | Main runs on when the engine stops (`engine-outlived-by-computers`) |
| Taking back helpers | `drivers/helper.zig` | A started engine finds each running helper and goes on with it (`engine-takes-back-helpers`) |

### Keeping the Mac awake

The helper of a running computer holds one assertion, `IOPMAssertionCreateWithName` with
`kIOPMAssertPreventUserIdleSystemSleep`, named `Silo: main is running`, so that `pmset -g
assertions` shows who holds the Mac awake. It holds it only while:

- its computer is `running` (not paused, not stopped), and
- the Mac is on power: `IOPSGetProvidingPowerSourceType` says AC. A change of power source is
  followed through `IOPSNotificationCreateRunLoopSource`.

On battery it releases the assertion and writes a line, and `silo status` says
`the Mac may sleep: it is on battery`. Holding the Mac awake on battery is the option
`mac-kept-awake` leaves out, because it drains the battery.

A closed MacBook sleeps whatever the assertion says, unless it is on power with an external display
(Apple's clamshell mode). Silo cannot change that, so it says so: `silo start`, `silo setup`'s last
line and `silo status` on a Mac with a battery and no external display print `If you close the lid,
the Mac sleeps and main with it.` The lid's state is read from `AppleClamshellState` in the power
management root domain, and the displays from `CGGetOnlineDisplayList`.

Whether Apple's framework already holds the Mac awake while a virtual machine runs is not known
(#348, `mac-kept-awake`'s notes). #359's test 16 runs an idle night; this step adds
`pmset -g assertions` to its facts, every hour.

### Sleeps and wakes in the record

The helper of each running computer is a source of the recorder (`run/recorder.sock`,
[formats.md](formats.md), "How a source sends its lines"). It registers with
`IORegisterForSystemPower` and writes:

| When | Line |
|---|---|
| `kIOMessageSystemWillSleep`, before it calls `IOAllowPowerChange` | `the Mac goes to sleep`, with the reason when one is known: the lid, the power button, idle on battery |
| `kIOMessageSystemHasPoweredOn` | `the Mac woke`, with how long it slept |
| The power source changes | `on battery` or `on power`, and whether the assertion is held |
| Its 60-second tick sees a sleep no message announced | `the Mac woke`, with how long it slept |

How long the Mac slept is measured, not guessed: `mach_continuous_time` counts sleep and
`mach_absolute_time` does not, so the difference of their changes since the last tick is the sleep.
That is why the wake line is always written, even when the sleep line was not (the Mac can sleep
before the helper gets to send it).

The helper also writes a `connection` line each time `silo proxy` asks it for the way in, with the
program that asked (`ssh`, by its path, read with `proc_pidpath` from the peer's process id). It
passes the socket on and keeps no copy, so it does not see the connection end.

Each line is written into the record of the helper's own computer, since what it records is that
this computer was suspended with the Mac. With two computers running, each record holds the sleep.

The recorder counts silence in awake time, as timeouts are (`how-effects-work`). A night when the
Mac slept is then one sleep line and one wake line, and no silence and no gap for any source.

Which author a sleep has is open: `who-made-each-change` names you, a rule, a precaution, an
extension and Silo, and none of them put the Mac to sleep. This spec writes `"author":{"by":"device"}`
and the README lists it as a choice.

### When the engine stops

The engine is a launchd job and each helper is another (step 2). When the engine stops, for any
reason:

- main keeps running, and its helper keeps its power assertion and its lines to the recorder;
- `ssh main` and `ssh main.keep` still work: `silo proxy` talks to the helper, not to the engine;
- `silo stop`, `silo start` and every other request fail with exit code 3 and
  `the engine is not running; main is running and nothing about it can change until the engine is
  back` (`when-the-engine-stops`);
- launchd starts the engine again after 10 seconds.

When the engine starts, it reads its state and finds main `wanted running`. Before it acts on
main, it connects to `run/helpers/c1.sock` and runs the handshake both ways
([formats.md](formats.md), "The helper's protocol"). The helper answers with its state and its
`boot`. When that matches the boot the engine last saw, nothing is done: the record gets one line,
`engine restarted; main found running`. When the helper is not there, or answers with another boot,
main is seen as it is and reconciled as step 2 says.

A helper whose handshake fails is not this computer's helper. The engine leaves it alone, marks
main `unknown`, and says so in `silo status`; it never starts a second virtual machine on a disk
that may still be in use.

### When the recorder stops

The helper keeps its unacknowledged lines in a ring of 4,096 and sends them again once the
recorder is back; a full ring loses the oldest and says how many in its next line, which the
recorder writes as a gap with the count (`losses-show`).

## What happens on a crash

| What stops | When | What happens |
|---|---|---|
| The engine | any time in the night | Main runs on, the Mac stays awake; the engine is back within about 10 seconds and finds main |
| The recorder | | Lines wait in each source; the engine's events wait in its ring |
| The helper | | Main stops with it, as in step 2. The engine starts it again, since it is wanted running: main boots again and the agents in it are gone; the record says `helper-ended`, then the new boot |
| Your Mac's connection | the Wi-Fi drops | The SSH session ends after 3 minutes without an answer (`ServerAliveInterval 30`, `ServerAliveCountMax 6`); tmux in main does not notice |
| The Mac's power | a power cut | Main is lost from its last flush, like any machine; after the restart and your login, main boots again |

## Tests

| Test | Checks | Where |
|---|---|---|
| The simulator, with the engine killed while a fake helper runs main, and helpers that answer with the same boot, another boot, a wrong key or nothing | Main is never started twice on one disk; `computers-outlive-the-engine` | CI |
| The helper's clock arithmetic on recorded pairs of `mach_continuous_time` and `mach_absolute_time` | The sleep's length, and a sleep with no message | CI (pure arithmetic) |
| An agent started in `main.keep`, the terminal closed, the Wi-Fi off for a minute, `ssh main.keep` an hour later | The run went on and the session is found again | polarzero's Mac |
| The same, with `kill -9` of the engine in the middle, and of the recorder | Main runs on; the record holds every line, with no gap | polarzero's Mac |
| #359 test 02, run inside `ssh main.keep` as well as plain `ssh main` | Claude Code and Codex keep their sign-in in a tmux session over SSH. tmux runs apart from the SSH session, and how macOS gives it the Keychain is not tested | polarzero's Mac |
| #359 test 16, with `pmset -g assertions` every hour | On power with the lid open, an idle night passes with no sleep, and what macOS writes by itself | polarzero's Mac, one night |
| A sleep forced with `pmset sleepnow`, then a wake | The two lines, the length of the sleep, no silence written | polarzero's Mac |
| On battery, then back on power | The assertion released and held again, and the lines | polarzero's Mac |
| A closed lid with no external display | Silo said so before; the record holds the sleep and its reason | polarzero's Mac |

The boundary's checks of #347 do not apply: nothing in this step reads main's network.

## Done when

- an agent started in `main.keep` is still at work after the terminal is closed and the Wi-Fi
  drops for a minute;
- on power with the lid open, an idle night passes with no sleep, and `pmset -g assertions` shows
  Silo's assertion while main runs and none once it stops;
- killing the engine during the night leaves main running, and the engine finds it again when it
  starts, with one line in the record;
- every sleep and wake of the Mac is in main's record, with its length.

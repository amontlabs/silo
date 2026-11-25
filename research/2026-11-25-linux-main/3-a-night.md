# Step 3 under B: a night

Part of option B's specs ([README](README.md)). #370's `3-a-night.md` holds under B as written:
`ssh main.keep`, the Mac kept awake on power, its sleeps and wakes in the record, main running on
while the engine restarts, and the engine taking back its helpers. It builds
`agents-work-through-the-night` and `engine-outlived-by-computers`. The differences are small and
all in the helper.

| | #370 (macOS main) | Under B |
|---|---|---|
| `main.keep`'s command | `/opt/homebrew/bin/tmux new-session -A -s main` | `/usr/bin/tmux new-session -A -s main` |
| The power assertion, the sleep and wake lines | Swift, IOKit | The same IOKit calls from Zig through their C headers: `IOPMAssertionCreateWithName`, `IORegisterForSystemPower`, `IOPSNotificationCreateRunLoopSource`. The helper runs a CFRunLoop on a thread of its own for them |
| Sleep measured by | `mach_continuous_time` against `mach_absolute_time` | The same |
| Main's clock after the Mac sleeps | macOS in main sets its clock again | Not known: main's Linux clock may lag by the length of the sleep until `systemd-timesyncd` corrects it through the boundary. The helper's wake line says how long the Mac slept, so the record never depends on main's clock |
| The helper taking back a computer | Handshake on `run/helpers/c1.sock` | The same; the `boot` the helper reports is the count of `krun_vmm_run` calls since `live/` was made, kept in `computers/c1/boot` |
| What `silo status` says about the lid | Read from `AppleClamshellState` | The same |

## One more test

| Test | Checks | Where |
|---|---|---|
| A forced sleep (`pmset sleepnow`) of 10 minutes with main running, then a wake | Main's clock is correct within 2 s, 60 s after the wake; the two lines in the record | polarzero's Mac |

Every other test of #370's step 3 runs unchanged, with #359's test 02 in its Linux form.

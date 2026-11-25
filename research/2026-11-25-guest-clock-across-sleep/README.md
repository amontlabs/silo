# The guest's clock across a sleep of the Mac

25 November 2026. From #378 (finding 2: a guest an hour and a half behind after a night) and #390
(test 37). Linux prototype: a QEMU guest paused and resumed the way a sleep pauses main, the
things that break at each offset, and a fix shown failing first.

## What was found

1. **Main falls behind by the whole sleep, and does not catch up.** A guest paused for 10 s,
   30 s and 120 s was behind the host by 10.001 s, 29.997 s and 120.000 s one second after the
   wake, and by the same a minute later. Awake for a minute with no pause, it drifted by less
   than a millisecond. Nothing in the guest corrects it on its own.
   ([`results/measure.json`](results/measure.json))
2. **The guest's hardware clock helps only if it ran during the sleep.** With QEMU's RTC on the
   host's clock, the RTC was right after the wake (within its 1 s resolution), so a guest that
   re-read it would be fixed. With the RTC on the guest's own clock, the RTC fell behind too, by
   10.7 s, 40.7 s and 160.7 s (the sum of the sleeps). Which one Apple's framework gives a macOS
   main is not known: a forum answer names `AppleVirtualPlatformRTCPlugin`, which sets the time in
   a macOS guest, and nothing says what it does after a sleep. Test 37 of #390 measures it
   (`offset_wake` and `offset_60`).
3. **A few seconds behind already breaks things.** At each offset, with the real tools where
   there are some ([`results/breaks.json`](results/breaks.json)):

   | In main | 5 s behind | 60 s | 15 min | 90 min |
   |---|---|---|---|---|
   | HTTPS through the boundary, leaf dated now | not yet valid | not yet valid | not yet valid | not yet valid |
   | HTTPS through the boundary, leaf dated a day back | ok | ok | ok | ok |
   | `git commit -S` with a GPG key made on the Mac today | key "created in the future" | refused | refused | refused |
   | TOTP code made in main (server allows one step either side) | ok | refused | refused | refused |
   | GitHub App JWT made in main (`exp` 10 min ahead) | ok | ok | refused | refused |
   | AWS SigV4 request signed in main (15 min skew allowed) | ok | ok | ok | refused |
   | `make` after main edits a source of a target the Mac wrote | stale | stale | stale | stale |

   Every file main writes, every commit date and every log line is behind by the same amount.
   What the record writes from the Mac's side (#370: the sleep as a `device` fact, measured with
   `mach_continuous_time`) is right; anything it takes from inside main is not.
4. **The fix: the helper sets main's clock on a wake and every minute.** The test
   ([`host/test_clock.py`](host/test_clock.py)) sleeps the guest 20 s and wants main within 1 s
   of the host 2 s and 65 s after the wake ([`results/test_clock.txt`](results/test_clock.txt)):

   | Run | 2 s after the wake | 65 s after | Verdict |
   |---|---|---|---|
   | no fix | -19.999 s | -19.998 s | fails |
   | check every 60 s, no wake signal | -19.999 s | -0.000 s | fails |
   | wake signal and check every 60 s | +0.000 s | +0.001 s | passes |

   The check alone leaves main wrong for up to a minute after every wake; the wake signal closes
   that. The check every minute stays as the backstop for a missed wake, and for an agent with
   `sudo` that changes main's clock.
5. **Dating the boundary's certificates a day back fixes HTTPS and nothing else.** Worth doing
   anyway, since it covers the second or two between the wake and the step. Inspecting proxies
   do the same (mitmproxy dates its leaves back; how far, not checked against its current
   release).

## What it changes in the plan

- New open decision **`main-clock-kept`** (computers, after `mac-kept-awake`), with its choice:
  the helper sets main's clock on a wake and every minute, over its own way in (#370 step 2: SSH
  with Silo's key and `sudo`), writes each step in the record, and the boundary dates its leaves a
  day back. The other options (NTP answered by the boundary, leaving it to macOS, backdating
  only) are written there with their costs.
- **`mac-kept-awake`**: a note that holding the Mac awake does not cover a closed lid or battery,
  and points to `main-clock-kept`.
- **`who-made-each-change`**: a step of main's clock is a control fact with author `silo`, beside
  the sleep itself with author `device` (said in `main-clock-kept`'s notes).
- `deterministic-core` already notes that a monotonic clock stops while the Mac sleeps and that
  which clock a hold's limit uses is still to name. Nothing here settles it; it is the same
  question on the Mac's side.

## How the model holds

On a Mac, the guest's counter runs on `mach_absolute_time`, which stops while the Mac sleeps
(QEMU's HVF backend has the same issue, qemu#864), while the Mac's wall clock goes on. QMP `stop`
freezes QEMU's virtual clock the same way while the host's goes on. The guest is TCG (no KVM in
the container); the pause's effect is QEMU's virtual clock, not the accelerator's.

## What was not verified

- Nothing ran on a Mac. Whether macOS in main sets its own clock after a wake is test 37's to
  measure; if it does within seconds, the helper's step finds nothing to do.
- On the Mac the step is `sudo date -u MMDDhhmmCCYY.ss` over SSH: whole seconds, so main is
  within a second, not a millisecond as here. Not run.
- macOS's own time service in main (`timed`) was not looked at: whether it fights a step, or
  steps on its own when it gets network time through the boundary.
- The GitHub, TOTP and AWS rows apply the rule each provider documents; no request was sent.
- The `make` row needs a file whose time comes from the Mac (a put-back with the Mac's times, a
  shared folder). Files main writes itself are consistent with each other.

## Run it

```
sudo apt-get install qemu-system-x86 busybox-static cpio   # and gcc
sh host/build.sh                       # kernel from Ubuntu's archive, initramfs with clockagent
python3 host/measure.py                # results/measure.json, about 8 minutes
python3 host/breaks.py 0 5 60 900 5400 # results/breaks.json
python3 host/test_clock.py none        # fails
python3 host/test_clock.py keeper      # passes
```

`host/vm.py` drives the guest; `host/keeper.py` is the fix; `guest/clockagent.c` answers the
host's reads and sets the guest's clock, a stand-in for the helper's SSH command.

# Keeping main's clock right across a sleep of the Mac

## The choice

An agent's part, 25 November 2026, from
[the guest clock across a sleep](../../../research/2026-11-25-guest-clock-across-sleep/README.md).
You leave a MacBook on battery at 23:00 with two agents in main. The Mac sleeps from 01:00 to
02:30 and wakes on power. Main's clocks stopped with the Mac, so at 02:30 main thinks it is
01:00. It stays 90 minutes behind until something sets its clock: the prototype measured no
catching up at all, a minute after a wake. From then on, in main:

- every HTTPS connection through the boundary fails, because the certificate the boundary signed
  at 02:30 is "not yet valid" at 01:00;
- `git commit -S` fails, because gpg refuses a key made on the Mac today as "created 5400 seconds
  in the future";
- a TOTP code, a GitHub App token and an AWS request signed in main are refused by the server;
- every file main writes carries a time 90 minutes early.

How far behind main falls is the length of the sleep, to the millisecond in the prototype (10 s,
30 s and 120 s sleeps). Even 5 s already breaks the certificates and gpg.

The options:

- The helper sets main's clock, when the Mac wakes and every minute (recommended). The helper
  already has a way into main with Silo's key and `sudo` (#370, step 2). On a wake it runs
  `sudo date` there. Every minute it reads main's clock and sets it when it is more than a second
  off. It costs one command in main a minute, and a clock that jumps forward once after each
  sleep. An agent with `sudo` in main can still change main's clock; the helper puts it back
  within a minute.
- The boundary answers NTP and main's own time service sets the clock. It costs a UDP path from
  main that the closed network (#378) does not have, and it waits on macOS's time service, which
  checks on its own schedule.
- Leave it to macOS in main. Apple's framework has a clock plugin for macOS guests, and it may set
  main's clock after a wake. No source says what it does after a sleep; test 37 of the Mac kit
  (#390) measures it. It costs nothing if it works.
- Only date the boundary's certificates back. It fixes HTTPS and nothing else in the list above.

Recommended: the helper sets main's clock on a wake and every minute, and the boundary dates its
certificates a day back as well. The wake matters: with only the check every minute, main stayed
20 s behind for up to a minute after a 20 s sleep, and failed the test. If test 37 shows macOS in
main sets its own clock within seconds, the helper's step finds nothing to do and costs nothing.

## Notes

Agent, 25 November 2026. #378 found the case in a tool that runs agents in virtual machines: a
guest an hour and a half behind after a night. That tool sets the guest's clock when it is ready,
every 5 minutes, and on a wake. #382 lists it as gap 5. The note of the integration on
`mac-kept-awake` recommends the same fix; this entry is that fix, measured.

The wake is seen on the Mac by `NSWorkspace.didWakeNotification`, or by the growth of
`mach_continuous_time` over `mach_absolute_time`, the pair #370 already reads to write a sleep in
the record. A step of main's clock is a control fact with author `silo`.

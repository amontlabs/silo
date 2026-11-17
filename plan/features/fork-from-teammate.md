## Notes

- Owner, 2026-11-17, about what the home page shows: a computer "can be shared from linux server yes but also from a teammate". The owner asked for this entry the same day.
- The other way to hand a computer over is `export`: one file, the disk without memory, Linux only.

## Open points

- How it works. `remote-computers` has `fork --on <device>`, but only between devices one person connected with `connect-device`, which lets that Silo manage the whole device. A teammate's device is not that, so reusing it is a guess.
- How a teammate grants it: per computer, per checkpoint, or through the team's shared machine (`team-machine`).
- What travels with the fork. Two rules pull apart: `fork-access` lets a parent hand its secrets to a fork, and `export` sends secrets as names only and leaves logins out. Which one applies between two people is not decided.
- macOS. `export` says a macOS share goes as a recipe plus changes, because of Apple's licence. Whether a teammate's macOS computer can be forked at all is open.
- Memory. `export` says a memory snapshot only restores on the same CPU and runtime. Unless a teammate's device matches both, a fork onto it would cold-boot and running programs would restart.

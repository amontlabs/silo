## Proposed, not confirmed by the owner

### For the owner to settle

- What travels with a teammate's fork: the running state (memory, programs, the agent's conversation, the browser) and the diff, or only the disk and the diff?
  - Recommendation: The disk, the diff and the conversation always, and memory only between devices with the same CPU and runtime, where it can restore.
- How does a teammate grant a fork: a link to one checkpoint or computer, read-only or with write permission, or through the team machine?
  - Recommendation: A link to a checkpoint, as the home page's flow shows it, so two people need no team machine.
- Can you merge a teammate's fork into your own main, which it did not come from, or only back into theirs?
  - Recommendation: Only back into theirs at first, since merge brings a fork's changes into the computer it came from.

## Notes

- Owner, 2026-11-17, about what the home page shows: a computer "can be shared from linux server yes but also from a teammate". The owner asked for this entry the same day.
- The other way to hand a computer over is `export`: one file, the disk without memory, Linux only.

## Open points

- How it works. `remote-computers` has `fork --on <device>`, but only between devices one person connected with `connect-device`, which lets that Silo manage the whole device. A teammate's device is not that, so reusing it is a guess.
- How a teammate grants it: per computer, per checkpoint, or through the team's shared machine (`team-machine`).
- What travels with the fork. Two rules pull apart: `fork-access` lets a parent hand its secrets to a fork, and `export` sends secrets as names only and leaves logins out. Which one applies between two people is not decided.
- macOS. `export` says a macOS share goes as a recipe plus changes, because of Apple's licence. Whether a teammate's macOS computer can be forked at all is open.
- Memory. `export` says a memory snapshot only restores on the same CPU and runtime. Unless a teammate's device matches both, a fork onto it would cold-boot and running programs would restart.

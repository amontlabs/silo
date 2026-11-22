# What ships on macOS if reports from inside are not ready there

## For the owner

An agent's part, 22 November 2026. The first version runs main as a macOS computer on your Mac, and
an agent runs `npm install` there. Whether Silo can see from inside macOS which program did what is
still to find out, because Apple may require a permission Silo does not have yet.

The options:

- Ship with what Silo sees from outside (recommended). The record shows the hosts main reached, the
  model calls, the spend and the checkpoint before, and says that programs inside are not reported
  on macOS. It costs detail: you do not see which program opened each connection or wrote each file,
  and stopping one agent's run or seeing what each run wrote waits on macOS.
- Wait until reports from inside work on macOS. It costs the first version itself, which waits on a
  permission Apple may grant late or never.

Recommended: ship with what Silo sees from outside. Main runs macOS first, so waiting would hold
back everything, and what crosses the boundary is the part an agent cannot fake.

## Notes

Whether macOS computers can give processes and file events with no clicks is still to find out:
`processes-from-inside-macos`, in `record`.

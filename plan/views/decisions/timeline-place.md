# Timeline

## For the owner

An agent's part, 22 November 2026. At 13:42 claude #2 ran "clean the build" in main and freed 12 GB.
At 14:10 you find that a file you needed is gone, while two other agents kept working in main.

The options:

- One line for each computer (recommended). Main's line shows the checkpoints, every run's turns and
  commands, and the asks. You go to 13:42, see the three commands, and fork from the checkpoint just
  before. It costs a busy line when several runs overlap; each mark names its run, and quiet
  stretches collapse.
- One line for each run. claude #2 has a line of its own. It costs a true picture of going back: a
  checkpoint holds the whole computer, so going back from one run's line also takes back the other
  two agents' work, and that view hides it.

Recommended: one line for each computer. A checkpoint holds the whole computer, so the line you go
back from has to show everything a restore would undo.

## Notes

The sketch shows content, not design.

```text
 13:00 ──●─────○──────◆────────●──▶ now      ● checkpoint  ○ turn  ◆ selected moment
 ◆ 13:42  claude #2 · "clean the build" · 3 commands · −12 GB
          [fork from here] [restore to before] [put back paths]
```

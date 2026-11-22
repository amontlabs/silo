# Sources

## For the owner

An agent's part, 22 November 2026. Disk shows "target/ grew by 3 GB, written by cargo build". The
first half was seen on the disk from outside main. The second was told by the part of Silo that runs
inside main.

The options:

- Seen or told at a glance, the exact source on opening (recommended). The two halves look
  different, and opening the line says "seen on the disk" and "told from inside main". It costs one
  step to reach the exact source.
- The source written on every line. Each line carries its source in words. It costs a screen full of
  labels, when people ask for less to look at.

Recommended: at a glance, the exact source on opening. Whether Silo saw it or was told it is what
you need every time, and which part saw it only when something looks wrong.

## Notes

It needs `sources-number-their-lines`, which sets the word "line".

| Source | What it sees | What it writes |
|---|---|---|
| engine | Every call that changed something, every decision, checkpoint, ask and answer | Facts |
| boundary: network | Every connection in and out, and the content of traffic it opens | Facts |
| boundary: disk | What changed on the disk between two moments (how is to find out: `file-changes-from-outside`) | Facts |
| boundary: screen | The computer's display and the input sent to it | Facts |
| inside | Processes and which process touched which file, told by Silo's probe, a small part of Silo that runs inside the computer; and an agent program's own account of itself | Reports |

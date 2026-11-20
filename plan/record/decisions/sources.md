# Sources

It needs `sources-number-their-lines`, which sets the word "line".

| Source | What it sees | What it writes |
|---|---|---|
| engine | Every call that changed something, every decision, checkpoint, ask and answer | Facts |
| boundary: network | Every connection in and out, and the content of traffic it opens | Facts |
| boundary: disk | What changed on the disk between two moments (how is to find out: `file-changes-from-outside`) | Facts |
| boundary: screen | The computer's display and the input sent to it | Facts |
| inside | Processes and which process touched which file, told by Silo's probe, a small part of Silo that runs inside the computer; and an agent program's own account of itself | Reports |

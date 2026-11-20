# The first screen

"The four places below" are Now, Disk, Network and Timeline: `now-place`, `disk-place`,
`network-place` and `timeline-place`.

The sketch shows content, not design. How a screen is built is governed by the repository's
interface rules.

```text
 main        ● 3 agents     NEEDS YOU
  └ try-bun  ○ fork, idle    codex      asks to reach api.stripe.com            [allow] [deny]
 client-a    ● 1 agent       claude #2  nothing for 38 min, it runs `sleep 600`  [open]
                             disk       +41 GB today, six copies of target/      [purge]
                             network    new host paste.rs, 2.1 MB sent by codex  [deny this host]
                            DONE
                             claude #1  1 h 12 · $6.40 · 38 files · tests passed [receipt]
 [Checkpoint] [Fork] [Pause all]                     Now · Disk · Network · Timeline
```

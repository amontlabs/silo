---
"silo-ui": minor
---

Rebuild a Linux computer on the newest guest image with `silo rebuild <computer>`, keeping its workspace storage, home folder and SSH identity. Recipes are replayed, and the previous computer is kept as `<name>-undo` (stopped). A computer already on the newest image is refused unless you add `--force`. Nothing else can start, stop or change the computer while it is rebuilt, and an interrupted rebuild is finished or rolled back the next time Silo starts. The computer's own processes are paused for the short time its home folder is copied, so nothing written meanwhile is lost. A rebuild shows the step it is on, can be cancelled until the swap begins, and stops at its next step when you quit Silo. If Silo cannot settle an interrupted rebuild by itself, it tells you, and `silo rebuild <computer> --abandon` puts the old computer back.

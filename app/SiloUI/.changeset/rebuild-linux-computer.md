---
"silo-ui": minor
---

Rebuild a Linux computer on the newest guest image with `silo rebuild <computer>`, keeping its workspace storage, home folder and SSH identity. Recipes are replayed, and the previous computer is kept as `<name>-undo` (stopped). A computer already on the newest image is refused unless you add `--force`. Nothing else can start, stop or change the computer while it is rebuilt, and an interrupted rebuild is finished or rolled back the next time Silo starts.

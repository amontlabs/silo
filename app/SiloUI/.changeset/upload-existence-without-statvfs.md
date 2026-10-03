---
"silo-ui": patch
---

Fix uploads to computers whose file server cannot report free space, which reported a failure for a file that had been stored (and left duplicates when keeping both), and stop Copy from returning an old image when the computer's earlier clipboard answer arrives after the copied text.

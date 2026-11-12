---
"silo-ui": patch
---

An export made without logins now ends the Silo tools login of the computer it was made from even if that computer was renamed and another one took its name while the export ran. If Silo cannot delete the unfinished export file after a failed export, it says so, keeps trying when you reopen it, and does not report the export as settled until the file is gone. A computer you can undo a rebuild to keeps a closed network policy you gave it; only the temporary closed policy Silo uses while it cannot read the old rules is replaced.

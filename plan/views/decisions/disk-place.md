# Disk

## For the owner

An agent's part, 22 November 2026. Your Mac is down to 20 GB free. In main, six copies of target/
take 78 GB, written by claude #2 and codex. Nine worktrees take 31 GB, and the runs that made them
ended 9 days ago. Since 14:00, 38 code files and ~/.ssh/authorized_keys changed, and two commits
were never pushed.

The options:

- Space, who wrote it, what changed and what is only here, in one place (recommended). You purge the
  78 GB, since a rebuild brings it back, and you see the two unpushed commits before you drop
  anything. It costs some lines that say "unattributed", and a name that is Silo's opinion can be
  wrong.
- Space only, as a disk analyser shows it. You see the biggest folders. It costs what you need to
  know before you delete: who made it, whether a rebuild brings it back, and what would be lost.

Recommended: the first. Agents fill a disk fast, and before you delete you need to know what would
come back.

## Notes

The sketch shows content, not design.

```text
 78 GB  ~/work/app/target ×6   builds      claude #2, codex   today    [purge 65 GB]
 52 GB  ~/.codex/sessions      agent data  codex              +3 GB a day
 31 GB  ~/work/app-wt-* (9)    worktrees   no live run        9 days   [purge]
 Changed since 14:00   code 38 files · packages +3 · services +1 · ~/.ssh/authorized_keys ⚠
 Only here             2 unpushed commits · .env · CLAUDE.md (edited)
 Produced              results.html · 4 screenshots · run.mp4
```

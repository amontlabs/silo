# `msb rename`: rename a stopped sandbox

## Summary

A sandbox cannot be renamed. Its name is the catalog key and also names its directory, so anything
that wants to give a sandbox another name has to create a new one and copy the state, which changes
its identity and drops its snapshot ancestry. This adds `msb rename <name> <new-name>` (and
`Sandbox::rename_stopped`) for a created, stopped or crashed sandbox.

## What changed

- The catalog row, the sandbox directory `sandboxes/<name>` and the `name` stored in its saved
  configuration are renamed together. Everything else is keyed by the numeric sandbox id (runs,
  labels, root-disk record, the snapshot ancestry cursor inside the directory) or is a relative path
  inside the directory, so it follows.
- `runtime/root-disk.json` records the absolute path of each root-disk layer, and a start refuses a
  journal whose layer is not the configured one (`root-disk journal does not match the configured
  base layer`). The paths below the old directory are rewritten in the JSON state files of the
  sandbox's `runtime/` folder, and in the saved configuration.
- The rename is refused while the sandbox is running or paused, when the new name belongs to
  another sandbox or already has a directory, and when the new name is not valid for a sandbox.
- It takes the transition, snapshot-ancestry and lifecycle locks of both names, in name order so
  two opposite renames cannot wait on each other.
- It survives a crash. A journal (`<home>/rename-journal/<sandbox id>.json`, synced) records the
  complete new configuration before the directory moves. A process that stops in between leaves the
  journal, and the next rename or start of either name finishes it (or abandons it when the new name
  was taken before the directory moved).
- Snapshot groups are labels chosen when a capture is taken, so existing groups keep their name; a
  capture taken after the rename defaults its group to the new name.

## Why

Swapping which of two sandboxes a name refers to (for example promoting a fork to the long-lived
sandbox of the same name) needs a rename that keeps the sandbox, its disks, its snapshots and its
ancestry. A copy would give the new sandbox a new identity and leave the snapshots behind.

## Test plan

- Unit tests: row, directory, stored name and root-disk paths move for created, stopped and crashed
  sandboxes; running, starting, draining and paused are refused with nothing changed; an existing
  name (catalog or directory), the same name and a missing sandbox are refused; two names swap
  through a third; an interrupted rename finishes from each crash point (journal only, directory
  moved, row updated); a rename whose new name was taken is abandoned; unrelated journals are
  left alone.
- Live run on a throwaway home (macOS 0.7.6): two sandboxes from one image with a marker each, a
  disk snapshot of one, a refused rename of a running sandbox, a swap through a third name, a start
  of each (each keeps its own marker), a restore from the snapshot taken before the rename, and a
  capture from the renamed sandbox.

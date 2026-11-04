# Remove a capture's layer directory once its snapshot is published

## Summary

A full snapshot leaves its per-capture directory `runtime/checkpoints/<id>/` behind. The directory
holds hard links to the captured disk layers, so the layers (and the disk blocks behind them) stay
allocated for as long as the sandbox exists, even after `modify --compact` and removing every old
snapshot.

## What changed

- `snapshot create --full` (installed snapshot and `-o` archive) removes the capture directory
  after the snapshot or archive is published. If publication fails, the directory is removed too,
  unless the source also failed to resume: then the recovery error still points at the directory.
- When a sandbox runtime starts it removes the directories an earlier process left behind, which
  reclaims captures of a crashed runtime or client. This runs before the runtime accepts captures.
- Disk-only snapshots already removed their directory; they are unchanged.

Nothing else reads these directories: restore and export read the snapshot, and memory objects
come from `runtime/checkpoint-store`.

## Why

Measured on macOS 0.7.6: five full captures of one sandbox leave five `runtime/checkpoints/<id>/`
directories, each with hard links to the root and `/workspace` layers. After this change there are
none, and (together with the compaction cleanup fix) removing the old snapshots returns their disk.

## Test plan

- Unit tests: an installed snapshot, a failed installed publication and an archive each leave no
  capture directory; a capture with a failed source resume keeps it; a runtime start removes stale
  directories and drops the layer link count.
- Live run on a throwaway home: five captures leave no directories, snapshots restore, and the
  restored sandbox has the data written before the capture.

## Note

A related idea, linking a sealed layer instead of cloning it when its header already names the
published predecessor, was tried and dropped: no live layer header ever matches, and on APFS all
50 traced copies were `clonefile` clones.

A garbage collector for `runtime/checkpoint-store` was prototyped and dropped: the next
incremental capture links every object of the previous manifest, which still points into old packs,
so it removed only device-state objects (0.1 to 0.3 MiB per capture).

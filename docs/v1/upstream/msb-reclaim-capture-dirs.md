# Remove a capture's layer directory once its snapshot is published

## Summary

A full snapshot leaves its per-capture directory `runtime/checkpoints/<id>/` behind. The directory
holds hard links to the captured disk layers, so the layers (and the disk blocks behind them) stay
allocated for as long as the sandbox exists, even after `modify --compact` and removing every old
snapshot.

## What changed

- `snapshot create --full` (installed snapshot and `-o` archive) takes a lock file inside the
  capture directory and removes the directory after the snapshot or archive is published. It also
  removes it when validating or publishing the capture fails. It only adopts the directory its own
  capture id names, never a path the runtime returned for something else.
- When a sandbox runtime starts it removes the directories an earlier process left behind, which
  reclaims captures of a crashed runtime or client. This runs before the runtime accepts captures
  and skips a directory whose lock another process holds (a client still publishing from it).
- A capture whose source failed to resume keeps its directory, named in the recovery error, until
  the runtime next starts and sweeps it.
- Disk-only snapshots already removed their directory; they are unchanged.

Nothing else reads these directories: restore and export read the snapshot, and memory objects
come from `runtime/checkpoint-store`.

## Why

Measured on macOS 0.7.6: five full captures of one sandbox leave five `runtime/checkpoints/<id>/`
directories, each with hard links to the root and `/workspace` layers. After this change there are
none, and (together with the compaction cleanup fix) removing the old snapshots returns their disk.

## Test plan

- Unit tests: an installed snapshot, a failed installed publication and an archive each leave no
  capture directory; a capture with a failed source resume keeps it; the sweep removes stale
  directories (dropping the layer link count), skips one whose lock is held, and runs in the
  step that creates the capture root.
- Live run on a throwaway home: five captures leave no directories, snapshots restore, and the
  restored sandbox has the data written before the capture.

## Note

A related idea, linking a sealed layer instead of cloning it when its header already names the
published predecessor, was tried and dropped: no live layer header ever matches, and on APFS all
50 traced copies were `clonefile` clones.

A garbage collector for `runtime/checkpoint-store` was prototyped and dropped: the next
incremental capture links every object of the previous manifest, which still points into old packs,
so it removed only device-state objects (0.1 to 0.3 MiB per capture).

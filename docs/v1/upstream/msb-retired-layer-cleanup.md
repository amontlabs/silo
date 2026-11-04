# Unlink the retired root-disk layers after a compaction

## Summary

After `msb modify --compact` the sealed layers it replaced stay on disk. The merged layer is
written, the journal is switched, but the old layer files are never deleted, so a compaction first
costs the merged size and returns nothing.

## What changed

The cleanup that runs after a compaction only removed paths under the `runtime/` directory. The
root-disk layers live one level up, in the sandbox directory, so none matched. It now covers the
sandbox directory, and skips every layer of the new chain and the launch base (the base is still
needed to start the sandbox). A failed unlink is now logged instead of ignored.

## Why

Measured on macOS 0.7.6: after compacting five snapshots' worth of layers, four retired layers
(about 500 MiB) stayed in the sandbox directory until the sandbox was removed. With this change the
directory holds only the base and the new chain. Snapshots keep their own links, so none of them
changes.

## Test plan

- Unit tests: a chain whose layers sit beside `runtime/` is compacted; retired layers are gone, the
  base, the new chain and a snapshot's hard link remain, and the chain still loads. A second
  compaction unlinks the first merged layer under `runtime/.compact-*`. A restored child keeps its
  nominal launch base (which need not exist), and the start-time base check still passes after
  cleanup.
- Live run on a throwaway home: compact, snapshot, remove the older snapshots; the sandbox directory
  lists only `upper.ext4`.

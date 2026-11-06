# Upstream draft: full checkpoint after the host unlinks a file the guest looked up

Draft issue for MicroSandbox (not filed).

**Title:** Full checkpoint of a running sandbox fails after the host unlinks a file the guest looked up in a virtiofs share

**Version:** msb 0.7.6, macOS host (Apple silicon), Linux guest.

## What happens

A running sandbox's full checkpoint serializes the entries the guest has looked up in the runtime
share (`/.msb`, virtiofs `msb_runtime`). When the host removes a file the guest had looked up, the
checkpoint fails with `control operation rejected by peer` until the guest forgets the entry or the
sandbox restarts.

## Reproduction

1. Start a Linux sandbox `s`.
2. On the host, write a file into the sandbox's `runtime/` folder: `echo hi > runtime/x`.
3. In the guest: `cat /.msb/x`.
4. On the host: `rm runtime/x`.
5. On the host: `msb snapshot create snap --from-sandbox s --full --guest-flush required`.

Result: `control operation rejected by peer`. The same capture succeeds if step 3 is skipped.

## Workaround and its limit

In the guest, `sync; echo 2 > /proc/sys/vm/drop_caches` makes the guest drop its dentry cache and
the capture then succeeds. It does not help while a guest process holds the removed file open (the
open file pins the inode); the sandbox has to restart. Silo runs this before each full capture of a
running computer that has an in-guest bridge folder and pauses its own handling of that folder during
the capture, which narrows the window but cannot release a pinned inode.

## Expected

Serializing the share skips or tombstones inodes the host has unlinked, or `--guest-flush` makes the
guest send FORGETs for them, so a host that removes files from a share it also writes to does not
break checkpoints.

## Related: `root-disk.json` in the guest-writable share

`runtime/root-disk.json` is written with the default file mode (`0666` under the usual umask) into
the runtime folder that the guest sees as `/.msb`, and it holds host disk-layer paths. It is
validated by layer id only (`crates/runtime/lib/checkpoint/disk.rs`, `write_state_with_sync` and
`RootDiskState::validate`), so a guest user that can write it can name the layers the next boot
uses. Asking for creating it with mode `0600` (`OpenOptions::mode(0o600)` on the temporary file in
`write_state_with_sync`, and the same for other files the runtime writes into `runtime/`) and for
the layer paths to be checked against the sandbox's own folder. Silo shows every entry of the
runtime folder except its own bridge folder as root-owned and private to the guest in the meantime,
and applies that again after each start, restore and rename, since the runtime rewrites the file.

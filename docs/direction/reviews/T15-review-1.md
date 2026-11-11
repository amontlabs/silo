# T15 review 1 (Sonnet, of 5217a244d)

No path deletes or overwrites the original workspace disk or `/home/silo`; `computers.rebuild` is
OwnerOnly; shell strings are constants, names go as argv; journal writes are atomic. Findings:

1. Swap not journaled step by step (`mod.rs:261-308`, `engine_env.rs:583-615`): a crash after the
   first rename rolls back by deleting the rebuild, leaving no `<name>`; a crash before relabel
   leaves duplicate machine-ids. Journal `Swapping` first, make the swap idempotent, recover
   forward once any rename happened.
2. Writes after the clone are lost (`mod.rs:200-211`): the old computer boots again after the disk
   clone. Do boot, tar, `msb cp` first; stop; then clone; never start the old one after.
3. Only the keep lane is held (`engine_env.rs:244`); start/stop/drop/checkpoint use
   `operation_gate::Scope::Computer`. Recovery takes nothing and sends `held` at once. Take the
   gate and the lane; recovery too, `held` after.
4. Partial home tar passes (`engine_env.rs:443` `; true`, `tar_out` ignored); failed `du` reads as 0
   (`:201`, `:238`); `--same-owner` with differing uids. pipefail, check exits, verify count, compare
   uids.
5. Host staging tar: default permissions, holds keys, leaks on `?` (`:455-488`), no sweep;
   `/tmp/home.tar` predictable in the guest. 0700 dir, RAII cleanup, startup sweep, mktemp.
6. Refusals after changes: measuring boots a stopped computer; sparse `upper.ext4` length is
   capacity; checks run before the lane; non-layered disk without reflink not refused but
   `cp --reflink=always` used; `is_linux: true` hardcoded (`:122`).
7. New computer from labels/defaults (`:72-84`, `:296-326`): capacity may differ; network policy,
   env, secrets, ports not re-applied; `forget_computer_id` not called after the swap.
8. Undo inventory write result discarded (`:618-626`); confirm the cloned record gives an agent no
   extra control over `-undo`.
9. Rollback leaves the old computer running when it was stopped, the `before-rebuild-*`
   checkpoint, `.swap-<vol>` in `owned-volumes/`; check `.disk-owner` is not cloned.
10. Check script: no image-changed assertion, no undo data check, no crash/stopped/layered/recipe
    cases, `SOCK` unused (prove Dev socket). Unit tests lack a partial-swap case.

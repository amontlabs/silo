# T15 review 2 (Sonnet, of 66f365608)

Holds: swap states converge from every crash point traced; steps before `Swapping` roll back, later
finish forward; lane and gate serialise rebuild with merge/undo/promote/restore; no nested gate;
OwnerOnly; `-undo` and the build id are not in the policy store; new tools token; staging 0700,
swept; guest tar 600; checksum, uid and gid verified. Findings:

1. Writes to `/home/silo` after packing are lost (old computer runs during copy, `mod.rs:359-364`,
   `engine_env.rs:51-60`, `:729`); tar exit 1 accepted. Freeze the `silo` user's processes before
   packing (or stop services), check nothing newer after, treat exit 1 as failure or retry.
2. Recovery takes lane and gate once, outside the retry loop (`mod.rs:477-479`); a failure at
   engine start gives up silently, `held()` never sent, old computer stopped. Move into the loop
   like `promote::recover_one`; notice on final failure.
3. An unrecognised swap state blocks forever (`swap.rs` `state()`), every start and every later
   rebuild fail. After N attempts: clear notice and an owner abandon path that restarts the old
   computer if intact.
4. Device-wide operations (Quit, update) wait for the whole rebuild with no timeout, cancel or
   progress (`engine_env.rs:533-543`); queued device requests freeze other computers' calls.
   `allow_cancel`, `expect_within`, cancellation between steps with rollback, `relabel` per step.
5. Metadata read-modify-write without a lock (`change_computer_configuration_while_held`,
   `write_inventory`, `swap_inventory`, `remove_build`) races other computers' `set_computer_names`;
   `apply_configuration_with_progress` runs device-wide `prepare_retry`. Mutex around metadata
   writes (shared with the existing writers) or a short device-wide gate.
6. `finish` swallows failures and removes the journal (`mod.rs:399-420`, `engine_env.rs:875-895`).
   Keep the journal until settled, or independent steps.
7. A crash during the home carry leaves `/tmp/silo-home-<id>.tar` (keys) in the old computer.
   Remove it in rollback, or put it where startup cleans it.
8. Network policy, secrets and GitHub assignment on the rebuilt computer are unasserted.
9. `home_bytes` may start a stopped computer before the journal exists; the build sandbox is
   addressable by name.
10. Check script: `$A` is created on the newest image so the image check is vacuous; one random
    crash point; no policy, secrets, tools token, `-undo` no-access or Quit-during-rebuild checks;
    leftover `silo-rebuild-*` not removed; `ENGINE_HOME` hard-coded.

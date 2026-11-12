# MicroSandbox 0.7.14 upgrade: Codex review (gpt-6.1-sol, of 25276e2d7)

Verdict: not approved until these four are fixed (R11).

1. P1 `runtime_migration/protected_storage.rs:253`: every running computer is stopped before its
   storage is checked; one Needs attention computer makes every later launch stop healthy VMs.
2. P1 `backup_controller.rs:3159`: `doctor` runs on an import saved as a pending restore before
   its sandbox exists; Needs attention disables Start; relaunch repeats. Unstarted checkpoint
   forks too.
3. P2 `silod/src/startup.rs:85`: storage migration runs after update recovery started; recovery
   fails while the runtime is gated and does not retry in that launch.
4. P2 `protected_storage.rs:395`: backup certificate and copied tree not synced before
   publication; after power loss `covers_all` can trust an incomplete copy.

Holds: upstream migration freezes and revalidates sources, fences older readers, keeps inputs,
publishes readiness after the disk group; fresh ready diagnosis required after repair; no
denied-address forwarding found; export accepts exactly the two defaults; disk-only
missing-resource permission keeps full restores and supplied-object checks. All 23 patch hashes,
order and bytes match the integration commits; cached source, macOS executable, agentd and library
hashes match; runtime and toolchain pins agree. Linux asset bytes not checked.

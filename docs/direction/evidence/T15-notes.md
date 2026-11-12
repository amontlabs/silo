# T15 live check notes (2026-11-11, Dev engine, Apple silicon, one guest image)

Final run: `evidence/T15.txt`, 173 PASS, 0 FAIL, 3 SKIPPED. No product code changed.

**Skipped.** A secret cannot be assigned on the Dev engine without the owner approving Silo's
Keychain access (`credential_access_denied`), so the three secret checks are skipped. The unit
tests cover that a rebuilt computer keeps its secret assignment and the undo computer has none.

**Image.** Only `ubuntu-24.04-v5` exists in the Dev guest-image folder, so the image reference
cannot change. A new root disk file (inode and creation time) shows the computer was rebuilt; the
undo computer has the old file.

**Script fixes made during the run.** The journal folder is `~/Library/Application Support/
org.silo.dev/rebuild`, not the engine folder; the secret name must look like an environment
variable; `workspace_moved` is overwritten by `recipe_replayed` within microseconds when there is
no recipe, so that crash point is "home_carried" plus 0.25 s (inside the disk copy); a Quit that
works ends with the engine closing the connection.

**Engine finding outside T15 (not fixed).** `evidence/T15-lock-finding.txt` is a run before the
script stopped orphaned computers after each kill. After the kill at "swapping + 2.0 s" the old
engine had just started the rebuilt computer. Its runtime process (and those of two other running
computers) kept a copy of the open file `.silo-configuration-worker.lock`, and the kill never
released the lock. Result: the next engine waited out the 3-minute lock timeout before settling,
and every later create, stop and Quit failed with "The previous computer command is still
finishing", and `silo stop` could not stop those computers until `msb stop` was run on them by
hand. This is the existing `command_lock` in `runtime/configuration_recovery.rs`, used by every
computer command, and not the rebuild code. A killed engine (crash, update, force quit) that
leaves computers running can therefore block all computer changes until they are stopped. The
final script stops the throwaway computers' runtime processes after each kill, as a crashed
machine would.

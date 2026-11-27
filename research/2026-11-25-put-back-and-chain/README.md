# Put back with a preview, and the chain without processes

Research of 25 November 2026, by an agent, for polarzero. silo-research #17 checked the demo
against the drafts and found two gaps. No prototype's put-back has a preview or a confirm, and it
brings back only deleted files. No view draws option A's chain, from a file to a tool call with no
process in between. This prototypes both on a recorded night and tests them.

It stacks on #399 and uses #399's `attribute.py` and #371's prototype as they are.

| File | Holds |
|---|---|
| [`putback.py`](putback.py) | `silo put-back <time> <path>…`: #370 step 5's six steps for any change, with a preview and a confirm |
| [`chain.py`](chain.py) | `silo chain <path or change>`: file → tool call → turn → what you asked → who else uses it, under option A |
| [`night_a.py`](night_a.py) | A night read as option A reads it: facts only, no process, authors from #399's rules |
| [`test_putback.py`](test_putback.py) | 18 tests, run on #399's `one-main` night |
| [`results/`](results/) | The previews, the chains, the tests and a second run of #399's attribution, as printed |

## The night

#399's `one-main` night, run again here: the real Claude Code 2.1.332 and the real Codex 0.172.0
against #371's scripted model, all four runs in main, a checkpoint every few seconds. Computers
are folders, and checkpoints are #371's manifests, not APFS clones. Nothing ran on a Mac.

## What was found

1. **#370's spec loses a write and says it does not.** Step 5 says a write between the preview
   and the swap "is in" the checkpoint saved at step 3. It is not. A write after the save and
   before the swap is in no checkpoint, and the swap removes it from main. The test
   `test_370_as_specified_loses_a_write_made_during_the_copy` shows this.
2. **Two changes close that gap, and both are tested.** First, the confirm is bound to what the
   preview showed. Silo keeps a fingerprint of the paths as shown and checks it before the save
   and again just before the swap. If anything changed, Silo stops before swapping, keeps the
   save, writes "put back stopped" in the record, and asks you to look again. Second, after the
   swap, Silo takes one more checkpoint while the staging folder still holds what the swap took
   out. That covers the short time between the last check and the swap.
3. **The swap should be one call.** #370 moves the current path aside, then renames the copy into
   place. A program reading the path during those two renames found it missing in 612 to 1,460
   reads out of about 110,000 to 350,000, across runs. One exchange of the two names (Linux
   `renameat2` with `RENAME_EXCHANGE`, macOS `renamex_np` with `RENAME_SWAP`) never left it
   missing, in about 120,000 to 330,000 reads per run. #370 already uses `RENAME_SWAP` for a
   restore.
4. **A preview of any change says whose work it undoes.** For `~/work/app` back to an early
   checkpoint, the preview groups the 46 rows by who changed them since. Some are claude #2's
   fixture deletions and commit, some are claude #1's `Write` of `store.js` and its commit. For
   `data/sessions.json` it says "not known from the traffic; while claude #1, codex #1, claude #3
   ran". This is the warning the user needs before undoing another agent's work. It is the same
   list `putting-back-a-runs-paths` (taken) calls "the others are shown".
5. **Option A's chain can be drawn from facts and one reading.** For the 400 deleted fixtures, the
   chain goes file → ◇ Silo's reading that `rm -rf ~/work/shared-fixtures` names a folder above
   them → ● the tool call, its result and turn 3 of claude #2 → ● "Don't touch anything outside
   the repo" → ◇ `~/work/client-b/test.config.js` uses it. That is silo-research #17's shot 3
   under A. 489 links are drawn on the night, and none disagrees with the probe where the probe
   named a run.
6. **The weaker rules stay out of the chain, and a second run shows why.** On this run, #399's
   timing rules ("own files", "only one"), which `author-from-traffic` already leaves out of its
   answer, named the wrong conversation three times. #399 measured zero on its own run. The rules
   in the answer stayed at zero wrong. Where only weak rules apply, the chain stops at the file
   and lists the tool calls that were open. For `~/.bashrc` that list shows codex's
   `curl -fsSL http://bun.sh/install | bash`, so the user can judge without Silo guessing.
7. **"Who else uses it" should follow paths, not names.** #371 matched a folder's name. That found
   `fixtures` inside client-b's `'../shared-fixtures'` and called it a user of
   `~/work/app/test/fixtures`. Resolving each path a file names, from its own folder or from `~`,
   keeps the real hit and drops the false one. It still misses paths a program builds at run time,
   such as `path.join(__dirname, '..', 'data', 'sessions.json')`.

Put back and chain print every path and agent text escaped (`«…»`, control characters as
`\xHH`), as #370 asks. One test puts a file named with a terminal escape into main.

## What it changes in the plan

| Entry | Change |
|---|---|
| `putting-back-a-path` | Notes: findings 1 to 4. A sentence is proposed for the answer: "If a path changes after you saw what it replaces, Silo stops and shows you again." |
| `the-chain` | Notes: findings 5 to 7, the chain under A as drawn |
| `author-from-traffic` | Notes: finding 6, a second run that supports the answer's exclusions |

No verdict, answer, list or taken entry was changed.

## For polarzero to rule

- `putting-back-a-path`: add "If a path changes after you saw what it replaces, Silo stops and
  shows you again" to the answer, with the fingerprint check and the checkpoint after the swap.
  Recommended. Without them, "nothing is lost" in the answer is not true.

## What was not verified

- Nothing ran on a Mac or inside a VM. The swap and the copy ran on Linux folders. On a Mac they
  run inside main over SSH, so the cost of the fingerprint check over SSH was not measured.
  Neither was its cost on a folder of 20,000 files, which needs one listing and one hash of each
  file.
- The night is #371's scripted one. Two agents sharing a repository on a real night will change
  more of the same files, which makes the preview's "not known" group bigger. Its size was not
  measured.
- #378's attack is not addressed. An agent swaps a folder for a link while a root `tar -x` copies
  into it. This prototype copies as the user, not as root.
- Checkpoints here hold every file's content, so every file is readable. APFS files Silo's reader
  cannot read are named and skipped by the code, but no such file existed to test it.
- The checkpoint after the swap shows up in the record as `.silo-put-back-*` files created and then
  deleted. A build should mark those or keep them out of `silo changes`.

## Run it

Python 3, Node, strace, `claude` on the PATH, and Codex 0.172.0, as for #399:

```sh
npm install --prefix /tmp/codex @openai/codex@0.172.0
CODEX_BIN=/tmp/codex/node_modules/.bin/codex ../2026-11-25-attribution-without-processes/nights.sh /tmp/nights one-main
python3 putback.py /tmp/nights/one-main main:f193 ~/work/shared-fixtures      # preview, then asks
python3 chain.py /tmp/nights/one-main ~/work/shared-fixtures
NIGHT=/tmp/nights/one-main python3 -m unittest test_putback -v
```

Change ids such as `main:f193` differ from night to night. `results/chain.txt` prints the id of
each change's put-back.

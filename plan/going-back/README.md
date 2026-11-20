# Going back

Checkpoints taken on their own, restore, forks, and what would bring a path back.

## For

To undo what an agent did, or to try something risky, without losing what matters
(`automatic-checkpoints`).

## Why it is so

- **Two grains, each with its own promise.** A whole computer at one moment
  (`whole-computer-grain`), and one run inside a shared main (`one-run-grain`). Undoing one agent's
  work among several is left out (`no-automatic-undo`).
- **On a shared main** (`fork-from-the-moment`).
- **No merge of machine state** (`no-merge-of-machine-state`). A fork ends in one outcome
  (`how-a-fork-ends`), and what is worth keeping from it leaves through Git
  (`code-leaves-through-git`) or is saved by hand (`keeping-from-a-fork`).
- **One question asked of every path** (`what-would-bring-it-back`), and an opinion never decides
  what a checkpoint keeps (`opinion-never-chooses-what-is-kept`), because a wrong one would lose
  history that cannot be computed again.

## One example

An agent works in a fork on a risky change. Before you drop the fork, Silo lists what is "only
here": an uncommitted file and an edited instruction file. You bring the file back to the parent,
and the fork goes to the bin (`before-a-drop`, `drop-and-bin`).

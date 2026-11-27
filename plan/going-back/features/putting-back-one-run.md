# Putting back one run's work

## The choice

An agent's part, 22 November 2026. Three agents work in main. Agent B, sent to fix the CSS, also
rewrote `config/database.yml` and two scripts in `scripts/`. You put back B's run. The two scripts,
which nobody touched since, go back in one step. Agent A edited `database.yml` afterwards, so Silo
shows it to you instead. In `~/code/site`, Silo says which two commits came from B, and you revert
them with Git.

The options:

- Build it (recommended). One run's work is undone without touching the others'. It costs certainty:
  which files a run changed is Silo's opinion, and a file it changed through another program, such
  as a server it asked to write, can be missed.
- Leave it out. To undo one agent, you restore main and every agent goes back, or you fork from
  before and carry the others' work over. It costs the others' work, or the time to carry it.

Recommended: build it. In a main shared by several agents, undoing one of them without the others is
the way back that case needs.

## A prototype of putting back (24 November 2026)

An agent's part, 24 November 2026, from `research/2026-11-24-checkpoint-engine/` (Linux, an ext4
disk standing for a Linux computer's). A run deleted a folder of 20,000 files, edited 10 files and
added 5; another agent then edited 2 of the 10. The prototype found the 20,219 paths the run
changed in 1.2 s, put back 20,209 and removed the run's 8 new paths in 0.7 s, and showed the 2
edited later. Three things for whoever builds it:

- A checkpoint is read through a clone of it: the ext4 disk of a running computer has a journal to
  replay, which Linux does only on a disk it may write. The checkpoint itself never changes.
- The files are written inside the computer, over SSH or a shared folder, since the computer's own
  kernel has its disk mounted. Each goes to a temporary name and is renamed, so no program reads
  half a file.
- An agent that writes a path between Silo's check and Silo's write loses that write. Pausing the
  computer for the put-back, 0.7 s here for 20,000 files, closes that window.

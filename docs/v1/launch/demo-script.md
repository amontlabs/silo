# Silo v1: 60-second demo, shot list

> **DRAFT for the owner's approval.** Terminal film of the v1 flow: up, fork four, review, keep,
> undo, throw away. Commands and outputs are those of the final run's demo section
> (`docs/v1/evidence/final/final-run.txt`, D1 to D6), with fork names shortened to `try-1`..`try-4`.
> This is for the v1 launch; `docs/SiloUI-DEMO-SCRIPT.md` (the app film, fixtures only) is left
> as it is.

## Ground rules

- The time column is edit length, not how long Silo took. Where the real wait is cut, the cut is
  marked on screen ("time cut") and, when a measurement exists, the caption gives it with its
  source. No other durations appear on screen.
- Record on a real Silo build, not fixtures. Use the production `silo` command so the screen
  shows `silo` and `ssh main.silo`; the final run used a Dev build (`silo-dev`, `main.silo-dev`).
- Start with no `main` (`silo list` shows none), the project folder open in the terminal and the
  `demo` recipe already built once, so `silo up` does not show the package install.
- One terminal, large font, no music, no voice-over. Captions are plain text, bottom left.

## Shots

| Edit time | On screen | Command | What the viewer sees (from the run) | Caption |
|---|---|---|---|---|
| 0–4 s | Title card | — | "Silo" | git for your agent's computer |
| 4–10 s | Terminal | `silo up --recipe demo` | `main is running (made from demo). ssh main.silo reaches it.` | One computer: `main`. (time cut) |
| 10–20 s | Terminal | `silo fork main -n 4 --name try -- bash -c 'echo kept-from-$(hostname) > ~/final-note.txt'` | four lines `try-N  exit 0  http://3000.try-N.silo.localhost:47383`, then `4 forks of main; review with …` | Four forks, each ran a command. Measured: median 17.06 s, N=3 (final-run.txt). (time cut) |
| 20–27 s | Browser, four tabs | open the four printed URLs | each page shows `host=` and that fork's hostname | A URL per fork. |
| 27–35 s | Terminal | `silo diff try-1` | `Other changed` / `+ /home/silo/final-note.txt` / `Ignored: 2 paths (caches, logs, build output)` / `credentials used: none` | What the agent changed. |
| 35–44 s | Terminal | `silo fold try-1`, then `silo exec main -- bash -c 'cat ~/final-note.txt'` | `files: 1 written, 0 already identical`, `Undo with …`, then `kept-from-try-1` | Keep it in main. (time cut) |
| 44–51 s | Terminal | `silo undo main`, then `silo exec main -- bash -c 'test ! -e ~/final-note.txt' && echo gone` | `… is running again, as it was.`, then `gone` | Changed your mind? Undo. (time cut) |
| 51–55 s | Terminal | `for f in try-1 try-2 try-3 try-4; do silo stop $f; silo drop $f; done`, then `silo list` | no `try-` rows | Throw the rest away. |
| 55–60 s | End card | — | "Forks get none of the credentials Silo manages unless you name them; files on disk are copied." | Repository URL |

## Notes for the owner

- **Shot 20–27 s is not in the final run.** The run printed the four URLs but did not open them;
  four forks answering at once on their own URLs was checked in phase 3
  (`docs/v1/evidence/phase-3/urls-check.txt`), on an earlier build. Record it, or drop the shot
  and give its seconds to the diff.
- **Only one timing is a measurement** (the `-n 4` caption, from
  `docs/v1/evidence/final/README.md`). The demo run's own `-n 4` took 18.17 s
  (`final-run.txt`, D2): one run, with a different command in the forks, so not shown as a number.
  `silo up` (12.06 s in D1) includes the recipe build and is marked "not a measurement" there.
  Keep and undo were not timed.
- **Wall-clock length of the real flow:** the run's D1 to D6 timestamps go from 10:27:34 to
  10:28:40, so the uncut flow is longer than this edit even without `silo up`. The "time cut"
  markers are what makes the edit honest; do not remove them.
- The `Kept … into …` and `Put … back …` lines print computer ids, not names. Keep them as they
  are; do not retype them as names.
- Optional second film (not in this cut): quiet forks (`final-run.txt`, Q1), Make this main
  (P1–P2), computer use on a fork (C1–C2, screenshots in `docs/v1/evidence/final/raw/`).

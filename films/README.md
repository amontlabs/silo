# Films

A film is a short narrated explainer of some entries of the plan. The owner watches one to review
what a decision sets. A visitor watches one to understand a part.

- A film states nothing of its own. Its `film.json` names the entries it shows, and the page under
  the film prints them from the plan.
- A film is behind when an entry it shows has changed since it was built.

Each film ends as one HTML file: the film with its voice and subtitles, the entries it shows as
`plan/` holds them, and the questions the owner answers under the film.

## The films, in the order they are watched

| Film | Kind | Scenes |
|---|---|---|
| `engine-one-call` | teaches | 6 |
| `what-silo-knows` | decides | 7 |
| `actions-and-kinds` | decides | 6 |
| `rules-and-precautions` | decides | 8 |
| `added-classifiers` | decides | 6 |

The owner watched all five on 19 November 2026. What the owner settled since and a film does not carry
yet is in that film's `pending.md`; the file goes once the film carries it. The fifth film needs a
new script.

## What is here

```
README.md            this file
.gitignore           every out/, the Python environment, caches, index.html, the kit's renders, voice.py's temporary folders
index.html           made by tools/index.py, not tracked: the films, in order
kit/                 the Fjord explainer kit, copied at its commit 7ce714b533 without its examples/; NOTICE.md lists its third-party files
player/              the player as used here: kit.js (patched copy), voice.js, page.html, page.css, page.js
tools/
  voice.py           narration.json → voice/ clips + voice.json
  entries.py         reads from plan/ the entries a film shows, and what each not decided would set (standard library only)
  place.py           the folders and files a tool may use: a film is a folder directly under films/ (standard library only)
  build.py           → out/index.html, one file with everything inlined
  check.sh           the checks; builds first
  shot.sh            stills of the built page, taken the way check.sh takes them
  check_sources.mjs, check_player.py, check_page.py, sheet.py, listen.py   (used by check.sh)
  _venv.py           makes every Python tool that needs it run with .venv, offline
  index.py           → index.html, the one page that lists the films in the order to watch them
<id>/
  script.md          the script the film was made from
  film.json          id, kind, title, line, shows, words, and the optional parts below the film
  narration.json     the voices and, per scene, the sentences spoken
  explainer.js       the film
  voice.json, voice/ made by voice.py (voice/ is the sentence cache: keep it, it is costly to make again)
  pending.md         what the owner settled that the film does not carry yet, when there is any
  out/               made by build.py, check.sh and shot.sh, not tracked: index.html, sheet.png, stills/, shots/, check.log
```

Read [`kit/CLAUDE.md`](kit/CLAUDE.md) before making a film. It describes the pieces, the motions and what makes a
film good. Do not edit anything in `kit/`. The player's changes live in `player/` and are listed
at the end.

## Outside the repository

The tools take only a film folder directly under `films/` that holds a `film.json`, and refuse any
other, a link that leads elsewhere included. A clip that `voice.json` names must be a plain file of
that film's `voice/`. What they and the kit use outside this repository:

- the browser shell of Playwright, in the user's cache:
  `~/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell`
  (`check.sh`, `shot.sh`). Each run exits by itself; never use the Chrome in `/Applications`, and
  never kill a run.
- ffmpeg, at `/opt/homebrew/bin/ffmpeg` (`voice.py`).
- the Hugging Face cache, read offline: Kokoro's voice model (`voice.py`) and the Whisper
  listening model (`listen.py`).
- macOS `say`, for the stand-in voice only (`voice.py --stand-in`).
- `/usr/bin/plutil`, called by the kit's guard (`kit/guard.sh`).
- the Python at `/Library/Frameworks/Python.framework/Versions/3.12/bin/python3`, called by the
  kit's screenshot tool (`kit/shot.sh`).

The kit's third-party files and their licences are listed in [`kit/NOTICE.md`](kit/NOTICE.md).

The kit's own scripts (`kit/render.sh`, `kit/shot.sh`) name other paths of the machine they were
written on. The films' tools do not call them.

## Python

`entries.py` needs only the standard library and runs with any Python. `build.py` uses only the
standard library too, but starts through `tools/_venv.py` like the other Python tools: they run
with `films/.venv/bin/python` and with `HF_HUB_OFFLINE=1`, so nothing reaches the network.
`tools/_venv.py` does this: each tool starts itself again with that Python, whichever Python you
call it with, and stops if `.venv` is missing. `check.sh` uses the same Python.

The environment is not tracked (about 840 MB, it pulls in torch). Make it from the Python 3.12 of
python.org, which already holds `mlx-audio`, `soundfile`, `pillow`, `faster-whisper` and `numpy`:

```
uv venv --python /Library/Frameworks/Python.framework/Versions/3.12/bin/python3 --system-site-packages films/.venv
uv pip install --python films/.venv/bin/python 'misaki[en]'
```

Kokoro's model (`mlx-community/Kokoro-82M-bf16`) is read from the Hugging Face cache.

## Kinds of film

Each film is one of two kinds, set by `kind` in `film.json`. The script says which.

- **teaches**: how the engine works, established engineering practice the owner cannot judge. The
  page asks whether each scene was clear, one question to test the picture, what the owner has seen
  go wrong, and a choice only where a mechanism has a price the user pays. Its promises are the
  plan's properties, shown under `shows` like any entry.
- **decides**: what the product does.

Under both kinds, the entries the film shows are printed from `plan/`. An entry not decided shows,
under its sentence, every sentence a yes would set (the list it goes to, its id or word, and its
`says` or `means`) and every id it would remove, since a yes covers exactly that. Then it has Agree,
Change and Leave open, and a note. An entry in force, or dropped, shows its sentence and no buttons.

## Making a film from a script

All commands run from `films/`.

1. **Make the folder.** Create `<id>/` and put the script there as `script.md`.

2. **Write `film.json`** from the script. Copy `engine-one-call/film.json` (it has every part)
   and change it. Copy all text exactly from the script. Where a script's paragraph breaks across
   lines, join the lines with single spaces.
   - `id`: the folder's name. Answers are kept under it.
   - `kind`: `"teaches"` or `"decides"`, from the script.
   - `title`: the script's title.
   - `line`: the script's answer line. The page shows it under the title.
   - `shows`: the ids of the plan's entries and sentences the film explains, in the order the page
     lists them. Find them with `tools/plan/bin/plan show <id>` and in `plan/`. A sentence that a
     decision not yet taken puts forward under `sets` is shown by that decision's id. A thing the
     film says that no entry holds, a promise included, goes into the plan first, as an entry; the
     film never states it.
   - `words`: `{ "word", "means" }`.

   Optional parts. Leave out any part the script does not have, or marks "None":
   - `clear`: `true` gives "Was it clear?": one line per scene (its caption), Clear or Lost me
     here, and why.
   - `test`: `{ "question", "answer" }` gives "Test the picture": the question, a field for the
     answer, and a button that shows the given answer.
   - `wrong`: the prompt gives "Seen this go wrong?" with one field.
   - `choice`: `{ "question", "options": [{ "key", "text", "cost", "recommended" }] }` gives "A
     choice". `key` is the option's letter. `cost` is the text after "What it costs you:". Set
     `"recommended": true` on the one option the script recommends. The owner picks one option
     and may add a note.

   The page shows the parts in this order: Was it clear?, Test the picture, What the film shows,
   Seen this go wrong?, A choice, Words, Copy my answers.

3. **Write `narration.json`.** List the voices and give each scene its sentences, in order. A
   numbered narration line in a script often holds several sentences. Split it: each sentence
   goes in the list on its own, and each becomes one clip. Each clip gives you a moment to place
   a motion, and one subtitle line. Keep the words exactly as the script gives them. Copy `voices`
   and `speed` from the first film: Heart (`af_heart`) only, at speed 1.0, which the owner
   approved. The tools can make several voices; the voice switch appears only when a film has more
   than one.

4. **Make the voice:** `tools/voice.py <id>`. It prints each scene's narration length per voice.
   - Clips are cached by a hash of engine, voice, speed and text. Changing one sentence speaks
     only that sentence again, and clips no longer used are deleted.
   - Silences: 0.4 s before the first sentence of a scene, 0.35 s between sentences, 0.6 s after
     the last.
   - Kokoro speaks every voice listed (today only `af_heart`), from the model in the local cache.
   - Fallback only: if Kokoro cannot run (misaki missing from `.venv`), the script stops and says
     so. `--stand-in` then lets macOS `say` speak instead (Samantha for `af_heart`, Daniel for
     `am_michael`), and the voice switch shows "(stand-in: …)". Do not send a film made that way.
     Run `voice.py` again without the flag once Kokoro works, then `check.sh`: every timing
     follows from `voice.json`.

5. **Write `explainer.js`.** Write it the way `kit/CLAUDE.md` says, with these differences:
   - These films are carried by a voice: usually 60 to 100 seconds and five to seven scenes, not
     15 to 30 seconds. This is not a hard limit. The owner wants them short and clear, and longer
     when a subject deserves it (19 November 2026). Never cut an idea, and never rush a sentence,
     to fit a length.
   - Each scene's `caption` has ten words or fewer. It shows above the stage as the scene's title.
     Its `source` is shown under the film by the Sources button.
   - `build(K, tl, scene, V)` gets a fourth argument, `V`, the narration's timing for the current
     voice:
     - `V.at(s, j)`: seconds into scene `s` where sentence `j` starts. Both count from 0.
     - `V.word(s, j, "phrase")`: where that phrase of the sentence is said. It is estimated from
       the phrase's place in the text, so it can be off by a few tenths of a second.
     - `V.end(s, j)`: where sentence `j` ends. `V.total(s)` is the scene's narration with its
       silences.

     Pass these as the kit's numeric `at`, for example `K.glow(row, { at: V.word(0, 2, "Each row") })`.
     Tie each motion to the sentence that names it. A motion with no `at` follows the one before.
   - A scene lasts at least as long as its narration. The player stretches it, so do not pad with
     empty motions. If the motions run longer than the voice, the scene runs longer too.
   - Code shown on stage keeps its spaces: write the indentation and aligned columns as they
     should appear.
   - Use `V` for every time tied to the voice. Never write a number taken from `voice.json`: it
     changes when a sentence changes, and each voice has its own timings.

6. **Check:** `tools/check.sh <id>`. It prints a summary and exits 1 if any check fails.
   1. **sources**: `film.json` has `shows`. Captions have ten words or fewer. The film has as many
      scenes as the narration, and its title is the film's.
   2. **player**: the page is loaded once per voice. It fails on any error, on any layout note
      from the kit (overlap, a piece cut by the frame, something on an arrowhead, words cut short)
      and on any scene shorter than its narration. It prints each scene's length.
   3. **page parts**: the built page shows every part named in `film.json`, with the right number
      of items, and no part that is not there. Every id in `shows` is listed, in order, with its
      sentence and status from `plan/`. An entry not decided has its three buttons and a note, and
      every sentence it would set and id it would remove; one in force or dropped has none of
      these. `film.json` has no `statements`, no `marks` and no `promises`. The build
      itself stops on an id that `plan/` does not hold.
   4. **stills**: the held frame at the end of every scene, in light and in dark, goes to
      `out/stills/`, plus `out/sheet.png` (one row per scene, light left, dark right). It also
      saves the whole page: `page-light.png` (opening, with the Play button) and `page-dark.png`
      (a frame mid-scene, with a subtitle).
   5. **listening**: would transcribe each clip with faster-whisper and list the words heard
      differently. **It is skipped today.** faster_whisper imports, but the Whisper model in the
      Hugging Face cache (`mobiuslabsgmbh/faster-whisper-large-v3-turbo`) is incomplete: its
      weights and tokenizer are missing. The check downloads nothing, so it prints "skipped" with
      that reason, and the last line of `check.sh` says the listening check was skipped. Until the
      model is in the cache, listen to the clips yourself if a word matters. Nobody but the owner
      has heard the films.

7. **Look at the stills** (read the PNGs, `out/sheet.png` first, then single frames) and fix what
   you see. Two or three rounds is usual. Look for:
   - pieces that overlap, or text that is clipped or crowded;
   - a link's label hidden behind a piece (the layout check does not catch this; place a `K.text`
     instead);
   - anything resting on an arrowhead;
   - a piece cut by the frame;
   - a held frame that does not show the scene's point (its hero moment should still be visible);
   - a frame where half the stage is empty: frame it with the camera;
   - the same frame in dark: colours that vanish on the dark ground.

   To see a moment of motion or any other view, use `tools/shot.sh <id> "<query>" …`. It takes one
   still per query, the way `check.sh` does, into `out/shots/`, and prints each file's path. A
   query with `embed=1` gives the stage alone at 1280×720, for example `"t=21.5&embed=1"` or
   `"scene=2&theme=dark&embed=1"`. Any other query gives the whole page, for example
   `"theme=dark"`. Build first.

   Also look at the whole page once (`out/stills/page-light.png`): every part present, in order,
   nothing clipped.

8. **List it:** add the id to `ORDER` in `tools/index.py`, then `python3 -I tools/index.py`.

The page to send is `<id>/out/index.html`. It needs no network and no other file. Delete `out/`
once nothing needs it.

## The page

From top to bottom, the page shows the title and the line, then the film. Above the stage is the
scene's caption, and under it the sentence being spoken. The controls are play, the scene bar, the
time, Sources, Sound on/off, and the voice when there is more than one. Below the film come the
parts listed in step 2, then a "Copy my answers" button. It puts every answer on the clipboard as
plain text: each scene's clarity and why, the answer to the test, each entry not decided by its
id, the story and the pick with its note. Answers are kept in the browser's
localStorage under `silo-walkthrough:<film id>`, the answers on entries under `entries` by id, and
so are the voice and Sound on/off. `player/page.js` builds the parts.

Query parameters, from the kit and the voice layer: `?scene=N` (the held frame of scene N,
counting from 0), `?t=seconds`, `?theme=light|dark`, `?embed=1` (the stage alone, as the stills
use it) and `?voice=<id>`.

## How the sound follows the film

The film's timeline is the clock. On every frame the player asks which sentence is being spoken.
If one should be sounding, it starts that clip at the right offset through Web Audio. Whenever the
sound and the timeline drift more than 0.1 s apart, it starts the clip again at the right offset.
Each sentence starts from the timeline again, so nothing builds up over a film. Pausing,
scrubbing, jumping to a scene, replaying, a theme change (which rebuilds the film) and a voice
switch all go through the same path. Muted, nothing sounds, but the timing and the subtitles are
the same. The browser allows no sound before a click, so with a voice the film opens paused
behind a Play button. No clip is decoded before the first click, so stills and timing work
headless without sound.

## Changes to the player

The kit's own files are unchanged. These are the differences, so they can be sent back.

`player/kit.js`, a copy of `kit/kit/kit.js` with seven small changes (run
`diff kit/kit/kit.js player/kit.js` to see them):
1. `need(i)` reads the narration's length of scene `i` from `window.FILM_VOICE`, or 0 when there
   is no voice.
2. In `scene(i)`, the previous scene ends at `max(its motions, its start + need(i - 1) - HOLD)`,
   so a scene lasts at least its narration.
3. The last scene does the same with `END_HOLD`, in `mount()`.
4. `spec.build(K, tl, scene, window.FILM_VOICE)`: build gets `V` as a fourth argument.
5. `show()` calls `window.FILM_VOICE.frame(tl, starts, scene)` on every frame, for subtitles and
   sound.
6. With a voice, the film opens paused instead of playing. `window.__player` gains `rebuild()`
   (builds the film again where it stands, used by the voice switch) and `starts` (each scene's
   start).
7. A window row's `meta` (the small words at its right, used for a record row's mark such as
   `fact · disk`) is size 14 in the muted tone, not 13 in the faint tone: it has to be read in a
   held frame.

New files:
- `player/voice.js`: the voice layer. It provides `V`, the subtitles, the sound synced through
  Web Audio, the Play button, Sound on/off and the voice switch.
- `player/page.html`: the page's markup, which replaces `kit/kit/player.html`. It puts the caption
  above the stage, adds a Play button over the stage and a subtitle line under it, and wraps the
  film in the page. The kit's `.ask` and `.answer` elements are kept but hidden: the page shows
  its own title and line.
- `player/page.css`: the page's styles, loaded after the kit's `player.css`. It also sets
  `#stage text { white-space: pre }` so code keeps its indentation, and extends the kit's
  `?embed=1` to hide the page around the film.
- `player/page.js`: the parts under the film, their answers, and the copy button. It runs after
  `explainer.js` and reads the scenes' captions from the kit's `window.__spec`.

## Writing a script

- One idea per scene, caption or sentence.
- Ground work only: no detail of one use case, no arrangement, no feature.
- What the voice says, the picture shows, whole. Wrong: the voice names four cases and the picture
  shows two.
- The title names what the film is about, and nothing else in the plan.

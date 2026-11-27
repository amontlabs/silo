# Persistence after a bad night

What an agent leaves behind to run later (shell start-up lines, scheduled jobs, services, git
hooks, install scripts, its own settings, a live process), whether the record finds each one, and
whether putting back by path (#370, step 5) undoes it. Measured on a Linux stand-in for main, with
the macOS places listed for the Mac kit.

## What was found

| | Result |
|---|---|
| Things planted that run later | 29, in 10 kinds of place, on a night that also did ordinary work (8,891 changed paths) |
| In the file facts between the two checkpoints | 29 of 29 |
| On a list of "places that run later" | 29 of 29, in 41 lines. The same night without the agent gives 8 lines, every one a real thing that runs later (the bun installer's lines in `~/.bashrc`, a PostgreSQL unit, husky's hooks, an editable `.pth`, two dependencies' install scripts) |
| Undone by put-back as #370 specifies | 8 of 29. The other 21 are new files, and #370 refuses a path the checkpoint does not have |
| Undone by put-back that removes a path absent from the checkpoint | 29 of 29 |
| Of those, undone without losing good work | 27. Two sit in a file the good night also changed (`~/.bashrc`, the repository's `.git/config`), and putting the file back takes the good change with it |
| A live process that writes its line back | The line was gone right after the put-back and back 3 seconds later. After the process ended (what restarting main does) and one more put-back, it stayed away |

Three things this changes:

1. **Put-back must be able to remove.** Most persistence is a new file. "Put this path back as it
   was" has to include "as it was: absent", shown in the preview and saved first like any other
   put-back. #365's put-back of one run already removes what the run created; #370's put-back by
   path does not. New open entry `putting-back-a-new-path`.
2. **A put-back does not end what is running.** A process started from a file keeps running after
   the file is gone, and can write it back. Without processes in the record (option A, #399), Silo
   cannot name it. Restarting main ends every process and keeps the disk. New open entry
   `restart-after-a-put-back`: Silo offers a restart after a put-back that touches a place that
   runs things.
3. **The morning needs a short list of places that run later, with keys.** A list by path alone
   either misses things (`core.fsmonitor` in `.git/config`, `scripts.postinstall` in
   `package.json`, `node-options` in `.npmrc`, `LocalCommand` in `~/.ssh/config`) or flags every
   edit to files agents change all night for good reasons. The list here matches paths for files
   that run whole (shell start-up, crontabs, units), and keys inside files where only some lines
   run something. Two more rules found what the list alone missed: a file a listed line names
   (`--require ~/.cache/.n/hook.js`, a library in `/etc/ld.so.preload`), and the folder
   `core.hooksPath` points at (husky's `.husky/`). This is the "places people worry about" of
   #370's shipped data (`engine/data/`), and the same list `fork-identity` needs for "scheduled
   jobs off in a fork".

## The test

`proto/score.py <workdir>` runs it all, as root, in about four minutes; results are in
`proto/results/`.

- `night.py` makes main as a folder (`main/` is its `/`): a home with shell files, git, SSH, Claude
  Code and Codex settings, a Python virtual environment, a repository with 12,000 files of
  dependencies. Checkpoint A is a copy. Then a good night: an `npm install` of 6,000 files with two
  install scripts, husky, a `pip install` with an editable `.pth`, the bun installer's lines in
  `~/.bashrc`, a PostgreSQL user unit, commits, caches, a build. A bad night is the same plus 29
  planted items. Checkpoint B is a copy taken while the live process runs.
- `silo_persist.py` holds the stand-ins: `facts` (the comparer of #370 step 5: new, changed and
  gone paths), `later` (the list below), and `putback` (#370's steps 4 and 5: a copy beside the
  target, then a swap; mode `370` refuses an absent path, mode `absent` removes it).
- `score.py` puts back each item's paths on its own copy of B and checks main is as at A at those
  paths; then puts back everything listed on the running main and watches `~/.bashrc`.

Each item, with where it goes on a Mac (`proto/results/items.jsonl` has the paths):

| Item | Linux place | On a macOS main | #370 | Removing absent paths | Good work undone |
|---|---|---|---|---|---|
| bashrc-line | `~/.bashrc` | `~/.zshrc` | yes | yes | bun's lines |
| bash-profile-new | `~/.bash_profile` | `~/.zprofile` | no | yes | |
| zshenv | `~/.zshenv` | `~/.zshenv`, read by every zsh, scripts too | no | yes | |
| profile-d | `/etc/profile.d/` | `/etc/zprofile`, `/etc/paths.d/` | no | yes | |
| user-crontab | `/var/spool/cron/crontabs/u` | `/usr/lib/cron/tabs/` | no | yes | |
| cron-d | `/etc/cron.d/` | `/Library/LaunchDaemons/` with `StartCalendarInterval` | no | yes | |
| at-job | `/var/spool/at/` | `/usr/lib/cron/jobs/` (atrun is off by default) | no | yes | |
| systemd-user-unit | unit and its enable link | `~/Library/LaunchAgents/` and `launchctl bootstrap` | no | yes | |
| systemd-timer | timer and its enable link | a LaunchAgent with `StartInterval` | no | yes | |
| xdg-autostart | `~/.config/autostart/` | Login Items (Background Task Management) | no | yes | |
| git-hook | `<repo>/.git/hooks/` | the same | no | yes | |
| git-global-hookspath | `~/.gitconfig` and the hooks folder | the same | no | yes | |
| git-fsmonitor | `<repo>/.git/config` | the same | yes | yes | husky's `hooksPath` |
| npm-postinstall-repo | `<repo>/package.json` | the same | yes | yes | |
| npm-postinstall-dep | `node_modules/dep42/package.json` | the same | yes | yes | |
| npmrc-node-options | `~/.npmrc` and the file it loads | the same | no | yes | |
| python-pth | `site-packages/*.pth` | the same | no | yes | |
| python-sitecustomize | `site-packages/sitecustomize.py` | the same | no | yes | |
| path-shim | `~/.local/bin/git` | `/usr/local/bin/`, `/opt/homebrew/bin/` | no | yes | |
| ssh-authorized-key | `~/.ssh/authorized_keys` | the same | yes | yes | |
| ssh-localcommand | `~/.ssh/config` | the same | yes | yes | |
| ld-preload | `/etc/ld.so.preload` and the library | `DYLD_INSERT_LIBRARIES` by `launchctl setenv` (SIP limits it) | no | yes | |
| sudoers | `/etc/sudoers.d/` | the same | no | yes | |
| claude-hook | `~/.claude/settings.json` hooks | the same | yes | yes | |
| codex-notify | `~/.codex/config.toml` notify | the same | yes | yes | |
| agents-md | `<repo>/AGENTS.md` | the same | no | yes | |
| vscode-task | `<repo>/.vscode/tasks.json`, `runOn: folderOpen` | the same | no | yes | |
| direnv | `<repo>/.envrc` | the same | no | yes | |
| live-process | a loop started from `~/.cache/.s/keep.sh` | the same, or `launchctl submit` | no | the file yes, the process no | |

The list of places, with the macOS paths for each group, is `PLACES` in
[`proto/silo_persist.py`](proto/silo_persist.py). Its macOS side adds what Linux has no twin for:
`/Library/LaunchDaemons/`, launchd's override files (`/var/db/com.apple.xpc.launchd/disabled.*.plist`),
the Background Task Management store behind Login Items, `/etc/periodic/`, authorization plugins
(`/Library/Security/SecurityAgentPlugins/`), user records in `/var/db/dslocal/`, and the pre- and
post-actions of Xcode schemes (`*.xcscheme`).

## What put-back cannot undo

These are the record's to show, not put-back's:

- **What already ran.** An install script ran during `npm install`; putting back `package.json`
  stops the next run, not the last one. What it did is other facts of the same night.
- **What already left.** A key in `authorized_keys` may have been used while it was there; a
  `curl … | sh` fetched and ran whatever the host sent. That is the boundary's record (hosts,
  #406).
- **A program started from a place.** See the live process above. On a Mac, a job loaded with
  `launchctl bootstrap` stays loaded after its plist is removed, until it is unloaded or the user
  logs out (from Apple's documentation, to measure).

## For the Mac kit

A test 40, `persistence-put-back`, proposed and not written: in a macOS main, plant the macOS
column above, take checkpoints around it, and check with #370's reader and put-back:

1. Each item is in the file facts and on the list. The Background Task Management store is a binary
   file: does the reader's fact for it say which login item changed, or only that the store did?
2. A LaunchAgent loaded with `launchctl bootstrap gui/501`, then its plist put back as absent:
   `launchctl print gui/501/<label>` before and after a restart of main.
3. `crontab` after a put-back of `/usr/lib/cron/tabs/<user>`: does cron read the old file again?
   A put-back keeps the old modification time, and cron decides to reread by modification time.
4. A login item added with `osascript` or `SMAppService`, then put back: does it come back from the
   store on the next login?

## What was not verified

- No service manager runs in the test: systemd, cron, `at` and launchd never loaded the planted
  jobs. That a removed unit stays loaded is from the documentation, not measured.
- The dependency trees are generated, not installed by npm or pip. Real installs write more
  files; the list's 8 lines for a good night may grow with real packages that ship install scripts.
- The agent is a script, not a model. Which of these a real agent leaves on a real night is #377's
  and #371's ground, not this test's.
- macOS places are from documentation and known persistence lists, not measured.
- Which agent left each item (attribution) is not tested here; #399 does it from traffic.

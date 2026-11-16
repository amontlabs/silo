# Journeys

How Silo is used, step by step, and what works today.

- `[built]`: someone drove it as a user and it works.
- `[partial]`: it works with gaps, named in the step.
- `[missing]`: it does not exist.
- `[untested]`: the code and its docs say it exists; nobody has driven it as a user since.

Format, for agents: a journey is a `## ` heading with an `<!-- id: … -->` line under it, then
one paragraph that says what the user gets. A step is `- [status] Title. Detail`: a short title,
then what is true today. A change to what a user does or sees updates its step in the same merge.
The owner reads this file through [the dashboard](../OWNER-DASHBOARD.md).

Last driven: 2026-11-16, on the Dev build, from the command line. The app window, editors, GitHub
access, secrets and other devices are not driven yet.

## 1. You live in a computer
<!-- id: live -->

Your setup is inside main. Your tools open into it like a local folder.

- [partial] First run makes main. `silo up` makes main in 58 s. A rebuilt Dev app can open on "Silo could not load" when an older engine left its record behind. The app's own first run is not driven.
- [partial] Your agent is installed and opened. `silo agents install` puts Claude Code in main in 14 s, with Silo's tools connected. Opening it from the app is not driven.
- [missing] T3 Code connects in from the Mac. T3 Code stays on the Mac and works with the agent inside main.
- [untested] Your editor opens a folder in main. VS Code and Zed.
- [built] A terminal opens in main. `ssh main.silo-dev`.
- [untested] Your repositories are there. With GitHub access per repository.
- [untested] Secrets work without being readable.
- [untested] GUI automation runs inside. Desktop and computer use.
- [missing] atelier runs inside. The default recipe has no Node, so most tools need setup first.
- [missing] One recipe sets all of this up. A new main is ready to use.
- [partial] Making a recipe is convenient. A YAML file added with `silo recipe add`. There is no guided way.

## 2. A computer per task
<!-- id: task -->

You or your agent make another computer for a task, work there, and bring the result back.

- [built] The agent forks main without asking. 21 s, from inside main.
- [untested] The agent makes a disposable computer from a recipe. It works from the Mac; not driven from inside main.
- [partial] The agent works in the fork. Commands work. Reading its logs and screen is not driven.
- [partial] The diff shows everything that changed. Code, packages and settings are shown. Databases are not driven.
- [partial] Merging asks you, with the diff. The request is held and listed on the Mac. The app's Pending page was used once, by the owner.
- [built] Merge when main has not changed. Every change is applied and main keeps running.
- [untested] Merge when main has changed. Item by item, and you choose on conflicts.
- [partial] Undo, drop, replace main. Undo takes 19 s. A running fork must be stopped before it can be dropped. Replace main is not driven.
- [untested] You do each of these yourself. From the app or with `silo`.
- [untested] Several task computers run at once. Without colliding.

## 3. A task started from outside
<!-- id: outside -->

A GitHub event, a schedule or a program starts a task in a fresh computer and gets the result. It starts from a recipe or a checkpoint, so there is no setup to wait for.

- [built] A recipe is built once, then forked. 12 s per new computer.
- [built] A computer starts from a checkpoint you chose. 13 s, and it can run a command as it starts.
- [missing] One call starts a task. Which recipe or checkpoint, what to run, where the result goes. Today a caller chains several `silo` commands, a recipe and a command cannot go in one call, and the computer stays running afterwards.
- [untested] The same call works on another device. `--device`, a Linux host.
- [partial] The caller gets the result back. The exit status comes back. A report or files need a second command.
- [missing] The task's work is merged into main.
- [untested] GitHub Actions jobs run in fresh computers. `silo-runner`: the one trigger that exists, specific to GitHub jobs.
- [missing] Fjord starts a task from a GitHub event.

Example: someone opens a pull request. A computer starts from a checkpoint that already has the
repository installed, pulls the pull request, tries it, writes a report and sends it to you.

## Outside the three journeys
<!-- id: other -->

Features that exist and are not part of a journey above. Each is reviewed in turn: does it make the three journeys better?

- [untested] macOS computers.
- [untested] Container computers. On Linux hosts.
- [untested] Computers on another device. Connections, and hosting computers for other devices.
- [untested] The desktop viewer.
- [untested] Sharing a computer as a file. Export and import.
- [untested] Published websites and ports.
- [untested] Folder forks.
- [untested] Review requests and pull requests from a fork.
- [untested] The hosted reviewer service.
- [untested] Personal GitHub tokens.
- [untested] Network policy per computer.
- [untested] The audit log and notifications.

# Journeys

How Silo is used, step by step. Each step carries what is true today:

- `[built]`: someone drove it as a user and it works.
- `[partial]`: it works with gaps, named on the line.
- `[missing]`: it does not exist.
- `[untested]`: the code and its docs say it exists; nobody has driven it as a user since it was
  written.

Format, for agents: a journey is a `## ` heading with an `<!-- id: … -->` line under it. A step
is a list item that starts with its status. A change that alters what a user does or sees
updates the step in the same merge. The owner's dashboard reads this file.

Status: first draft of 2026-11-16, written from the docs. Every status here is `[untested]` or
`[missing]` until the audit drives each step on the Dev build.

## 1. You live in a computer
<!-- id: live -->

Your setup is inside main, and your tools open into it as easily as they open a local folder.
You stop working on your own machine.

```mermaid
flowchart LR
  subgraph device["Your Mac"]
    t3["T3 Code"]
    editor["Editor"]
    term["Terminal"]
  end
  subgraph main["main computer"]
    agent["Claude Code, Codex"]
    repo["Your repositories"]
    tools["atelier, GUI automation"]
  end
  t3 --> agent
  editor --> repo
  term --> main
```

- [untested] Install Silo; the first run makes main.
- [untested] Pick Claude Code or Codex; Silo installs it in main and opens it.
- [missing] T3 Code on the Mac connects to the agent inside main.
- [untested] Your editor (VS Code, Zed) opens a folder in main.
- [untested] A terminal opens in main.
- [untested] Your repositories are reachable from main with GitHub access per repository.
- [untested] Secrets work inside main without being readable there.
- [untested] GUI automation runs inside main (desktop and computer use).
- [missing] atelier runs inside main.
- [missing] One recipe sets all of the above up, so a new main is ready to use.
- [partial] Making a recipe: a YAML file added with `silo recipe add`. There is no guided way to
  create one.

## 2. A computer per task
<!-- id: task -->

You or your agent make another computer for a task, work there, and bring the result back.

```mermaid
sequenceDiagram
  participant You
  participant Agent as Agent in main
  participant Fork as Task computer
  Agent->>Fork: fork main
  Agent->>Fork: work
  Fork-->>Agent: diff
  Agent->>You: asks to merge, with the diff
  You-->>Agent: allow or deny
  Agent->>Fork: merge into main, or drop
```

- [untested] The agent in main forks main without asking you.
- [untested] The agent makes a disposable computer from a recipe.
- [untested] The agent runs commands in the fork and reads its logs and screen.
- [untested] The diff shows code, packages, settings, files and databases.
- [untested] Merging asks you in the Silo app, with the diff.
- [untested] Merge when main has not changed: every change is applied and main keeps running.
- [untested] Merge when main has changed: item by item, and you choose on conflicts.
- [untested] Undo a merge, drop a fork, replace main.
- [untested] You do each of these yourself, from the app or with `silo`.
- [untested] Several task computers run at once without colliding.

## 3. A task started from outside
<!-- id: outside -->

Something outside Silo starts a task in a fresh computer and gets the result. The computer starts
from a recipe or a checkpoint, so there is no setup to wait for.

Example: someone opens a pull request. A computer starts from a checkpoint that already has the
repository installed, pulls the pull request, tries it, writes a report and sends it to you. Its
work can also be merged into main.

```mermaid
flowchart LR
  trigger["A GitHub event,<br/>a schedule, a program"] -->|start a task| silo["Silo"]
  ready["recipe or checkpoint"] --> silo
  silo --> task["fresh computer<br/>here or on another device"]
  task -->|result| trigger
  task -.->|merge| main["main"]
```

- [untested] A recipe is built once and later computers fork from its checkpoint.
- [untested] A computer starts from a checkpoint you chose.
- [missing] One call starts a task: which recipe or checkpoint, what to run, where the result
  goes. Today a caller chains several `silo` commands itself.
- [untested] The same call works on another device (`--device`, a Linux host).
- [missing] The caller gets the result back: exit status, a report, files.
- [missing] The task's work is merged into main.
- [untested] GitHub Actions jobs run in fresh computers (`silo-runner`). This is the one trigger
  that exists, and it is specific to GitHub jobs.
- [missing] Fjord starts a task from a GitHub event.

## Outside the three journeys
<!-- id: other -->

Features that exist and are not part of a journey above. Each is reviewed in turn; see
[the product page](PRODUCT.md#everything-else). The audit completes this list.

- [untested] macOS computers.
- [untested] Container computers on Linux hosts.
- [untested] Computers on another device (Connections) and hosting computers for other devices.
- [untested] The desktop viewer.
- [untested] Sharing a computer as a file (export and import).
- [untested] Published websites and ports.
- [untested] Folder forks.
- [untested] Review requests and pull requests from a fork.
- [untested] The hosted reviewer service.
- [untested] Personal GitHub tokens.
- [untested] Network policy per computer.
- [untested] The audit log (Activity) and notifications.

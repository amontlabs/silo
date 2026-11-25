# Files that run on the Mac, and package age beyond npm and PyPI

For polarzero. This answers two gaps of the draft PR #350: G3, files that leave a computer and run
on your Mac, and G4, the package age rule for editor extensions, container images and skills. The
code is in [`prototypes/autorun-paths/`](prototypes/autorun-paths/); each script has its run
command below, and every number here was printed by one of them on 24 November 2026.

## Summary

**G3. Files that run on the Mac.** When a push from a computer changes a file that a tool on your
Mac runs by itself, such as a `SessionStart` hook in `.claude/settings.json`, Silo should hold the
push and show you each such change, and the receipt should list it in every case. The boundary
already reads the push when it swaps the Git secret: it takes the ref updates from the head of the
request, indexes the pack the request carries against a copy of the remote kept outside the
computer, and compares the two trees. That a tool runs a path is shipped data, like the hosts of a
registry, so the hold is a rule triggered by a fact. On the last 1,000 commits of each of eight
popular repositories, the hold would have stopped between none and 18 of them (1.8%), because it
reads which keys changed and not only which paths. A check on paths alone would have stopped up to
29%. I propose the hold ships switched on for every computer, with GitHub workflows listed but not
held. Sure of the mechanism, which the prototype runs end to end against the real `git` client;
fairly sure of the default, which rests on repositories of teams, not on agents' own pushes.

The finding that sizes the gap: Claude Code asks once per folder, never again. I measured it with
the installed Claude Code 2.1.331. In a folder you trusted once, a hook that a `git pull` changed
ran at the next launch with no prompt; `claude -p` ran the hook, the key helper and the MCP server
of a folder it had never seen, with no prompt at all.

**G4. Package age for extensions, images and skills.** The VS Code Marketplace, Open VSX, crates.io,
RubyGems, Quay and Docker Hub each state the publish time of a version themselves, and the boundary
can tie a download to its version from the request path, so the age rule can cover them. Container
images pulled by digest from GHCR and most other OCI registries, Go modules, Homebrew bottles, and
anything fetched from a Git URL (skills, plugins, `npx github:`, `uvx --from git+`) have no publish
time from the registry that the boundary can read without your own credentials, so the rule cannot
cover them and its worst case should say so. Sure of the table, which is from live calls; the
GitHub activity API, which could date a commit on GitHub, is untested here.

One finding answers an open question of `registry-traffic-read`: `npm install` asks for the short
form of package metadata, which has no publish times, so for npm the boundary has to ask the
registry separately; for crates.io the times are in the answer passing through.

## G3. Files that leave a computer and run on the Mac

### The case

Mini Shai-Hulud, 19 May 2026: 637 malicious versions of 317 npm packages in 22 minutes. Each one
wrote a `SessionStart` hook into `.claude/settings.json` that runs `node .claude/setup.mjs`, and a
`runOn: folderOpen` task into `.vscode/tasks.json`
([SafeDep](https://safedep.io/mini-shai-hulud-strikes-again-314-npm-packages-compromised/), HN 391
points). The Lightning worm did the same through PyPI and also pushed to the victim's own repository
([Semgrep](https://semgrep.dev/blog/2026/malicious-dependency-in-pytorch-lightning-used-for-ai-training/)).

In Silo, the install runs in main. Placeholders leave the stealer nothing to steal there. The hook is
now a file in the working tree. An agent that commits with `git add -A` takes it along with its
fix. The push goes to GitHub. Later you `git pull` on the Mac and run
`claude` in that folder, which you trusted months ago. The hook runs on the Mac, with your real
keys, outside every rule Silo enforces.

### Where code from a computer reaches the Mac

`computer-reached-over-ssh` says you open your editor on a folder of the computer. VS Code over SSH
runs tasks and most extensions inside the computer, so a `folderOpen` task opened that way runs in
the computer, under its rules. The Mac is reached in four other ways:

1. **A push, then a pull or clone on the Mac.** This is the common case above, and the one this
   design is about. Code leaves through Git (`code-leaves-through-git`, `what-a-fork-is-given`,
   `keeping-from-a-fork`).
2. **Files copied to the Mac** through a channel (`files-through-a-channel`; the direction toward
   the Mac is not designed yet, as `files-imported-from-your-mac` notes).
3. **GitHub Actions.** A workflow with `on: push` runs on GitHub's runners the moment the push
   lands, with the repository's secrets. It is not the Mac, but it is outside every rule too, and
   the same check finds it.
4. **The editor's link back.** `computer-reached-over-ssh` already notes that VS Code's server
   keeps a link back to the Mac. That is a channel question, not a file question, and is left out
   here.

### What runs by itself: the catalog

The data file is
[`autorun-paths.json`](prototypes/autorun-paths/autorun-paths.json): 36 entries, each with its
paths, the tool, what triggers it, what the tool asks first, and a source. It sorts each file into
one of three classes, because they call for different things:

- **runs**: the tool executes a command written in the file. Example: a hook in
  `.claude/settings.json`.
- **redirects**: the file changes which program a command you type runs. Example: `yarnPath` in
  `.yarnrc.yml` makes every `yarn` command run a committed JavaScript file.
- **instructs**: the file's text goes into the prompt of an agent on the Mac. Example: `CLAUDE.md`.
  It runs nothing by itself; it is a prompt injection into an agent that has your real keys.

The table keeps the entries that matter most; the data file has all of them.

| File | Tool | When it runs | What the tool asks first | Source |
|---|---|---|---|---|
| `.claude/settings.json`: `hooks`, `statusLine`, `apiKeyHelper`, `otelHeadersHelper`, `env` | Claude Code; Cursor also reads `.claude` hooks (CVE-2026-48124) | session start, each tool call, each status refresh | trust dialog once per folder; nothing when a parent folder is trusted; nothing in `claude -p` (measured below) | [permissions](https://code.claude.com/docs/en/permissions#what-runs-before-you-trust-a-folder) |
| `.mcp.json` | Claude Code | session start: each server's command | one approval per server, interactive only; none in `claude -p` (measured) | [mcp](https://code.claude.com/docs/en/mcp#project-scope) |
| `.claude/skills/`, `.claude/agents/`, `.claude/commands/` | Claude Code | a skill's frontmatter `hooks` run while it is used, and its `allowed-tools` run with no prompt | skill hooks: nothing, even before trust | [permissions](https://code.claude.com/docs/en/permissions#what-runs-before-you-trust-a-folder) |
| `.codex/config.toml`, `.codex/hooks.json` | Codex | session start: hooks, MCP servers | loaded only in a project you trusted; not asked again on change | [Codex config](https://learn.chatgpt.com/docs/config-file/config-advanced) |
| `.cursor/mcp.json`, `.cursor/hooks.json` | Cursor | agent start | MCP: asked again when an entry changes, since 1.3 (after MCPoison) | [Check Point](https://research.checkpoint.com/2025/cursor-vulnerability-mcpoison/) |
| `.gemini/settings.json` | Gemini CLI | session start: hooks, MCP servers, tool discovery command | Trusted Folders exists and is off by default | [Gemini CLI](https://geminicli.com/docs/cli/trusted-folders/) |
| `.vscode/tasks.json` with `runOptions.runOn: folderOpen`, and the tasks it `dependsOn` | VS Code, Cursor, Windsurf | the folder opens | VS Code: Workspace Trust, then one Allow per workspace; Cursor: Workspace Trust is off by default | [tasks](https://code.visualstudio.com/docs/debugtest/tasks), [Oasis](https://www.oasis.security/blog/cursor-security-flaw) |
| `*.code-workspace` | VS Code | holds `tasks` and `settings` like `.vscode/` | as above | [workspaces](https://code.visualstudio.com/docs/editing/workspaces/multi-root-workspaces) |
| `.vscode/settings.json` keys that name a program: `python.defaultInterpreterPath`, `git.path`, terminal profiles, `eslint.runtime`, `chat.tools.autoApprove`... | VS Code and extensions | on open | Workspace Trust restricts the known keys; any extension can add one | [Workspace Trust](https://code.visualstudio.com/docs/editing/workspaces/workspace-trust), [CVE-2025-53773](https://embracethered.com/blog/posts/2025/github-copilot-remote-code-execution-via-prompt-injection/) |
| `.devcontainer/devcontainer.json`: `initializeCommand` | Dev Containers | on the Mac itself, before the container is built and at each start | one click on "Reopen in Container" | [spec](https://containers.dev/implementors/json_reference/) |
| the same file: `postCreateCommand` and the others | Dev Containers | inside a container on the Mac, which gets your SSH agent and Git credentials | the same click | [credentials](https://code.visualstudio.com/remote/advancedcontainers/sharing-git-credentials) |
| a committed venv: `pyvenv.cfg`, `*.pth`, `sitecustomize.py` | VS Code Python extension | it picks a venv in the workspace and starts its interpreter; an `import` line in a `.pth` runs at every start | Workspace Trust | [site](https://docs.python.org/3/library/site.html), Pillar in B15 |
| `.idea/`, `.run/*.run.xml`, Gradle and Maven wrappers | JetBrains IDEs | startup tasks and build import on open | Trust Project per project; nothing under a trusted location | [JetBrains](https://www.jetbrains.com/help/idea/project-security.html) |
| `.envrc` | direnv | a shell enters the folder | `direnv allow`, tied to the file's path and content, so any change asks again | [rc.go](https://github.com/direnv/direnv/blob/master/internal/cmd/rc.go) |
| `mise.toml`, `.mise.toml`, `.tool-versions` | mise, asdf | a shell enters the folder: hooks and env sources | `mise trust` for files with code; tied to content only in paranoid mode | [mise trust](https://mise.jdx.dev/cli/trust.html) |
| `package.json`: `preinstall`, `install`, `postinstall`, `prepare` and the rest; `packageManager`; dependencies from Git, URLs or files | npm, pnpm, Yarn, Bun, corepack | `npm install` | nothing | [npm scripts](https://docs.npmjs.com/cli/v11/using-npm/scripts#life-cycle-operation-order) |
| `.yarnrc.yml` `yarnPath`, `.yarnrc` `yarn-path`, `.yarn/plugins/` | Yarn | every `yarn` command, even `yarn --version` (measured) | nothing | [yarnrc](https://yarnpkg.com/configuration/yarnrc#yarnPath) |
| `.pnpmfile.cjs` | pnpm | every `pnpm install` (measured) | nothing | [pnpmfile](https://pnpm.io/pnpmfile) |
| `.npmrc`: `script-shell`, `node-options`, `registry`, `git` | npm | `npm install` | nothing | [npm config](https://docs.npmjs.com/cli/v11/using-npm/config) |
| `.husky/`, `lefthook.yml`, `.pre-commit-config.yaml` | Git hook managers | your next commit or push, once installed (`prepare` installs husky on `npm install`) | nothing after install | [husky](https://typicode.github.io/husky/how-to.html) |
| `pyproject.toml` `[build-system]` with `backend-path`, `setup.py` | pip, uv | installing the project (`pip install -e .`, `uv sync`) | nothing | [PEP 517](https://peps.python.org/pep-0517/#in-tree-build-backends) |
| `build.rs`, `.cargo/config.toml`, `rust-toolchain.toml` | rust-analyzer, cargo, rustup | rust-analyzer runs build scripts and proc macros when the folder opens | Workspace Trust | [rust-analyzer](https://rust-analyzer.github.io/book/configuration.html) |
| `.gitmodules`, `.lfsconfig` | Git | a recursive clone fetches submodule URLs; LFS fetches from the server `.lfsconfig` names | recursion is off unless asked | [gitmodules](https://git-scm.com/docs/gitmodules) |
| `.github/workflows/` | GitHub Actions | the push itself, on GitHub's runners with the repository's secrets | nothing for a writer | [events](https://docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/events-that-trigger-workflows#push) |
| `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.cursor/rules/`, `.github/copilot-instructions.md`, `.agents/skills/` | every agent on the Mac | loaded into the prompt | nothing | [Pillar](https://www.pillar.security/blog/new-vulnerability-in-github-copilot-and-cursor-how-hackers-can-weaponize-code-agents) |

What a repository cannot do by itself, so that nobody builds a check for it:

- **`.git/hooks`, `.git/config` (`core.hooksPath`, `core.fsmonitor`, filter drivers).** A push
  carries objects and ref names, never these files, and Git refuses a tree entry named `.git`. A
  `.gitattributes` line such as `* filter=evil` does nothing unless the Mac's own config defines
  `filter.evil`. I checked: a fresh clone of such a repository has no `filter.evil` and runs nothing.
  Git LFS is the exception people have installed: its filter is defined globally, so `.gitattributes`
  plus `.lfsconfig` makes a checkout fetch from a server the repository names.
- **Makefiles, `justfile`, `npm test`, `launch.json`.** They run when you ask. They are out of this
  list, which is about what runs without your asking.

Two traps that are specific to the Mac, both in the prototype:

- **Case.** APFS, as the Mac formats it, is case-insensitive. A commit that adds `.CLAUDE/Settings.json`
  is another path for Git on Linux and the Claude Code settings file on the Mac. The check compares
  paths case-folded and Unicode-normalised. The same property made CVE-2024-32002 work: a crafted
  submodule wrote into `.git/` during a recursive clone on macOS and Windows
  ([Invicti](https://www.invicti.com/blog/web-security/git-scm-affected-by-cve-2024-32002)).
- **Links.** A committed symlink `.claude -> config/claude` makes `config/claude/settings.json` the
  file Claude Code reads. The check follows links committed at these locations.

### Measured: when Claude Code asks

[`cc_trust.py`](prototypes/autorun-paths/cc_trust.py) builds a repository whose
`.claude/settings.json` (a `SessionStart` hook, a `statusLine`, an `apiKeyHelper`, an
`otelHeadersHelper`) and `.mcp.json` (one server) each touch a marker file, then starts the
installed Claude Code 2.1.331 on it with a fresh home. No model is reached. Run:
`python3 cc_trust.py <scratch dir>`. It printed:

```
print mode -p: markers=['apikeyhelper', 'mcp', 'sessionstart'] exit=124
interactive, before any key: markers=[]
interactive, after Yes-I-trust: markers=[]
interactive, after Enter on the MCP dialog (default: without it): markers=['sessionstart', 'statusline']
trusted folder, relaunched after the hook changed, no key pressed: markers=['apikeyhelper', 'new_hook_after_pull', 'sessionstart', 'statusline']
relaunch asked about trust: False
```

What this means for you:

- The first launch in a new folder does protect you. The dialog says "This folder configures hooks
  that can run commands without asking, declared in .claude/settings.json", and its default is
  "No, exit". The MCP server gets a second dialog whose default is not to use it.
- After that, nothing is asked again. The trust is one flag per folder (`hasTrustDialogAccepted`
  in `~/.claude.json`), not tied to what the files say. A hook that arrives with a `git pull` into
  a folder you trusted runs at the next launch with no prompt. That is the case above, and it is
  the usual one: you trust your own repositories.
- `claude -p`, and any script or tool built on the SDK, runs the repository's hooks, key helpers and
  MCP servers with no dialog at all. The docs say so; the run confirms it. Trusting a parent
  folder, such as `~/code`, also covers hooks in every repository under it, by the same docs.

direnv is the counter-example: it hashes the path and the content of `.envrc`, so a changed file is
blocked until you allow it again. Cursor moved to the same model for MCP entries after MCPoison.
Claude Code, Codex, VS Code tasks and JetBrains trust a folder, not its content. A check outside the
Mac, on what changed, is the only place this is caught for them.

### Mechanism

#### Where Silo reads the change

A push over HTTPS is one `POST /<repo>.git/git-receive-pack`. Its body starts with pkt-lines, one
per ref update, then the pack:

```
00ae0000000000000000000000000000000000000000 36069829955d313ac59ef13a0490bebbfe0fba41 refs/heads/two\x00 report-status-v2 side-band-64k quiet object-format=sha1 agent=git/2.43.0
0000
PACK...
```

That line is the one the prototype boundary read from a real `git push` (shortened at the agent
string). Each update names the old commit, the new commit and the ref. The boundary already ends
the encrypted connection for this request, to swap the Git secret (`secret-placeholder`,
`ending-the-encrypted-connection`), so reading the body costs nothing new.

Three places could give the change. I compared them on the hostile agent:

| Where | How | What the agent can do | Verdict |
|---|---|---|---|
| The computer's disk, read from outside (a snapshot) | find the new commit in a snapshot and run `git diff` there | push from a repository in a folder Silo does not look in, then delete it; build the pack in memory with no repository on disk at all | not complete: a commit that is not found cannot be read |
| The pack in the request, alone | parse the pack | nothing, but a thin pack holds deltas against objects only the remote has | cannot be read without those objects |
| The pack in the request, indexed against a mirror of the remote kept outside the computer | `git index-pack --stdin --fix-thin --strict` into a quarantine folder whose base objects come from the mirror; then `git diff` of the two trees | nothing: the objects that leave are exactly the ones read | chosen |

The chosen way is what a Git server does for a `pre-receive` hook: Git's own receive-pack keeps
the objects of a push in a quarantine folder until the hook accepts them
([git-receive-pack](https://git-scm.com/docs/git-receive-pack#_quarantine_environment)). The mirror
is a bare clone of the remote, fetched by the boundary with the real secret before each push is
read, so it never comes from the computer. It costs a disk copy of each repository a computer
pushes to, and a fetch per push.

For a new branch the old side is all zeros. The base is then the parent of the first commit that
the remote does not have yet. Every new commit is also read on its own against its parent, because
a hook added in one commit and removed in the next is gone from the tip but runs on a checkout of
the middle commit (`git bisect` does that).

The comparison never runs code from the repository. `git diff` is called with `--no-ext-diff`
and `--no-textconv`, with no user or system Git config, so a driver that `.gitattributes` names
cannot run in the boundary.

#### What is compared

A path in the list is not always enough. `package.json` changes in most commits, and only a change
to a lifecycle script runs anything on `npm install`. So each entry of the data file names a content
check:

- `json-keys`: the named keys, such as `hooks` and `apiKeyHelper` in `.claude/settings.json`. The
  file is read as JSON with comments and trailing commas, as VS Code reads it. A file that cannot be
  read counts as changed in full.
- `vscode-tasks`: the tasks with `runOn: folderOpen`, and the tasks they depend on.
- `vscode-settings`: every key that changed, marked when it is a known key that names a program.
  Only marked keys can hold.
- `package-json`: lifecycle scripts, hook-manager keys, and dependencies fetched from Git or a
  URL. A `packageManager` bump such as `pnpm@12.23.0` is listed and does not hold, because corepack
  fetches that version from the npm registry; one that names a URL holds. A `file:` dependency on a
  folder of the same repository is not listed: that folder's own `package.json` is checked.
- `yaml-keys`: named top-level keys of a YAML file. In `pnpm-workspace.yaml` these are the ones
  that decide which dependencies may run install scripts, such as `onlyBuiltDependencies`; a
  version bump in a pnpm catalog is not listed.
- `version-file`: `.nvmrc`, `.python-version` and the like are listed and do not hold when they
  hold a plain version; a `rust-toolchain.toml` with a `path` holds.
- `frontmatter`: a skill's or subagent's `hooks`, `allowed-tools` and `mcpServers`. A change to its
  text only is listed as instructs.
- `beside`: a `build.rs` is a Cargo build script only next to a `Cargo.toml`, the same test
  `ecosystem-data` uses for `node_modules`. uv has test files named `build.rs` that are not.
- `list`: listed, never held. `.gitattributes` is one: its drivers run only if your own Git
  config defines them.
- `any`: every change counts.

One more case comes from the worms themselves. Mini Shai-Hulud's hook runs `node .claude/setup.mjs`.
On its second visit it only needs to change `setup.mjs`, and no listed file changes. So the check
also reads the commands of every auto-run entry in the new tree, changed or not, and lists any file
of the repository that such a command names literally and that changed.

#### Fact, data or opinion

That a push changes `hooks` in `.claude/settings.json` is a fact: the boundary read both trees
from the objects that leave. That Claude Code runs that key at session start is not something Silo
sees; it is knowledge about a tool, written down. The plan already keeps that kind of knowledge as
shipped data: the hosts of a registry in a rule (`rule-carries-its-data`), the folders a rebuild
brings back in `ecosystem-data`, where a program reads its instructions in
`agent-program-data-file`. It is not an opinion, because it judges nothing in the content; it says
what a tool does with a file. It can be incomplete: a tool that is not in the list is a false
negative, and the record names the version of the list that was used.

So the hold is a rule, triggered by a fact ("this push changes a path the rule's list names, in a
key the list names"), carrying the list as its own data. It is exact against its list and may do
what you wrote in it (`what-a-rule-may-do`), which here is a hold for your answer
(`held-requests-and-asks`).

#### List only, or hold

The receipt lists these changes in every case, as facts. Whether a rule also holds the push depends
on what it costs you, which I measured next.

### Measured: how often ordinary commits would be held

[`history.py`](prototypes/autorun-paths/history.py) runs the same check on each of the last 1,000
first-parent commits of a repository (in a repository that merges pull requests by squashing, one
commit per merged pull request, which is about one push). Run:
`python3 -I history.py <bare clone> 1000`. Clones were shallow, with blobs over 200 kB left out.

Each cell is the share of the 1,000 commits (874 for `anthropics/claude-code`, whose shallow
clone had fewer). "Path only" counts commits that touch a listed path that runs or redirects, with
no content check: the hold GitHub's "Restrict file paths" would give. "Held, for the Mac" is the
hold proposed here. "Held, workflows too" adds `.github/workflows/`. The last column is measured
apart, with [`editor-configs.json`](prototypes/autorun-paths/editor-configs.json): configuration
files that are code, which editor extensions load on open.

| Repository | Listed in the receipt | Path only | Held, for the Mac | Held, workflows too | Editor configs |
|---|---|---|---|---|---|
| anthropics/claude-code | 8.9% | 2.4% | 1.4% | 8.1% | 0% |
| astral-sh/uv | 29.8% | 7.4% | 0.6% | 29.3% | 0% |
| facebook/docusaurus | 23.3% | 28.3% | 1.1% | 20.2% | 3.9% |
| microsoft/vscode | 10.5% | 5.0% | 0.8% | 5.8% | 0.7% |
| openai/codex | 11.4% | 0.2% | 0% | 11.2% | 0% |
| pydantic/pydantic | 26.5% | 9.6% | 1.8% | 25.9% | 0% |
| vercel/next.js | 21.5% | 15.7% | 1.3% | 18.0% | 4.5% |
| vitejs/vite | 9.6% | 29.4% | 0.6% | 8.5% | 13.0% |

What the numbers say:

- **The content checks are what make a hold bearable.** On vite, 294 of 1,000 commits touch a
  listed path, nearly all of them a `package.json` or `pnpm-workspace.yaml` version bump; 6 change
  something that runs. On docusaurus, 283 against 11.
- **For files that run on the Mac, a hold costs about one merged change in a hundred**, between 0
  and 1.8% across the eight repositories. These are teams that edit their tooling a lot.
- **GitHub workflows are a different size.** They change in 5 to 29% of commits. They run on
  GitHub, not on the Mac, and G2 is about what reaches GitHub. They are listed, and not in the
  proposed hold.
- **Editor configuration files that are code would add up to 13%** (vite, which edits its lint and
  test configs often). They stay out of the hold and out of the list; the receipt's standing note
  covers them (see the bad paths).

What the holds were, read one by one for the ones I looked at:

- docusaurus, `bd5a73333` "chore(ai): format the files Claude Code edits with oxfmt": a
  `PostToolUse` hook `node "$CLAUDE_PROJECT_DIR/admin/scripts/claude/formatEditedFile.js"`. A
  legitimate change, and exactly the kind you would want to see before it runs on your Mac.
- vscode, `ade7f1b62` and `d41c931ba`: changes to `build/npm/postinstall.ts` and
  `build/npm/preinstall.ts`, which `package.json` runs on every `npm install`. Found by the step
  that follows a command to the file it names.
- next.js, `ef97226c9` "Add a 48-hour Cargo dependency cooldown": `.cargo/config.toml`. The team
  added its own package age rule, inside the tool, where an agent can undo it.
- pydantic: `.pre-commit-config.yaml` (10 commits), `pyproject.toml` build settings (7),
  `.cargo/config.toml` (2) and `build.rs` (2).
- claude-code: `.devcontainer/devcontainer.json` (5) and the frontmatter of `.claude/commands/`
  (7).

A first version of the checks, before the refinements in "What is compared", held 7.7% of vite and
5.5% of docusaurus for the Mac; each refinement came from reading those holds. That list of
refinements is not closed: someone will find the next routine change that holds.

The check took 3.5 seconds on a vscode commit (21,629 files in the tree) on this shared container,
mostly in matching every path of the tree against the list, which a real build would index.

### Measured: the prototype end to end

[`push_boundary.py`](prototypes/autorun-paths/push_boundary.py) stands in for the boundary. It
serves a bare repository through `git http-backend`, the CGI program Git ships, reads each
`git-receive-pack` request, indexes the pack against a mirror, runs
[`autorun.py`](prototypes/autorun-paths/autorun.py) on each ref update and on each new commit, and
either passes the original body on unchanged or answers with a report-status that refuses each ref
as held. [`make_worm_repo.sh`](prototypes/autorun-paths/make_worm_repo.sh) builds four commits: a
normal project, a benign change, the worm (a `SessionStart` hook, a `folderOpen` task, a
`postinstall` script and the payload `.claude/setup.mjs`), and a second visit that changes only the
payload. Run: `bash demo_push.sh <scratch dir>`. What the real `git` client printed:

```
=== push the benign commit
    To http://127.0.0.1:9203/demo.git
       8dfacd9..2eb6b94  2eb6b94... -> main

=== push the worm commit
    remote: Silo held this push: it changes files that tools on your Mac run on their own.
    remote:   .claude/settings.json: Claude Code (...): hooks added; new command: node .claude/setup.mjs
    remote:   .vscode/tasks.json: VS Code, Cursor, Windsurf, VSCodium: task 'env-setup' runs on folder open (added): node .claude/setup.mjs
    remote:   package.json: npm, pnpm, Yarn, Bun, corepack: scripts.postinstall added
    remote:   .claude/setup.mjs: Claude Code (...): named in a command of .claude/settings.json
     ! [remote rejected] 21f628d... -> main (held by Silo: changes files that run on their own)
    upstream main is now: 2eb6b94

=== push a hook added then removed
    remote:   .claude/settings.json: Claude Code (...): hooks added; new command: node .claude/setup.mjs
     ! [remote rejected] hide -> hide (held by Silo: changes files that run on their own)
```

The new branch and the push of three commits at once were held the same way. The benign push
listed `.vscode/settings.json` (`editor.tabSize`, which names no program) and passed.
[`test_tricks.sh`](prototypes/autorun-paths/test_tricks.sh) checks the three ways around a literal
path match. All three were caught:

```
--- 1. .CLAUDE/Settings.json
HOLD [runs] .CLAUDE/Settings.json added      - hooks added; new command: node x.mjs
--- 2a. adding the link
HOLD [runs] .claude added (symlink)           - link to config/claude: the files there count as .claude
--- 2b. changing the file behind the link
HOLD [runs] config/claude/settings.json changed (read on the Mac as .claude/settings.json)
--- 3. new branch from zero
HOLD [runs] .claude/settings.json added
```

`autorun.py` alone, on the worm repository: `python3 -I autorun.py <repo> <old> <new>` prints the
same findings with the tool, the trigger and what the tool asks first, and exits 1 when a rule
would hold.

In the prototype a held push is refused at once. In Silo it would wait for your answer
(`held-requests-and-asks`), and the limit of `hold-limit` would end it. Git's HTTP client waits
for the answer to its POST, so a held push that you allow within the limit goes through on the
agent's same `git push`. How long Git waits before it gives up is to find out per client; the
`http.lowSpeedTime` setting is the agent's, so Silo cannot count on it.

### Prior art

- **GitHub push rulesets** have "Restrict file paths": "Prevent commits that include changes in
  specified file paths from being pushed to the repository"
  ([GitHub docs](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets)).
  It blocks; it does not ask. It needs you to know the paths, it does not look at content, and it
  covers only repositories on GitHub with that ruleset. What differs: Silo ships the list, reads
  the keys inside, and asks you.
- **GitLab push rules** and **pre-receive hooks** on any Git server work on the same quarantine
  model the prototype uses. They run on the server, after the code has left the computer.
- **direnv** ties its allow to the content of `.envrc`; **Cursor** asks again when an MCP entry
  changes. They are the right model inside the tool, and only those two tools do it.
- **CODEOWNERS** on `.claude/` or `.vscode/` asks for a review of a pull request. It does nothing
  for a direct push or for a branch you pull before review.

### Bad paths

| What happens | Outcome | Defined handling |
|---|---|---|
| The push goes over SSH (`git@github.com:`) | The boundary does not read the SSH stream | The rule cannot establish what the push carries. As in `package-age-rule`, what cannot be established does not pass: here it is held for you. Silo lists "a push over SSH, not read". Once SSH keys are swapped at the boundary (open in `secret-placeholder`), it is read like HTTPS |
| The agent writes the file through the GitHub API (`PUT /repos/{o}/{r}/contents/.claude/settings.json`, or the Git data API: `POST /git/trees`, `PATCH /git/refs`) | No `git-receive-pack` request at all | The rule also names these API requests, whose paths and bodies carry the file paths. Any other write to `api.github.com` that Silo does not read as a change of files is listed as such. To build with G2 |
| The pack cannot be indexed (corrupt, a hash Silo does not know, `--strict` refuses it) | Nothing can be read | Held with the reason "the push could not be read". The prototype does this |
| The mirror cannot be fetched (remote down, secret refused) | Thin packs cannot be completed | Held, same reason |
| A tool not in the list, or a key not in an entry | False negative | The list is versioned and shipped with Silo; the record names the version used. A new tool means a new entry, not new code |
| A command that runs project code indirectly (`npm run setup`, a husky hook that runs `lint-staged`, which runs `eslint.config.js`) | The literal-path step sees only files named in the command | Defined limit, said in the receipt: once a repository has an auto-run entry that runs project code, any change to that code changes what runs. The receipt lists these standing entries |
| Configuration files that are code and that extensions load on open (`eslint.config.js`, `tailwind.config.js`, `jest.config.js`) | Not in the shipped list | Measured in the table above (up to 13% of commits); not proposed for the list or the hold |
| You opened the repository in Claude Code, ran `npm install` and the hook ran, all before Silo's hold | Not possible for a push Silo holds: the commit is not on the remote | For a push you allowed, the receipt keeps the list, so you can look before you pull |
| Noise: a team that edits its tooling often | A hold on each change that runs | Measured: up to 1.8% of commits for the Mac. Workflows, at 5 to 29%, are listed and not held. A rule of yours can allow a path you change every week, such as one hook script |
| A file brought to the Mac through a channel | Not a push | The same check runs on the paths of the files the channel carries, which Silo reads because it carries them |

### Recommendation

1. **Change `what-a-receipt-holds`.** After "the files that changed, grouped, with what is only
   here;" add: "among them, as facts, the changes to files that a tool on your Mac runs without your
   asking, each with the tool, when it runs and what that tool asks first;". The receipt of the case
   above then reads: "`.claude/settings.json`: hook added, `node .claude/setup.mjs`; Claude Code runs
   it at session start; it asks only the first time you open the folder".
2. **New decision in `rules`**: asks "What does Silo know about the files a tool runs by itself?";
   answer: "Silo ships, as data, the files of a repository that a tool runs without your asking,
   such as `.claude/settings.json` and `.vscode/tasks.json`, each with the tool, what in the file
   runs, when, and what the tool asks first. For an agent program it is in that program's data file
   (`agent-program-data-file`); for a package manager in its ecosystem file (`ecosystem-data`); for
   an editor or a shell tool in a data file of its own of the same kind. Paths are compared as your
   Mac's disk compares them: without case, and through links."
3. **New feature in `rules`**: name "Pushes that change files that run on your Mac"; what "A rule
   holds a push that adds or changes a file a tool runs by itself, and shows you each change. The
   boundary reads what the push carries outside the computer, from the objects that leave, compared
   with a copy of the remote that Silo keeps. A push whose content cannot be read is held too."
   Switched on for every computer, main included, alongside `merges-and-deploys-held`. Files of
   the instructs class and GitHub workflows are listed, never held by this rule. Since hold beats
   allow (`allow-hold-or-deny`), a rule that allows a fork's pushes does not get past it.
4. **In `files-through-a-channel`**, when the direction toward the Mac is designed: the same list
   is shown for files brought to the Mac.

I challenge nothing in force. `what-a-fork-is-given` already denies a fork's push until you allow
it; this gives that ask the one line it most needs, and covers main, whose pushes nothing asks
about today.

## G4. Package age for extensions, container images and skills

### The case

The GitHub breach of May 2026 came through Nx Console 18.95.0 on the VS Code Marketplace, live for
about 11 to 18 minutes ([HN](https://news.ycombinator.com/item?id=48207660)). I checked: the
Marketplace no longer lists 18.95.0: its list goes from `18.94.0` to `18.100.0`. In Silo,
VS Code's server runs inside the computer (`computer-reached-over-ssh`), and extensions that touch
the workspace install and update there, so their download crosses the boundary. An age rule of 7
days that covered the Marketplace would have denied 18.95.0 for every one of its minutes.

### How the boundary reads the age, per source

Every row comes from calls made on 24 November 2026 from this container; the command is in the row.
"Publish time" means the time the registry itself recorded when the version was published.

| Source | Request that downloads a version | Where the publish time is | Who sets it | Rule or report |
|---|---|---|---|---|
| VS Code Marketplace | `GET https://{pub}.gallerycdn.vsassets.io/extensions/{pub}/{name}/{version}/{epoch-ms}/Microsoft.VisualStudio.Services.VSIXPackage`, or `.../_apis/public/gallery/publishers/{pub}/vsextensions/{name}/{version}/vspackage` | `lastUpdated` per version from `POST /_apis/public/gallery/extensionquery` (Nx Console 18.104.0: `2026-11-18T16:25:56Z`); the CDN path also carries the upload time in ms (`1795018258604` = 16:10:58), and a wrong number gets 404 | the Marketplace | **rule**: the version is in every download path; ask `extensionquery`, or read the CDN path's number |
| Open VSX | `GET /api/{ns}/{name}/{version}/file/{ns}.{name}-{version}.vsix`, which redirects to `openvsx.eclipsecontent.org/{ns}/{name}/{version}/...` | `timestamp` in `GET /api/{ns}/{name}/{version}` (18.104.0: `2026-11-18T16:11:05Z`) | Open VSX | **rule**; both hosts named |
| npm (MCP servers via `npx -y`) | `GET /{pkg}/-/{pkg}-{version}.tgz` | `time[version]` in the full packument; **the abbreviated packument that `npm install` asks for (`Accept: application/vnd.npm.install-v1+json`) has no `time`** | npm | **rule**, with a separate request for the full document |
| PyPI (MCP servers via `uvx`) | `GET files.pythonhosted.org/packages/.../{name}-{version}...`; the path is a hash, the file name carries the version | `upload-time` per file in the JSON simple index (PEP 700), which pip and uv ask for | PyPI | **rule**, read from the answer passing through |
| crates.io | `GET static.crates.io/crates/{name}/{name}-{version}.crate` | `pubtime` per version in the sparse index file `index.crates.io/se/rd/serde`, which cargo fetches first; all 316 serde versions carry it. The download's `Last-Modified` (2026-06-28 for 1.0.219, published 2025-03-09) is not the publish time | crates.io | **rule**, read from the answer passing through |
| RubyGems | `GET rubygems.org/downloads/{name}-{version}.gem` | `created_at` in `GET /api/v1/versions/{name}.json` or `/api/v2/rubygems/{name}/versions/{v}.json`; the compact index has none | RubyGems | **rule**, with a separate request |
| Go modules | `GET proxy.golang.org/{module}/@v/{version}.zip` | `Time` in `@v/{version}.info` is the commit time from Git (cobra v1.8.0: `2023-11-02T15:23:08Z`, the same as `git log`), set by whoever made the commit. The proxy's own first-seen time is only in the `index.golang.org` feed (`2023-11-04T21:07:13Z`), found after paging 16 times through it | `.info`: the publisher; the feed: the proxy | **report** from `.info`; a rule only if Silo follows the whole feed |
| Docker Hub | `GET registry-1.docker.io/v2/{repo}/manifests/{tag}`, then by digest | `tag_last_pushed` and per-image `last_pushed` from `hub.docker.com/v2/repositories/{repo}/tags/{tag}` (node:22-alpine: `2026-11-12T02:38:14Z`, digest `sha256:1314fb…`, the same `Docker-Content-Digest` that `registry-1.docker.io` returned for the tag) | Docker Hub | **rule** for a pull by tag (check the digest matches); a pull by digest has no reverse lookup (node has 9,315 tags) |
| Quay | as Docker Hub | `start_ts` per tag from `quay.io/api/v1/repository/{ns}/{repo}/tag/` | Quay | **rule** for a pull by tag |
| GHCR and other OCI registries | `GET /v2/{name}/manifests/{tag or digest}`, then blobs by digest | nothing in the registry's answers. `created` in the image config and `org.opencontainers.image.created` are written by whoever built the image: `ghcr.io/astral-sh/uv:latest` says `2026-11-17T09:18:51Z`, `gcr.io/distroless/static-debian12` says `1970-01-01T00:00:00Z`. GitHub's packages API has `created_at` per version but answered 403 without a token | the builder | **report**; a rule for GHCR only with a token that reads packages |
| Homebrew | bottles: `GET ghcr.io/v2/homebrew/core/{name}/blobs/sha256:…`, by digest; the formula JSON from `formulae.brew.sh` maps version to digest | `org.opencontainers.image.created` in the bottle's index (jq 1.8.2: `2026-06-20T15:46:58Z`), written by Homebrew's own build | Homebrew's build, which is the registry here | **report**, close to the registry's word; homebrew-core is also reviewed. Third-party taps are Git repositories (below) |
| Claude Code plugins and marketplaces, skills from a Git URL, `npx github:…`, `uvx --from git+…` | `git-upload-pack` (the `want <commit>` lines name the exact commit), or `codeload.github.com/{o}/{r}/tar.gz/{ref}` | commit dates are set by the author. GitHub's "list repository activities" API records each push with its time and before and after commits ([docs](https://docs.github.com/en/rest/repos/repos#list-repository-activities)); not tested here, because this container's proxy answers `api.github.com` for its own use | the author; GitHub for the activity | **report** today; see the worst case below |

Three things in the table that change the design:

- **The version is in the download path for every package registry and extension store.** The
  boundary ties a download to its version without guessing, which is what `registry-traffic-read`
  asks. Container images and Homebrew bottles are fetched by digest, so their tie goes through the
  manifest or formula request that came first.
- **For npm, RubyGems and the VS Code Marketplace, the age is not in what the client fetched.** The
  boundary asks the registry itself, once per version, and can keep the answer, since a publish
  time does not change. That answers the "to find out" of `registry-traffic-read` for those three:
  asked separately.
- **`created` in an image is the builder's word.** distroless sets it to 1970 on purpose, for
  reproducible builds. A rule that read it would let every distroless image through as 56 years
  old, and an attacker can write any date. It is a report, and `package-age-rule` already says what
  happens when the age cannot be established: denied.

### Mechanism

The same rule, `package-age-rule`, with more registries named in its data
(`rule-carries-its-data`): for each, the hosts, how to read the version from the request, and where
to read its publish time. For the Marketplace that is: hosts `marketplace.visualstudio.com`,
`*.gallery.vsassets.io`, `*.gallerycdn.vsassets.io`; version from the path; publish time from
`extensionquery`. For Docker Hub: hosts `registry-1.docker.io`, `auth.docker.io`,
`production.cloudflare.docker.com`; the tag from the manifest path; the digest from the
`Docker-Content-Digest` header of the answer; the time from the Hub tag API, used only when its
digest is the one served.

For OCI registries with no publish time, and for Git, one more fact can serve: the first time the
boundary saw that exact digest or commit served. A digest or a commit id names its content, so if
Silo saw it served on 1 November, it existed on 1 November, and on 24 November it is at least 23
days old. That is a lower bound, never more than the true age, so a rule may use it. Proposed: the
age of a version is the oldest of what its registry states and when Silo first saw it served. It
counts only what a remote actually served through the boundary, not an id the agent typed, because
an agent can ask for any id.

What it costs: the first time any computer uses a version, Silo's own sight gives it age zero, so
on a source with no registry time it is denied until it is old enough in Silo's record, unless you
allow it by name. That is the cost `package-age-rule` already names for a fix published today. It
opens no new way around the rule: an attacker who waits 7 days in Silo's record could as well have
waited 7 days on npm.

### Bad paths

| What happens | Outcome |
|---|---|
| The agent pulls an image by digest from Docker Hub (`node@sha256:…`) | No publish time can be established, so it is denied under the rule, as `package-age-rule` says. A digest pinned in a Dockerfile for months is denied too. Cost: you allow that digest by name once |
| A tag is moved to a new digest after Silo read `tag_last_pushed` | The digest check catches it: the served digest is not the one the API dated, so it is denied |
| The agent downloads a VSIX from GitHub releases instead of the Marketplace | Not a registry the rule names; the network rules decide (`package-age-rule` says this already) |
| Docker Hub rate-limits the boundary (this container got a 429 from the registry once; anonymous pulls are limited to 100 an hour by its `ratelimit-limit` header) | The age cannot be established, so denied. A cache of publish times by digest makes repeats free |
| The Marketplace removes a malicious version | The rule never needed to know; it was younger than the age |
| The agent installs a skill from a Git URL pushed a minute ago | Passes the age rule, which does not cover it. Network rules decide whether the host is reached |

### What the rule's worst case should say for Git-URL skills

`package-age-rule` says a package fetched from anywhere else is not covered. For skills that sentence
needs a case, because skills are where people now get them. Proposed sentence for its worst case:
"A skill, plugin or MCP server fetched from a Git repository or an archive URL, such as
`/plugin marketplace add owner/repo` or `npx github:owner/repo`, has no publish time a registry
states, so the age rule does not cover it: a commit pushed a minute before is fetched. The record
still names the repository and the exact commit, which the boundary reads in the fetch."

Two ways to narrow it, to find out before anything is promised: GitHub's activity API, which dates
each push to a branch of a public repository, could give a time GitHub itself recorded, for
repositories on GitHub; and Silo's own first sight of a commit is a fact, but only of when Silo saw
it, so a rule on it would deny every skill the first time.

### Recommendation

1. **Change the data of `package-age-rule`'s registries** (`minimum-package-age`) to name the VS
   Code Marketplace, Open VSX, crates.io, RubyGems, Docker Hub by tag and Quay by tag, each with
   where its publish time is read, as in the table.
2. **Answer the "to find out" of `registry-traffic-read`**: "For npm, RubyGems and the VS Code
   Marketplace the age is asked of the registry separately, once per version; for PyPI and
   crates.io it is read from the answer passing through."
3. **Add to `package-age-rule`'s notes the worst case above**, and: "An image's `created` date is
   set by whoever built it, so it is never used as its age."
4. **New decision in `boundary`**, asks "Does Silo's own first sight of a version count toward its
   age?"; answer: "Yes. The age of a version is the oldest of what its registry states and when
   the boundary first saw that exact digest or commit served. It is never more than the true age,
   so the rule can use it where no registry states a time: images by digest, Git commits." With it,
   Git-URL skills come under the rule, at the cost of a wait or a yes on first use. Without it, the
   worst case above stands as written.
5. **G3 meets G4 at `.mcp.json`.** A committed `npx -y some-server@latest` runs on the Mac at every
   session start, outside any age rule, because the Mac has no boundary. That is one more reason the
   G3 list includes `.mcp.json`.

## Open questions

- How long Git's HTTP client, and the agents' own wrappers, wait on a held push before giving up.
- Whether GitHub's activity API answers without a token for public repositories, and how far back.
- Whether a list of files that run by themselves should also go to the Mac side: a small check
  before `git pull` would cover pushes that never went through Silo. That would be a piece of Silo
  on the Mac, which the plan has not drawn.

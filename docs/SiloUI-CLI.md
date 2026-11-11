# The `silo` command line and MCP server

`silo` drives computers and forks through the running Silo engine, on the device and from inside
a computer. It lives in
`app/SiloUI/cli` (crate `silo-cli`, binary `silo`) and talks to `engine.sock` with
`silo-client`, using the typed request constructors from `silo-api`.

## Install

The Silo app carries `silo` at `Contents/MacOS/silo` (built from this crate in the same
profile as the app and copied next to the engine). Production builds sign it with hardened
runtime and no entitlements (notarized in `developer-id` mode); Dev builds keep Tauri's ad hoc
signature. Every time the engine starts from an app bundle, whether or not Connections are on,
it links the command into `~/.local/bin`:

| Build | Link in `~/.local/bin` |
| --- | --- |
| Silo (production) | `silo` |
| Silo Dev | `silo-dev` |

The names come from `host/src/channel.rs` (`cli_link_name`); a Dev build never touches the
production link. Silo replaces only a link whose target is a `silo` inside a Silo, Silo Dev or Silo Engine
app. A file or link of your own with the same name (including one to another `silo` or a
dangling link) is left alone, and Connections's status shows that the command is not on PATH. Put `~/.local/bin` on `PATH`. Without
the app, build the crate with `cargo build --release -p silo-cli`.

## Choices

| Need | Choice |
| --- | --- |
| Argument parsing | `clap` (derive) |
| YAML | `serde_yaml_ng` (maintained fork of `serde_yaml`); `deny_unknown_fields` gives unknown-key errors with a line number |
| MCP | `rmcp`, the official Rust SDK (`#[tool]` macros, stdio transport) |
| Engine access | existing `silo-client` and `silo-api`; no new protocol code |

Gaps: none known. Jobs are awaited by polling `operations.job.get`, as the app does.

## Engine socket

In order: `--socket PATH`, `SILO_SOCKET`, then the build channel's own
`~/.silo/engine.sock` (a debug build uses `~/.silo-dev/engine.sock`, like the app).
A socket outside the build's own home is refused unless `--allow-other-channel` is given;
a debug build never uses `~/.silo`, even with the flag. If nothing listens, the command
asks you to open Silo.

If a wait ends before its job does (connection lost, Ctrl-C), the job id is printed and
`silo job <id>` waits for it again. `exec` accepts at most 180 seconds (the engine's
limit; `exec --stream` has none) and returns the command's exit code, or 128 plus the signal number.

## Commands

Every command accepts `--json`. Progress goes to stderr; a failed job exits non-zero with
the engine's message. Exit codes: 0 done, 1 failed, 2 refused (a merge or replace the engine
refused; nothing changed), 3 a merge that needs answers ([Merge](#merge-and-replace-main)), 4 the
owner denied it, 5 the owner has not answered, 6 the request ended without an answer
([Approvals](#approvals)).

| Command | What it does |
| --- | --- |
| `silo list [--all]` | computers and their state; `--all` also shows the hidden throwaways (`silo-baseline-*`, `silo-build-*`, `silo-export-*`) |
| `silo status <computer>` | one computer: size, checkpoints, whether its desktop is on, a fork's quiet state, assigned secrets and GitHub access |
| `silo resources` | what the device has left: memory, disk, how many computers run, and whether it is running short (the policy's [limits](SiloUI-POLICY.md#limits)). `silo fork`, `start` and `create` print a warning sentence under their result when it is |
| `silo access [<computer>]` | what a computer holds: the names of its secrets and the hosts they reach, GitHub access, published ports, SSH access and its network policy. Without a name, every computer the caller can see. Never a secret's value |
| `silo secrets list` | secret names and the computers they are assigned to (never values) |
| `silo secrets set NAME [--computer C]... [--domain D]...` | add a secret or replace its value; the value is read from stdin, never an argument |
| `silo secrets assign NAME COMPUTER` / `silo secrets unassign NAME COMPUTER` | change which computers have a secret |
| `silo network policy <computer> [--mode open\|allowlist\|closed] [--allow HOST]... [--remove HOST]... [--preset NAME]...` | show or change what a computer may reach; see [Network policy](#network-policy) |
| `silo create <name> [--linux\|--macos\|--container] [--cpus N --memory GiB --disk GiB] [--no-desktop]` | create a computer. `--no-desktop` starts no desktop in a Linux computer. `--container` makes a Linux computer that runs as a container. A container computer shares the host's kernel, so it is less isolated than a VM; use it for a dev-only machine with nothing sensitive on it. Linux hosts only. Its checkpoints save the disks only (never memory), also while it runs, and `silo fork`, `diff`, `merge`, `undo`, `export` and `import` work as for a VM. Its disk size is reported, not a bound: it shares the host's disk. Below 5 GiB free on the host, Silo refuses to start it, checkpoint it or fork it, and stops running container computers that are over their size, largest first ([the disk](SiloUI-ENGINE.md#container-computers-optional)). |
| `silo start <computer>` / `silo stop <computer>` | lifecycle |
| `silo desktop on\|off <computer>` | turn a running Linux computer's desktop on (and start it) or off (and stop it); forks made later inherit the choice, and `--gui` and the computer-use tools refuse a computer with it off. A macOS computer is its own display and has no such setting |
| `silo retry-setup <computer>` | finish the setup of a macOS computer that ended `failed` before it was ready |
| `silo up [--recipe R]` | make `main` from a stored recipe (default `linux-dev`) if needed, start it, publish the recipe's ports on it, make `ssh main.silo` work (`main.silo-dev` from a development build), ([recipes](SiloUI-RECIPES.md)); it saves no checkpoint, a fork starts from `main` as it is |
| `silo recipe add FILE [--name N]` / `list` / `show NAME` / `remove NAME` / `build NAME [--force]` | store a recipe file in the engine, list the stored recipes and which are built, print one, forget one, or build one now ([recipes](SiloUI-RECIPES.md)). Adding, removing and building are the owner's |
| `silo ports <computer>` | list the published ports with their host port and URL |
| `silo publish <computer> <port> [--http]` / `silo unpublish <computer> <port>` | publish or stop publishing a guest port; `--http` serves it as a website with a URL per computer ([URL proxy](SiloUI-URL-PROXY.md)) |
| `silo ssh-setup <computer>` | make `ssh <computer>.silo` reach a computer. A Linux computer gets SSH access turned on and an entry in `~/.silo/ssh-hosts.conf` (`~/.silo-dev/` for a development build), which `~/.ssh/config` includes; a macOS computer's saved entry also answers to the name. After `silo replace-main` or its undo the name is written again for the computer that now has it (a macOS entry follows on its own) |
| `silo fork <source> [--name N] [--checkpoint ID] [--secret NAME]... [--no-start]` | copy a computer and start the copy; the name defaults to `<source>-<6 hex>`. `--no-start` leaves it stopped |
| `silo fork <name> --recipe R` | make a computer named `<name>` from the cached build of stored recipe R (built first if missing) |
| `silo fork <source> -n N [--name PREFIX]` | N forks of the same checkpoint, made at once and started; each fork's name and URLs are printed ([details](#several-forks-and-commands)) |
| `silo fork <source> [-n N] -- <cmd...>` | fork, start, run the command in the fork (output streams, the exit code is passed through) and leave the fork for review |
| `silo fork <source> --gui [--computer-use ask\|auto] [--gui-timeout SECS]` | fork, start the fork and wait until its desktop and computer-use driver are ready; prints the name and computer-use status ([details](SiloUI-COMPUTER-USE.md)). `--computer-use auto` sets the approval of agents running inside the fork to no prompts; it applies to forks only (also without `--gui`), `main` keeps its mode, and only a person at the device may choose it |
| `silo fork ./dir` | copy a host folder's git repository into a fresh Linux fork (device only, [details](#folder-forks)) |
| `silo diff <computer> [--json] [--max-lines N] [--no-baseline] [--baseline]` | what changed since the fork (captures the computer now, compares with the record `silo fork` took); `--baseline` measures a missing baseline first, on a throwaway idle fork of the same checkpoint (also for a fork that has been used), and a baseline measured on one fork of a computer applies to its other forks; `silo merge --dry-run` ignores review digests |
| `silo merge <fork> [--into C] [--choose ITEM=ANSWER]... [--skip ITEM]... [--dry-run] [--data-ok]` | bring a fork's changes into the computer it came from; see [Merge](#merge-and-replace-main) |
| `silo merge <fork> --request` | record a review request for the fork and print its id; nothing is merged ([Review requests](#review-requests)) |
| `silo replace-main <fork> [--into C] [--dry-run]` | make the fork main: it takes main's name, ports, secrets and GitHub access, and the old main is archived as `main-before-<time>`; this restarts main; `silo undo main` swaps back ([details](SiloUI-CHANGES.md#replacing-main-with-a-fork-silo-replace-main-fork)) |
| `silo undo <computer\|fork\|./dir>` | put a computer back to before its latest merge or replace, or a folder back to before its latest merge |
| `silo drop <computer> [--force]` | throw a computer away (a hidden throwaway too, by name or id); `main` needs `--force`. A stopped or failed `silo-baseline-*` copy more than an hour old is dropped by the next fork (alongside the fork, for at most 20 s; one the engine cannot drop is tried again next time) or `silo diff --baseline` |
| `silo exec <computer> [--cwd DIR] [--timeout SECS] -- <cmd...>` | run a command; the exit code is passed through |
| `silo exec <computer> --stream [--stdin] [--cwd DIR] -- <cmd...>` | run a command with no time limit and print its output as it arrives; `--stdin` reads this process's standard input to its end (at most 1 MiB) and gives it to the command as its own, so a value travels in the request to the engine and appears in no command line or environment. The command is not recorded as a step. `silo-runner` starts GitHub's runner this way ([CI runner](SiloUI-CI-RUNNER.md)) |
| `silo checkpoint <computer> [--name N] [--kind manual\|automatic\|before-keep\|step]` | save a checkpoint |
| `silo checkpoints <computer>` | list checkpoints |
| `silo restore <computer> <checkpoint>` | put a computer back to one of its checkpoints |
| `silo steps <computer>` | the commands run in the computer through Silo, numbered, with exit code and the checkpoint each left ([steps](SiloUI-CHANGES.md#steps-and-the-fork-tree)) |
| `silo fork <computer> --from-step N` | fork from the checkpoint taken after step N |
| `silo tree` | the computers as a tree: what each was forked from, its state and pending review |
| `silo requests [--status open\|kept\|thrown-away\|all]` / `requests show\|merge\|decline\|open <id>` | review requests ([details](#review-requests)) |
| `silo approvals` / `approvals show\|allow\|deny <id>` / `approvals status\|wait\|note <id>` | what computers' agents asked you to allow, and the caller's side of a request ([details](#approvals)) |
| `silo agents list <computer>` / `silo agents install <computer> <agent>` | the coding agents installed in a computer, or install one |

A fork publishes the ports its source publishes (a recipe fork, the recipe's `ports`), each on a host port of its own. The engine does not copy published ports when it forks a computer, so `silo fork` does it with the same `publish` calls. The MCP server has `ports`, `publish`, `unpublish` and `ssh_name` tools.

| Command | What it does |
| --- | --- |
| `silo tools enable <computer> [--binary PATH]` / `disable` / `status` | turn Silo's tools in a computer on or off, or say whether it has them and whether it can reach them now, at its next start or after a restart. New computers have them already; `--binary` first copies a `silo` built for Linux into a running Linux computer ([Silo's tools in a computer](SiloUI-IN-GUEST-CLIENT.md)) |
| `silo host init [--authorize KEY\|FILE]...` / `silo host status` / `silo host revoke FINGERPRINT\|KEY\|FILE` | prepare this machine to host computers for other devices, show how it is set up, or stop accepting a controller's key ([details](#hosting-computers-for-other-devices)) |
| `silo devices add <address> [--host-key FINGERPRINT\|KEY\|FILE] [--replace]` / `list` / `remove <device>` / `key` | connect, list or remove the devices this one manages, or print this device's Silo key ([details](#hosting-computers-for-other-devices)) |
| `silo export <computer> [-o FILE] [--checkpoint ID\|NAME] [--include-logins] [--yes]` | save a whole computer as a `.silo-backup` file to share; a Linux computer's known login files are removed from it unless `--include-logins` (traces may remain) ([details](#sharing-a-computer-as-a-file)) |
| `silo import FILE [--name NAME]` | add the computer in an export file as a new, stopped computer |
| `silo job <id>` | wait for a job that is already running |
| `silo mcp` | serve the MCP tools on stdio ([MCP setup](#mcp-setup)) |

**Coming:** `silo rebuild <computer>`, which makes a Linux computer again on the newest image and
keeps its workspace and home. It is not in this version.

`silo fold` is a hidden alias of `silo merge` that prints one line saying so (and `silo fold
--promote` of `silo replace-main`); it is only there so an old script says what to change. The
hosted reviewer's `silo review` is hidden from help ([Set aside](#set-aside)).

**Inside a computer.** When `/etc/silo/endpoint.json` exists (or `SILO_ENDPOINT` names another
file), `silo` reaches Silo over that computer's own channel with the computer's token instead of
the engine socket; `--socket` always means the socket. Every command is the same, and the engine
decides what the computer may do ([the policy](SiloUI-POLICY.md)): a refused call prints the
engine's reason, and a call that needs your approval prints what was asked and the request id,
then waits for your answer ([Approvals](#approvals)). `silo list` shows the computer and the
computers it made. `silo mcp` works there too, without the tools that only make sense on the
device (`up`, SSH names, reaching a review request's fork); a `device` argument or a folder as a
fork's source is refused. [The table in Silo's tools in a
computer](SiloUI-IN-GUEST-CLIENT.md#what-silo-does-inside-a-computer) says what each command does
there.

## Sharing a computer as a file

```sh
silo export main -o ~/Desktop/main.silo-backup       # a Linux, container or macOS computer, running or stopped
silo import ~/Desktop/main.silo-backup --name friend-main
silo start friend-main
silo merge friend-main --into main                    # bring its work into your own main
```

`silo export` writes the whole computer: its disks, settings and (for a fork) the fork-time
record. A Linux computer's known login files are removed from it unless you pass
`--include-logins`, with a caveat ([Logins](#sharing-a-computer-as-a-file) below). Without `-o` the file is `<computer>-<date>.silo-backup` in the current folder, and an
existing file is never replaced. `--checkpoint` exports a saved point of the computer (its id or
name) instead of its current state. A running computer is captured in place; it is not stopped.

The file never holds the computer's **memory**, whatever kind of computer it is: an export
carries disks only. A running computer has its cached file data written out to its disk, then
its disks are copied (a macOS computer is paused for the copy). A macOS checkpoint that saved
memory is exported from its disks alone. A Linux VM checkpoint that was saved while the computer
ran holds its memory and the runtime cannot write it without, so `--checkpoint` refuses it: fork
that checkpoint (`silo fork <computer> --checkpoint <id>`) and export the fork, or export the
computer itself. A file written before this rule can hold memory; importing never uses it (an
imported computer starts from its disks).

The receiver gets a **new computer** with its own id, stopped, on a deny-all network policy, with
no secrets, ports or published URLs. `silo import` names it after the sender's computer, or with
`-imported` added when that name is taken; `--name` chooses. The guest image is not in the file
when the computer uses the image this Silo pins: the receiving Silo prepares the same image the
way it does for a new computer (a download when it is not on the device), and refuses a file that
names another image than its own. Files made before this change carry their image and import as
before.

**A container computer** exports and imports the same way, on a Linux device that can run
containers, and over Connections with `--device`. Its file holds its disks, not the guest image:
the importing device needs the image the computer was made on (the one its Silo pins, or one it
kept), and a device without it says the computer was made on an older guest image that it does not
have. A device that cannot run containers refuses the file with the reason `silo host init`
gives, before anything is copied. The file is written in format 6, which a Silo that predates
container exports refuses as newer. No memory is in any file. An imported container computer
starts on a deny-all network policy and stays on it, as an imported virtual machine does, and its
forks keep it; assigning a secret does not change it. Socket files are not in the file.

**Bringing the work into your main.** If the exported computer was a fork, the file carries its
fork-time record, so the imported computer keeps the way any fork does: `silo diff` shows what
the sender changed since forking, and `silo merge <imported> --into main` is an ordinary
three-way merge against your main (a file changed on both sides is a conflict and nothing is
merged). The record does not say which computer it was forked from on your side, so `silo merge`
without `--into` answers that Silo does not know where it was forked from. A computer that was
not a fork (a `main`) imports as its own computer and has nothing to merge.

**Logins.** An export of a Linux virtual machine's current state removes the computer's known login
files from the computer the file holds, **but traces may remain in the file**: read the caveat below
before you rely on it. The known logins are the agent and tool sign-ins (a Claude Code login,
`~/.claude/.credentials.json`, and `~/.claude.json`, which holds the account Claude Code is signed in
as and the headers of the MCP servers added for the user; a Codex login, `~/.codex/auth.json`; a Gemini
CLI login, `~/.gemini/gemini-credentials.json` or the older `~/.gemini/oauth_creds.json`; a GitHub CLI
login, `gh/hosts.yml`), cloud and registry credentials, SSH private keys and other key files, found in
`/etc`, `/root`, `/home`, `/usr/local`, `/opt` and `/srv` (the folders `silo diff` walks). What every
computer has is not touched: the system password files, the machine's own SSH host keys, the desktop's
connection password and Silo's own certificate authority. The same list names the logins here, in
the dialog, and under "Not kept" in `silo diff` and `silo merge`.

Silo does this from a short-lived hidden copy of the computer (`silo-export-<8 hex>`, left out of
`silo list`): the copy is forked from the computer's current state with no secrets, no GitHub access and a
closed network, started, stripped of the files the scan names, stopped and exported under the
computer's own name, then dropped, whether the export worked or not. Progress says each step in plain
words ("Making a temporary copy of the computer", "Removing the logins from the copy", ...). The fork
takes one disk-only checkpoint of the computer, which stays as any fork's does. A copy that cannot be
dropped (the engine stopped mid-export) stays hidden and is dropped by the next export. A computer that
is stopped is exported the same way; the copy is what boots. The result and `--json` name what was removed:

```text
Exported main to /home/me/main-2026-11-10.silo-backup (3.1 GiB)
Removed from the exported computer: a Claude Code login (/root/.claude/.credentials.json), Claude Code sign-in data (/root/.claude.json). Traces of them may remain in the file's disk layers, so keep the file private.
```

`--json` prints `{computer, archive, size, checkpoint, logins, unchecked, leftOut}`: `leftOut` lists the
files removed; `logins` is only what the file still holds on purpose.

**What this does not do.** It keeps the files out of the computer someone imports; it does not keep their
bytes out of the file. The copy is a fork, and a fork's disks are layers over the computer's own: the
layers it shares with the computer (everything up to the fork's checkpoint) are read-only and are written
into the file whole, with the deleted files' blocks in them. Discarding the freed blocks in the copy (`fstrim`
works on both of its disks once the root disk is mounted a second time, because `/` is an overlay) changes
only its own top layer. The canary check ([`t14_export_scrub_check.sh`](direction/checks/t14_export_scrub_check.sh),
[`T14.txt`](direction/evidence/T14.txt)) writes a long random marker to every known login path and finds it in
the layers of the default export. Treat the file as private whether or not the logins were removed.
A file in a place Silo does not recognise (an `.env`, a token in a shell history, a key under `/workspace`) is not
removed either.

`--include-logins` keeps the files in the computer the file holds. Then, as before, `silo export` lists the logins
on the computer before it writes anything and asks:

```text
This computer contains a Claude Code login (/root/.claude/.credentials.json). Share anyway? [y/N]
```

Three kinds of export cannot remove the logins and always ask this way (and the app asks in a dialog): an export
of a **checkpoint** (its disk is not the live one, and there is no copy to strip), of a **macOS computer** and of
a **container computer**. The question also covers the case where Silo **could not check**: the computer is stopped
(a check never starts it, so its disk is not listed; only its fork-time record is), the export is of a checkpoint, or
the check failed. Then the question names the reason:

```text
Silo could not check main for logins (the computer is stopped, so its files were not listed; start it to check them). Share anyway? [y/N]
```

If logins were found and part of the computer could not be checked, both are in one question
("... Silo could not check main for other logins (...)"). Without a terminal such an export refuses unless
`--yes` is passed; a yes puts the files in the export, and anyone with the file can use them. The disk is listed
with one change capture (the same walk `silo diff` does, over `/etc`, `/root`, `/home`, `/usr/local`, `/opt` and
`/srv`; a macOS computer's home folder). `unchecked` is the reason when Silo could not check and `null` when it did;
it is `null` when the export removes the logins.

**Free space.** Before it captures anything, `silo export` checks that the volume of Silo's working
copy and the volume of the destination can hold what this export writes: the computer's own
checkpoints and disk once (plus the guest image cache when the file carries the image), a small
margin and the 1 GiB Silo always keeps free. Other computers' checkpoints and caches are not counted.
Ancestors that live in another computer's group are not counted either; the check that follows the
write uses the file's real size.

**A computer on another device.** `silo --device mini export main -o ~/Desktop/main.silo-backup`
exports a computer of a connected device into a file on this one, and `silo --device mini import
~/Desktop/main.silo-backup --name friend-main` creates a computer there from a file here. The
other device does the export or import itself (the logins are removed or asked about on it, as above, and
`-o` is a path on this device). Because the managed Connections key can only run Silo's bridge, the
file travels through the other device's transfer area: it writes the export there under a name it
picks, this device copies it over the Connections link (a `Copying ...` step with progress, size
and checksum checked), and the other device removes its copy. An import goes the other way and the
other device removes the received file once the import has finished, worked or not (if this
device loses track of an import that may still be running, the file stays for that import and is
swept later). A leftover (the
connection dropped, the engine stopped) is swept when that engine starts and after a day. Ctrl-C
cancels the copy. Both devices need this Silo version; an older one answers "The other machine's
Silo is too old to share computers over Connections. Update Silo there." The file is limited to
16 GiB and the receiving device needs that much free space plus 64 MiB, after what its other
copies in progress have already claimed. The limit is 16 GiB because computer exports carry disk
images, which Files' 4 GiB is too small for, and because 16 GiB at the copy's 64 KiB/s time budget
is about three days, so the budget stays finite. Silo does not guess the size up front: an export
for another device stops as soon as the computers written so far are over the limit, before the
file is assembled, and the message names the computer and the limit; a finished file that is still
larger is removed with a message naming the file and its size. The copy lands in a `.part` file
next to the path you chose and takes its name without replacing anything (on a drive without
that, such as exFAT, it falls back to a link or to a check and a rename); a `.part` left by an
earlier crashed copy is removed and the copy starts again. See [Connections](SiloUI-CONNECTIONS.md#sharing-a-computer-between-devices).

The MCP server has no export or import tool: both read and write files at paths the caller
chooses, and the file can hold the computer's logins, so they stay a decision of the person at the
terminal.

## Several forks and commands

`-n N` (1 to 32) makes N forks of one checkpoint concurrently: one new fork point of the
source as it is now (`silo-fork-<id>`, disks only and kept, because `silo diff --baseline` forks
from it; see [recipes](SiloUI-RECIPES.md#silo-up)) that all N share. Names are `<source>-<6 hex>`,
or `<prefix>-1` to `<prefix>-N` with `--name`. Each fork is started and its URLs
(`network.urls`) are printed. Without `-n` or a command, `silo fork` makes one fork and starts
it (`--no-start` leaves it stopped).

Limits are checked before anything is made:

- macOS: Apple allows two macOS VMs at once, so N may not exceed the free slots (2 minus the
  macOS computers running, the source included). Fork a Linux computer for more.
- There is no fixed cap on how many computers are made. The engine fails a call that would
  freeze the device or fill its disk, and `silo fork`, `start` and `create` print a warning
  sentence when the device is getting short (`silo resources` shows the numbers; thresholds are
  in [the policy](SiloUI-POLICY.md#limits)).

`-- <cmd...>` runs the command in each fork once it is started, with no time limit (it uses the
engine's streaming exec). With one fork the output goes to stdout and stderr as it arrives and the
exit code is the command's own (128 plus the signal when it was killed). With `-n N` each line is
prefixed with `[fork]`, a per-fork summary follows, and the exit code is the first non-zero one.
The forks stay for review: `silo diff <fork>`, then `silo merge <fork>` or `silo drop <fork>`.

## Folder forks

Folder forks work on the device's own files and are not offered inside a computer or with
`--device`. `silo fork ./repo` needs the top folder of a git repository with a commit. It makes
a fork of the `linux-dev` recipe (built if needed; `--name` and `--secret` apply), starts it, sends `git bundle --all` through `files.upload` (into a `/workspace/silo-in-<id>` folder,
a root the transfer API allows, removed after the clone) and clones it to `~/<folder>` in the
fork. Only commits are copied; uncommitted changes stay on the host (the command says so). The
CLI remembers the host path, the commit the fork started from and the branch, beside the engine
socket (`cli-folders.json`). Words after `--` run in the repository inside the fork.

`silo merge <fork>` on such a fork merges into the folder instead of a computer:

1. The host folder must be on a branch with no uncommitted changes to tracked files; otherwise
   the merge is refused and changes nothing. Uncommitted work in the fork is committed there
   first (`silo: uncommitted changes in <fork>`).
2. The fork's commits since the starting commit come back as a bundle (`files.download` from a
   `/workspace/silo-out-<id>` folder in the fork, removed after) and are fetched into
   `refs/silo/<fork>/head` in the folder. The merge is refused, before anything is changed,
   when the commits add or change a `.gitattributes` file (it could select filter or merge
   programs the folder's git configuration defines), or add files where the folder has files git
   does not track (untracked or ignored; they are listed).
3. A backup ref `refs/silo/backup/<UTC time>` marks the folder's HEAD, then the branch is
   fast-forwarded, or merged when the folder moved on. A merge that would conflict is refused
   (the files are listed) before the tree is touched; the backup ref is made only for a merge that
   goes ahead.

`--dry-run` stops after step 2 and adds only the `refs/silo/<fork>/head` ref. `silo undo <fork>`
or `silo undo ./repo` resets the branch to the backup ref; it refuses when the folder has
uncommitted changes to tracked files, has new commits since the merge, or has untracked or ignored
files where the reset would write files. Every host git command of a merge or undo runs with
`core.hooksPath=/dev/null`, `core.fsmonitor=false` and `protocol.ext.allow=never`, and merges with
`--no-verify`, so no hook or program named by the folder's configuration runs because of a fork's
commits (the folder's own commit hooks do not run for the merge commit either); its identity and
other settings still apply. Everything done on the
host stays inside the repository (its `.git/silo` folder holds the bundles in transit, removed
afterwards). A folder fork cannot replace main or be merged `--into` a computer.

## Review requests

`silo merge <fork> --request` takes a fresh `silo diff` of the fork (for a folder fork, its commits
and uncommitted files) and calls `changes.request`, which stores a review request in the engine
(`fold-requests/<id>.json` under the app data folder): `id` (`fr-` and 8 hex), `forkId`, `target`
(the computer id; `folder:<path>` for a folder fork), `createdAt`, `reviewDigest`, `summary` and
`status: open`. Asking again for the same fork and target updates that open request. The id is
printed. CI or cron call `silo fork main -- <cmd>` then `silo merge --request`. A computer's agent
can ask for a review of a fork it made the same way.

- `silo requests [--status open|kept|thrown-away|all]` lists requests (default open) with the
  fork, its state, the computer it would be merged into and the count of changes per bucket.
  (`kept` and `thrown-away` are the stored names of merged and declined requests.)
- `silo requests show <id>` prints the same buckets as `silo diff` for a fresh look at the fork,
  then `Credentials used: ...` (`none` when the fork used no secret; a `Blocked:` line follows
  when the proxy refused something). It says when the fork changed since the request, which makes
  a merge refuse.
- `silo requests merge <id> [--data-ok] [--choose ITEM=ANSWER]... [--skip ITEM]...` merges the
  fork pinned to the request's digest. It ends as `silo merge` does: exit 2 when refused (the
  request stays open), 3 when it needs answers. A folder fork is merged with `silo merge`.
- `silo requests decline <id>` drops the fork and closes the request; `main` and the
  request's target are refused.
- `silo requests open <id>` starts the fork if it is stopped and prints its name, SSH name and
  URLs. Device only.

Each takes `--json`. The MCP tools are `requests_list`, `request_show`, `request_merge`,
`request_decline` and `request_open`, on the same engine methods.

## Merge and replace main

`silo merge <fork>` brings what a fork changed into the computer it was forked from (`--into
<computer>` names another). It is the same engine call as the app's Merge ([the
rules](SiloUI-CHANGES.md#merging-a-fork-silo-merge-and-silo-undo)). Nothing is written until the
engine has checked every item, and a refusal changes nothing.

- `--dry-run` reports what would be merged and what would stop it, and changes nothing.
- A fork and its target can both have changed the same file, package or repository. The merge
  does not guess: it stops and lists each such **conflict** with an id and the answers allowed
  (`file:/etc/app.conf  fork | main`, `repo:/work/app  fork | main | branch`), and exits with
  code 3. Run it again with an answer for each: `--choose file:/etc/app.conf=fork`. `fork` takes
  the fork's version, `main` keeps the target's, and for a repository `branch` leaves the fork's
  commits in the target as a branch and merges nothing, so Git settles it. `--skip ITEM`
  leaves an item as the target has it, conflict or not. `--json` prints `status: "refused"` with
  `refusal.reason: "choices-needed"` and `refusal.conflicts`, each with its `id` and `allowed`.
- Credentials are never brought over; a database is never combined. A database change is carried
  only when the target's recipe has a `migrate` step and the merge brings migration files, or
  with `--data-ok`; otherwise use `silo replace-main`.
- `--reviewed <digest>` (the digest from `silo diff --json`) makes the merge refuse if the fork has
  changed since the review.
- When the merge brings packages or settings files, the engine appends them to the target's
  recipe ([details](SiloUI-RECIPES.md#what-a-computer-remembers)); the output lists what was
  added.
- Undo with `silo undo <target>`.

`silo replace-main <fork>` is for what a merge cannot carry: the fork takes main's place and
the old main is kept aside as `main-before-<time>` (`silo undo main` swaps back). It restarts
main, so from a computer it always asks the owner, and says so in the request. `--dry-run`
reports without changing.

## Pull requests

`silo merge <fork> --pr [--repo OWNER/NAME] [--base BRANCH]` merges nothing. It is for the
device only (not inside a computer, not with `--device`), because it uses this device's git
credentials. It puts the
fork's code on a new branch `silo/<fork>-<id>` in the host repository (a folder fork: the folder,
which keeps its branch and tree; a computer fork: the repository the diff's Code bucket names,
bundled in the fork and fetched into a temporary repository on this device, with uncommitted
work committed in the fork first; the untracked files that were committed are listed in the
result and the PR body, and a repository holding credential paths is refused), pushes it from
this device, and opens a pull
request through the GitHub REST API whose body is the review summary (the diff's buckets, then
a `Credentials not kept:` line when the diff found credential paths; the review request uses the
same renderer). The token is `GITHUB_TOKEN`, else the one `gh auth token` prints, and goes in an
HTTP header for `https` remotes, never in an argument; without one, nothing is pushed. The push
destination never comes from a fork: for a computer fork it is `https://<git host>/<repo>.git`,
built here from `--repo`, else `GITHUB_REPOSITORY` (read only when `GITHUB_ACTIONS` is `true`,
as on a CI runner), else the `origin` of the git repository in the folder the command runs in;
without one of them `--repo` is required. The fork's own `origin`, when it names a different
OWNER/NAME, is refused, and so is an untracked file in the fork that looks like a credential (the
patterns of the diff's Not kept credentials). The git host follows `SILO_GITHUB_API_URL`:
`github.com` for the default, the host of a `/api/v3` base otherwise), and anything but `https`
is refused. For a folder fork it is the host folder's own `origin`, and the repository is the one
that origin names (a different `--repo` or `GITHUB_REPOSITORY` is refused): the commits up to the
fork's starting commit are compared with `origin/<base>`, host commits `origin` does not have are
refused and listed, and the local `silo/<fork>-<id>` branch is deleted after the push.
`--pr` cannot be combined with `--request`, `--into` or `--dry-run`, and is not an MCP option.
The result prints the pull request URL (`prUrl` with `--json`). A fork's own credentials are never
used and its GitHub access stays read-only. `SILO_GITHUB_API_URL` changes the REST base (default
`https://api.github.com`; tests point it at a local server). The token header goes only to a push
whose `https` host is the git host of that base. `silo merge <fork> --pr --json` prints `prUrl`
(also in `pullRequest.url`), `branch`, `base` and `repository`; `silo merge <fork> --request
--json` prints the request as `request.id`.

## Approvals

**Your side.** What a computer's agent asked you to allow (a merge into its own computer, a port,
a secret, GitHub access and the rest, see [SiloUI-POLICY.md](SiloUI-POLICY.md#asking-you)):

- `silo approvals` lists the requests waiting for your answer.
- `silo approvals show <id>` prints what is asked, in Silo's words, the agent's note apart, the
  call that would run, and once answered what became of it.
- `silo approvals allow <id>` checks the request again and runs its call; the output says what
  it returned, the job it started, or why it did not run.
- `silo approvals deny <id> [--note TEXT]` denies it; the agent reads the note.

Each takes `--json`. A merge's diff is its review request: `silo requests show <id>` with the id
`show` names.

**The agent's side.** Inside a computer, a command that needs the owner's answer does not fail.
It prints what was asked and the request id, then waits by checking `approvals.get`:

- The wait is 2 minutes unless `--wait <duration>` (`90s`, `10m`, `1h`) says otherwise;
  `--no-wait` prints the request and exits at once.
- `--reason <text>` adds the agent's own note to the request after it is asked; the owner reads
  it beside the request, marked as the agent's.
- When the owner allows it, the command runs and ends as usual (a job is followed). When they
  deny it, it prints their note and exits 4. When the wait runs out or with `--no-wait`, it
  exits 5 and nothing ran: check later. Exit 6 means the request ended without an answer
  (withdrawn or expired).
- `silo approvals status <id>` prints where a request stands and exits 0, 4, 5 or 6 the same
  way; `silo approvals wait <id>` waits like a command does; `silo approvals note <id> <text>`
  adds a note.

The MCP tools that can ask take `wait_secs` (default 30, at most 100, shorter than a client's
tool time-out) and `reason`. A wait that runs out returns `{"status":"waiting-for-owner",
"approvalId":...}` with a sentence saying so; the `approvals` tool (`action`: `list`, `status`,
`wait`, `note`) follows the request.

`silo status` prints, for a fork, a line such as
`quiet: applied; stopped: poller; scrubbed: /root/.env; secrets: none` (on macOS it adds the
count of parked jobs and the jobs left scheduled), then `secrets assigned:` and a `github:`
line (`read-only` or `can push`, with the repositories). `--json` carries the same data as
`quiet` (the fork's record, `null` for other computers), `secrets` (names) and `github`
(`null` when the computer has no assignment, or when the engine could not be read). The quiet
record comes from the engine's per-computer state (`quiet` on a Linux or macOS computer in
`computers.list` and the application state); it is absent on computers that are not forks.

A new secret needs at least one `--computer` and one `--domain`; for an existing secret both
default to what it has. A secret cannot be left with no computer, and a secret for any
destination (`*`) is created in the app. `set` reads all of stdin and drops one final newline:
`printf %s "$VALUE" | silo secrets set NAME --computer main --domain api.example.com`.

## Network policy

`silo network policy <computer>` prints a computer's network policy: its mode, the hosts you
allowed, its presets, and every host an allowlist lets through with where it comes from (your list,
a preset, GitHub access or an assigned secret). `--json` prints the same as data.

- `--mode open` is the default: any public address. `--mode allowlist` reaches only the listed
  hosts, on port 443. `--mode closed` reaches nothing, not even name lookups.
- `--allow example.com` or `--allow '*.example.com'` adds a host (repeat for several);
  `--remove HOST` takes a host or a preset off; `--preset npm` adds a named list kept by Silo
  (npm, pypi, crates, ubuntu, homebrew, github, anthropic, openai).
- The change is saved at once and applied the next time the computer starts; the output says when.
  A running computer keeps its rules until it restarts. A Linux virtual machine is restored in
  place from a checkpoint taken just before that start, named "Before network policy change".

An allowlist does not stop a computer from sending data to a host on its list. See the
[threat model](THREAT-MODEL.md#network-policy-per-computer).

## Set aside

The hosted reviewer (`silo review`, the MCP `review` tool, `requests show --review`) is set aside
for now: its commands are hidden from help, the MCP server does not list its tool, and nothing
configures a service. The code and [`reviewer-service/`](../reviewer-service/README.md) stay in
the repository. The OpenClaw and Hermes recipes are gone.

## Another device

`silo --device <name>` runs a command on a connected device instead of this one, over
Connections (the device's name, its id or an id prefix; `local` is this device). Every request
that device's engine can answer goes to it, so `silo --device mini fork main`, `exec`,
`fork main -- <command>` (and `-n`), `fork --gui`, `diff`, `merge`, `replace-main`, `requests`,
`approvals`, `recipe`, `steps`, `tree`, `checkpoints`, `resources`, `access` and `list` work on
the Mac mini's computers. The first form is used rather than an
`@device` prefix on computer names, so names stay plain. `export` and `import` copy a file between the two devices
([sharing](#sharing-a-computer-as-a-file)). `up`, `ports`, `publish`, `unpublish`,
`ssh-setup`, `secrets`, folder forks and `merge --pr` use this device's files, SSH configuration
or secrets and refuse `--device`.
The routing is described in [Connections](SiloUI-CONNECTIONS.md#driving-another-devices-main-v1-methods).

Recipes are stored by the engine that builds them, so `silo --device mini recipe add` stores a
recipe on the Mac mini, and its `keep:` hooks run there when a merge happens there. `exec` and
`fork -- <command>` run the command in the computer on that device: its engine runs
it as one of its own jobs, and every read of the output is one exchange over Connections. Its
step is recorded there. A fork made there publishes no ports and shows no URLs here: ports are
published by the device a computer runs on. `requests open` on another device prints that
device's URLs as `(as the other device sees it)` and no SSH name, since both belong to its own
network. An id found on this device and another device (or two devices) is refused until
`--device` says which. `list --all-devices` asks each device for at most 15 seconds, at once,
and skips one whose last health check failed.

- `silo list --all-devices` lists this device's computers and every connected device's, with a
  `DEVICE` column, and a line for each device it could not reach.
- `silo requests --all-devices` is one list of pending changes across devices (`[Mac mini] fr-...`).
  `requests show`, `merge`, `decline` and `open` find the device a request is on by its id
  (this device first, then the merged list) and act there; `--device` skips the search.
- MCP: only reads take a device. `device` is accepted on `list`, `status`, `diff`, `steps_list`,
  `tree`, `checkpoints`, `requests_list` and `request_show`, and `allDevices` on `list` and
  `requests_list`. The tools that change a computer have no `device`: the person at the terminal
  uses `silo --device`.

## Hosting computers for other devices

Any machine with a Silo engine hosts computers of the kinds it can run: a Mac runs Linux and
macOS computers (macOS allows two macOS computers running at once on one Mac), a Linux server
runs Linux computers over KVM. Another device manages them through
[Connections](SiloUI-CONNECTIONS.md); each host keeps its own secrets and GitHub sign-in.

First, on the controller (its engine running), print its Silo key and copy it to the host as
`controller.pub`. The output also shows the `silo host init` line to run there:

```sh
silo devices key
```

Then on the host, as the account that will own the computers (not root):

```sh
# Linux: check and unpack the engine archive (see the engine guide), then init
sha256sum -c silo-engine-<version>-linux-x64.tar.gz.sha256
mkdir -p ~/.local/opt && tar -xzf silo-engine-<version>-linux-x64.tar.gz -C ~/.local/opt
chmod -R go-w ~/.local/opt/silo-engine
~/.local/opt/silo-engine/silo host init --authorize controller.pub
# Mac: the app's silo (linked as ~/.local/bin/silo once Silo has run)
/Applications/Silo.app/Contents/MacOS/silo host init
```

The Linux order end to end: `silo devices key` on the controller, unpack the archive on the host
([engine guide](SiloUI-ENGINE.md#silod-on-a-linux-host)), `silo host init --authorize` with that
key on the host (it runs `silod --service register` for you; `silod --service register` alone
installs just the service), then `silo devices add <address>` on the controller. Without `--authorize`, `silo devices add` installs the controller's
key over SSH instead, when SSH lets it in.

`silo host init` changes nothing until its checks pass, and running it again is safe:

1. **The account.** Root and `nobody` are refused, as the engine refuses them.
2. **Computers can run here.** Linux: `/dev/kvm` must exist and be readable and writable by this
   account. When it is not, the message says what to do: an administrator adds the account to
   the device's group (`sudo usermod -aG kvm "$(id -un)"`), or, when the user database already
   lists it there, log out and back in. Mac: Apple silicon.
3. **The engine as a service.** Linux: `silod --service register` of the `silod` beside this
   `silo` (a `silo` outside an engine directory is refused). It writes and starts the systemd
   user unit and checks the engine directory the way the engine does
   ([engine guide](SiloUI-ENGINE.md#silod-on-a-linux-host)); its refusals and its lingering
   advice are printed as they are. Without lingering the engine starts at the first login, not
   at boot (`sudo loginctl enable-linger "$(id -un)"`); `silo host init` runs nothing as root and
   never turns lingering on itself. Mac: an engine that answers already runs as the login item
   the app registered; `launchctl print gui/<uid>/<identifier>.engine` (read only) tells whether
   launchd has that item, and when it has none the advice is to open the app once. When no
   engine answers, the app that holds this `silo` is opened once in the background
   (`open -g -j -a <Silo.app>`), so the app registers and starts its own login item, and the
   engine's socket is awaited for a bounded time (`engine_wait`, 60 s). If it does not answer,
   the message names System Settings > General > Login Items, where the engine may be off or
   waiting for approval. The `Silo Engine.app` inside the app never registers a login item of
   its own: that would be a second registration with the same launchd label. A Mac login item
   runs in a logged-in session.
4. **`silo` on PATH.** Linux: `~/.local/bin/silo` (`silo-dev` for a Dev engine) is linked to this
   `silo` when nothing is there, or the link there points at a `silo` with a `silod` beside it,
   or at a `silo` that is gone from a directory whose name starts with `silo-engine` (an unpacked
   engine archive that was replaced). Anything else, a link to a missing file elsewhere
   included, is left alone and reported, and a `~/.local/bin` missing from `PATH` is reported. On a Mac the engine links the app's `silo` itself when it starts, and
   its error, if any, is reported.
5. **Connections on** (`connections.setEnabled`), which also links the bridge
   `~/.local/bin/silo-remote` that a controller's forced SSH command runs.
6. **Controller keys.** Each `--authorize` key (`ssh-ed25519 ...`, as `silo devices key` prints it
   on the controller, or a file of such lines) is added to `~/.ssh/authorized_keys` by the engine
   (`connections.keys.authorize`) with Silo's restricted line:
   `restrict,command="exec ~/.local/bin/silo-remote --remote-bridge --key SHA256:..."` (the key's
   own fingerprint) and this channel's comment. The key can only run the bridge. An argument is a
   key only when it is one (`ssh-ed25519` and a base64 blob that decodes to the type string
   `ssh-ed25519` and a 32-byte key); anything else is read as a file, every line of which must be
   such a key. Every key is checked before anything changes, and the engine checks it again.
   This is an out-of-band key install with the same authority as a paired device's key; keys do
   not expire, and `authorized_keys` is the only place one is revoked.
7. **SSH's view of the account.** sshd's `StrictModes` (on by default) ignores
   `~/.ssh/authorized_keys` when the home or `~/.ssh` can be changed by group or others; that is
   reported with the mode, the group and the `chmod g-w` or `chmod go-w` that fixes it. Debian's
   and Ubuntu's sshd (detected by `/etc/debian_version`) let the owner's user-private group
   write (a local, non-root account and group, as the engine checks), so a home made under
   umask 002 is not reported there.
8. **The address.** It prints the addresses Connections offers (`user@host.local`, the host
   name, Tailscale and other interface addresses) and whether anything answers on port 22.
   Controllers reach the host over SSH: turn on Remote Login on a Mac, or run an SSH server on
   Linux. Use `ssh://user@host:port` for another port.

`silo host status` reads the same things without changing any: VM support, the service
(`silod --service status` on Linux, with advice when it is not `enabled`; the launchd check
above on a Mac), whether the engine answers, the command link, Connections, the bridge link, the
controller keys and SSH. The controller keys are every line of `~/.ssh/authorized_keys` whose
forced command is this channel's bridge, whatever its other options or comment and whichever
Silo version or person wrote it, each with its `SHA256:` fingerprint (as `ssh-keygen -lf` prints
it); a line that is not exactly Silo's current one is marked, and one without `--key` is named
in the advice. Lines for the same key without the bridge's forced command (a personal login) are
not listed. `silo host revoke FINGERPRINT|KEY|FILE` removes every bridge line for the key
(`connections.keys.revoke`) and says which keys had none. `connections.keys.authorize` and
`connections.keys.list` are this device's own: the Connections bridge does not serve them, so a
connected controller cannot add a key. Through the bridge, a controller can revoke only the key
it connected with (see [Connections](SiloUI-CONNECTIONS.md#controller-keys-on-the-host)).
`silo host` and `silo devices` work on this build's own engine only: they refuse
`--allow-other-channel` and a `--socket` outside this channel's home.

On the controller:

```sh
silo devices add ada@build-box.local
silo list --all-devices
silo --device build-box create e2e --linux
```

`silo devices add` is the app's Connect device (`connections.devices.connect`). The address is
`user@host`, an SSH alias, `ssh://user@host:port`, or `user@host:port`, which is saved as
`ssh://user@host:port` (the form ssh reads a port from). The first time,
SSH may not know the host's key yet: it runs the engine's own `ssh` command in this terminal
(`connections.ssh.authorizationCommand`, `StrictHostKeyChecking=ask`) so you can compare the
fingerprint with the host's (`ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub` there), then
tries again. When no key this device offers is accepted, it runs the key setup
(`connections.ssh.keySetupCommand`), which installs this device's Silo key there over SSH (SSH may
ask for the password), then tries again. Each repair runs at most once, a changed host key is
never trusted from here, and without a terminal the command is printed instead. A device that
already accepts one of your own SSH keys connects without either step, as in the app.

`--host-key` trusts the host's SSH key without asking: it is added to `~/.ssh/known_hosts`
under the name ssh looks up (`host`, or `[host]:port` off port 22) before connecting. A
`SHA256:` fingerprint, as `ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub` prints it on the
host, is matched against the keys the host offers (`ssh-keyscan`); a key or a `.pub` file is
taken as given. A fingerprint that matches nothing is refused with the fingerprints the host
does offer, and a different key of that type already listed for that name, found with
`ssh-keygen -F` (so hashed `|1|` names count), is reported, never replaced (`ssh-keygen -R`); a
key `known_hosts` marks `@revoked` is refused. The address is checked with the engine's rules
before `ssh-keyscan` runs. The fingerprint you type is the trust root: Silo checks the host only
against it. It does not read `ssh_config`, so for an alias whose `HostName` differs, pin
the key yourself or use the host's name.

When the host accepts no password and none of your keys, authorize this device's key on the
host instead: `silo devices key` prints it (`connections.keys.own`, made on first use), then
on the host `silo host init --authorize '<key>'`, then `silo devices add` here. With both,
nothing asks a question, which is how a script adds a host:

```sh
# controller
silo --json devices key | python3 -c 'import json,sys; print(json.load(sys.stdin)["publicKey"])' > controller.pub
# host (controller.pub copied there)
silo --json host init --authorize controller.pub
# controller (the fingerprint read on the host)
silo --json devices add ada@build-box:22 --host-key SHA256:... < /dev/null
```

`app/SiloUI/scripts/hosts-e2e.sh` does this with two accounts on one Linux machine (the
`linux-host.yml` workflow runs it on its KVM runner): it prepares the host, adds it by
`127.0.0.1`, makes, starts and drops an `e2e-hosts-*` computer there with `--device`, and
removes the host again, cleaning up after a failure. With `SILO_E2E_SHARE=1` it also exports the
host's computer to the controller, imports the file on the controller, imports it onto the host
with `--device`, and checks that neither engine's transfer area keeps a file.

`silo devices list` shows the saved devices with their last health check, and
`silo devices remove <device>` removes one, as in the app: the engine forgets it only after the
host removed every Silo key this device installed there or found none left. A key this device
used before (a rotated one) is refused by a host that can tell keys apart, and the removal stops
with that message; remove such a key on the host with `silo host revoke`. `silo host` and
`silo devices` refuse `--device`.

## MCP setup

```sh
claude mcp add silo -- silo mcp
```

The server has the same operations on the device and inside a computer (there it reaches the
computer's own channel, and the engine decides each call). Its tools: `list`, `status`, `tree`,
`resources`, `access`, `recipe_list`, `create`, `fork` (with `recipe`, `count`, `command`,
`fromStep`, `no_start`, or a repository path as `source` on the device), `start`, `stop`, `exec`,
`diff`, `merge` (with `choices`, `skip`, `dry_run`, `request`), `replace_main`, `undo`, `drop`,
`checkpoint`, `checkpoints`, `restore`, `steps_list`, `ports`, `publish`, `unpublish`,
`network_policy`, `secrets_list`, `agents` (with `install` to install one), `approvals` (`action`: `list`, `status`, `wait`, `note`), `requests_list`,
`request_show`, `request_merge`, `request_decline`, `retry_setup`, and the computer-use tools (they
drive forks and refuse `main`) `fork_gui`, `computer_use_ready`, `computer_use_screenshot`,
`computer_use_click`, `computer_use_type`, `computer_use_key`, `computer_use_hotkey`,
`computer_use_scroll`, `computer_use_move`, `computer_use_drag`, `computer_use_wait`,
`computer_use_tools` and `computer_use_call` ([computer use](SiloUI-COMPUTER-USE.md)). Only on
the device: `up`, `ssh_name` and `request_open`. A fork made by the server starts, like the
command's; pass `no_start` for a stopped one. `network_policy` only reads unless it is given
`mode`, `allow`, `remove` or `presets`; a change reads the policy first and sends it as the one
the change was judged against, so a save over a policy that changed meanwhile fails and changes
nothing (the command `silo network policy` does the same).

A tool that needs the owner's answer waits `wait_secs` (default 30, at most 100) and then returns
"waiting for the person" with the request id; see [Approvals](#approvals). The tools make the same
engine calls as the commands and return the same JSON as `--json`. `drop` cannot drop `main`
through MCP. No tool sets or assigns secrets; values are never sent over MCP. The server's
`instructions` text and each tool's description are written for an agent that has not seen Silo.

## Testing against a local GitHub server

A debug build of the engine sends its `api.github.com` requests to the URL in
`SILO_GITHUB_API_BASE` when it is a loopback `http` or `https` origin (`localhost`,
`127.0.0.1` or `[::1]`, with an optional port and no path). Anything else is ignored. A
release build never reads the variable.

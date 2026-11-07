# Silo 0.13.0 release notes — DRAFT

**DRAFT for the owner. Not approved public text.** Drafted from the 34 pending changesets in `app/SiloUI/.changeset/`. The real notes are generated from those files by `npm run release:version`; edit wording there. The version (0.13.0) follows from the changesets (22 minor, 12 patch) and is fixed only when the release is prepared.

Silo now has a command line, `silo`, and an MCP server, `silo mcp`, so you and your agents can fork a computer, let an agent work in the fork, look at what changed, and keep or throw away the result.

Forks get none of the credentials Silo manages unless you name them; files on disk are copied.

## Fork, review, keep

- `silo` lists, creates, starts, stops, forks and drops computers, runs commands in them and saves checkpoints. Describe forks in `.silo/recipe.yaml` and `.silo/silo.yaml`.
- `silo fork -n N` makes several forks at once (limited by `fork_ram_budget` and by Apple's limit of two macOS VMs). `silo fork <source> -- <cmd>` runs a command in the fork. `silo fork ./repo` copies a git repository into a new Linux fork and `silo fold` brings only its commits back.
- `silo diff <computer>` shows what changed in a fork since it was forked: code, packages, settings and files, databases and the rest. The review puts launch agents, crontabs and named files first and treats macOS background noise as noise.
- `silo fold` keeps a fork's changes in the computer it came from, and `silo undo` puts them back. A keep first saves a checkpoint. It stops without changing anything if the fork and the computer changed the same thing, or if a database change has no migration. It replays packages by name and version, merges git, copies files, merges crontabs and launch agents, and runs the recipe's migrate, restart and after steps. Linux and macOS.
- `silo fold <fork> --promote` makes a fork the main computer. It takes main's name, published ports, secrets and GitHub access, and the old main stays stopped as `main-before-<time>`. `silo undo main` swaps them back. Works for Linux and macOS, and keeps SSH entries, scheduled jobs and hostname with the main role.
- `silo fold <fork> --pr` pushes a fork's code to a new branch and opens a pull request with the review summary.
- Review requests: `silo fold --request` records one; `silo requests` lists, shows, keeps, throws away and opens them. A request shows a fresh diff and a "Credentials used" line, and a keep is pinned to the changes the request was made from.
- `auto_keep:` rules in `.silo/silo.yaml` keep a fork's changes without asking when every change falls in the buckets a rule lists and, if the rule asks, the recipe's `test:` command passes in the fork. Anything else gets a review request.

## Quiet forks

- A fork starts quiet. On Linux it cold-boots the disk only, gets a new machine id and hostname, and has scheduled jobs turned off. On a Mac, scheduled jobs, crontabs and scheduled launch agents are moved aside before the fork first starts. Services that keep running stay.
- Forks get none of the credentials Silo manages unless you name them; files on disk are copied. GitHub access that is copied is read-only.
- A recipe can name programs to stop (`quiet:`, which also turns off their autostart entries and services) and files to delete (`scrub:`).
- `silo status` and `silo diff` show which Silo-managed secrets a fork used, per host and method, and how many attempts were blocked, on Linux and macOS. They show names, hosts and methods, never values.

## Recipes, URLs and speed

- Recipes describe a clean computer in `.silo/recipes/<name>.yaml` (OS, packages, setup commands, services, ports). Build one with `silo recipe build` and fork from it. `linux-dev` and `macos-xcode` come built in.
- `silo up` makes `main` from a recipe and starts it. A fork of `main` starts from `main` as it is when the fork is made, from a disk-only capture; Silo says which checkpoint it used.
- Each computer's websites open at their own local name, such as `http://3000.my-computer.silo.localhost:47383`, so several computers serving the same port can be opened side by side. Forks made with `silo fork -n` publish their source's ports and print their URLs. `silo up` makes `ssh main.silo` work.
- Forking a macOS checkpoint is faster: the fork's first start is its only boot, and it no longer reinstalls developer tools the checkpoint already had. Forking a Mac without choosing a checkpoint forks its current state from a "Fork point" checkpoint.

## Agents

- `silo steps` records the commands an agent runs as numbered steps. `step_checkpoints` saves a checkpoint after each, `silo fork --from-step N` forks from any step, and `silo tree` shows how computers descend from each other.
- `.silo/silo.yaml` takes per-agent overrides under `agents:`, chosen by `--agent`, `SILO_AGENT` or the MCP client's name. A keep that brings packages or settings files appends them to the recipe the computer came from (`--no-write-back` skips it).
- An agent living inside a Linux computer can fork it and ask for review with `silo --in-guest`. `silo guest-client install <computer>` gives the computer a token. The agent can fork itself, diff its forks and ask for review. It cannot keep, promote, undo, drop or change secrets.
- An agent can drive a fork's desktop from the host: `silo mcp` has screenshot, click, type, key, scroll, move, drag and wait tools, and `silo fork --gui` waits until the desktop is ready. These tools work on forks and refuse `main`.
- The built-in `openclaw` and `hermes` recipes install the agent in `main` and start its gateway as a service; a fork of `main` does not answer the same chat channels twice.
- Drive another Mac's computers over Connections with `silo --device <name>`, and list computers and requests across devices with `--all-devices`.
- Pro: `silo review <fork|request>` sends a fork's diff to a hosted reviewer you configure and shows general, packages and code findings. It is advisory and off until configured.

## Fixes

- Deleting old checkpoints and compacting a computer's disk now give the disk space back. Deleting a computer removes its empty checkpoint folder.
- A fork starts from the checkpoint's disks instead of resuming memory. Freeing checkpoint disk on a computer with large disks waits for the merge instead of failing.
- Computer lists and status reads no longer fail while another computer change is running; they show the last known state.
- A new macOS computer gets its own password again, and a disk that cannot be expanded leaves the computer ready with a note. `silo retry-setup` finishes setup of a computer that stopped early.
- A keep is no longer refused because of background changes it does not read or write.
- Fixes for the in-guest client and for making a fork main (ordering error, long computer names, a running computer that had called the bridge).
- `silo` can list, publish and unpublish a computer's ports.

## Notes for the owner (delete before publishing)

- Source changesets cover everything above; check wording against them.
- Before release: confirm the macOS "promote" and the 11-character name limit text still match the final behavior,.
- No timings, sizes or counts are claimed here. Add numbers only from the final run (slice 6b) with their source.

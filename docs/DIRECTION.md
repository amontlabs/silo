# Silo: product direction

Agreed with the owner on 2026-11-10, after a review of everything built so far. This page says
what Silo is meant to be. It is a direction, not a description of the code. Most of it is now
built (see [What is built](#what-is-built)); what is not is listed there and under
[Still open](#still-open). Where another document disagrees, this page decides what gets planned;
the other document describes what the code does.

## What is built

- **Who is calling, and the one rule.** Every call carries who made it: the owner, another device
  or a computer. A computer does what gives it no more than it has and asks for the rest
  ([what an agent may do](SiloUI-POLICY.md)).
- **Approvals** in the Silo app: the Pending page, the menu bar, notifications, allow or deny with
  a note. The audit log shows who did what (Activity).
- **The address inside every computer**: a loopback port per computer with a challenge-and-response
  handshake, and the `silo` program and its MCP server inside the computer
  ([Silo's tools in a computer](SiloUI-IN-GUEST-CLIENT.md)). The `/.msb` folder carries no requests.
- **Merge** with choices for conflicts, fast-forward meaning "nothing can conflict", and replace
  main as a separate action. `silo merge`, `silo replace-main`, `silo undo`, `silo drop`.
- **Recipes** stored by the engine and remembered by each computer; the `.silo` folder is gone
  ([recipes](SiloUI-RECIPES.md)).
- **Network policy** per computer: open, an allowlist with presets, or closed, with a control in the
  app ([threat model](THREAT-MODEL.md#network-policy-per-computer)).
- **macOS computers** have the same page as Linux ones, and a macOS computer an agent may have
  used never boots on Apple's network.
- **Limits** as a safety floor on memory and disk, with `device.resources`.
- **Tokens as secrets**: several personal GitHub tokens, each assigned to chosen computers.
- **Agents**: Claude Code and Codex can be installed in a Linux computer, and onboarding makes
  `main`, installs the agent you pick and opens it. Installing an agent while creating a macOS
  computer is not offered yet.
- **Export without logins**, as best effort: known login files are removed from a Linux computer's
  export, and traces may remain in the file.

## The product

Silo gives your agent its own computer, so it does not run on yours.

1. You install Silo and it sets up one **main** computer.
2. You pick the agent you use (Claude Code, Codex, T3 Code, or a terminal). Silo installs it in
   main, and one action opens it connected to that computer.
3. You work with that agent, inside main. When a task needs it, the agent forks main, or makes a
   disposable computer, works there, and brings the result back.
4. You rarely open the app to do things by hand. You ask the agent, and the app shows you what
   needs your decision.

An agent on your own machine that drives Silo from outside still works, but it is not the path the
product is built around. It contradicts the reason Silo exists.

## One rule for what an agent may do

An agent inside a computer has the same tools everywhere, and can do what you could do in the
app. The engine decides what each call is allowed, from where the call comes from:

> A computer can do anything that gives it no more than it already has. Anything that widens what
> it has asks you.

| Free, within limits | Asks you | Never |
| --- | --- | --- |
| Fork itself, make disposable computers | Merge into main, or replace main | Read a secret's value |
| Hand a fork any access it has itself | A new repository, or write access | Touch a computer outside its own tree |
| Run commands in its forks, read their diffs, logs and screens | A new secret | Change engine settings or Connections |
| Checkpoint, restore, stop and drop its forks | A wider network policy | |
| | A port published on your device, SSH from outside | |
| | Replace main | |

This replaces the in-guest client's fixed list of thirteen methods, and the full authority that
`silo mcp` has on the host today.

## Approvals belong to Silo

An approval is shown by the Silo app, not by the agent's own program. The agent has root in its
computer, so it can answer any prompt that program shows.

1. The agent calls a tool that widens access.
2. The engine holds the call and creates a request.
3. The app shows it (notification, menu bar, the Pending page) with what is asked, why, and the
   diff when there is one.
4. You allow or deny. The agent's call returns with your answer.

It goes both ways: what you do in the app reaches the agent as events, and the agent can leave
suggestions for you. Today's Pending changes and review requests are the starting point.
There is no broker agent: policy is engine code plus your answer, never a model's judgment.

## How a computer reaches Silo

Every computer has Silo's tools at a fixed address inside it, served by the host-side network
stack that all its traffic already passes through. It is the same for Linux, macOS and container
computers, with nothing to install in the computer and no token file. The computer's identity is the
channel the call arrives on. This replaces the file polling in the `/.msb` folder.

As built, it is a loopback port on the device for each computer, which the computer's network rules
allow on the gateway address, and a token proved by a challenge and response that never crosses the
connection. The `silo` program is pushed into the computer; it is not preinstalled in the image.
macOS and container computers use the same port through their gateway. No runtime patch was needed.
A patch that maps the gateway port to a Unix socket would remove the open loopback port later.

## Git for computers

The framing stays: a computer has history, branches and a diff that covers more than Git does.

| Git | Silo | Meaning |
| --- | --- | --- |
| commit | checkpoint | a saved state |
| branch | fork | a new computer made from a state |
| log | steps | what was run, in order |
| diff | diff | everything a fork changed: code, packages, settings and files, databases |
| checkout, reset | restore | go back to a checkpoint |
| merge | merge | bring a fork's changes into main |
| revert | undo | take a merge back |

**Merge** is one word for the user, and it has two cases, as in Git:

- **Main has not changed since the fork.** A fast-forward: nothing can conflict, so every change
  is applied. Main keeps running; the computers are not swapped, because your agent lives in
  main.
- **Main has changed.** Silo lists the fork's changes item by item. Items only the fork touched
  are brought in. An item both sides changed is a conflict, and you choose a side. Code merges
  through Git itself: on a conflict the fork's commits arrive as a branch. A database is never
  combined: main's stays unless the recipe has a migration step.

**Replace main** is the separate, explicit action for taking a fork wholesale and discarding what
main did since. It restarts main, so it ends whatever runs there.

What went: the words fold, keep and promote, and keeping without asking when tests pass (the
tests run inside the fork, so the fork can fake them).

## One kind of thing: a computer

The operating system is a property of a computer, not a different product. Differences are only
the ones the platform forces (Apple's limit of two running macOS virtual machines, for example).

- **Same page in the app.** macOS computers get the terminal, files, network, GitHub and secrets
  that Linux computers have. Until then they are marked as a preview.
- **Same contents.** Both start with the same base (Git, GitHub tools, desktop, computer use,
  Silo's tools) plus the agent you picked. No agent is installed on one system and not the other.
- **An upgrade path.** Silo's own components in a computer update in place at start, with one
  mechanism for both systems. The base system is upgraded by rebuilding from the recipe on the
  newest image while keeping the workspace disk, with a checkpoint first.
- **No unprotected window.** A macOS computer never runs on Apple's own network once an agent may
  have touched its disk. The aim is to boot behind Silo's gateway from the first start.
- **One process per computer.** macOS computers move out of the engine's process into a helper
  each, like Linux computers. Not urgent.

### Recipes

A recipe is the text description of a computer: system, packages, setup, services, ports. It
belongs to the computer and Silo stores it. It is what a rebuild replays. The `.silo` folder in a
repository on your own machine, and `silo up` run from it, are dropped: they assume the code and
the agent live on your machine. The rules that were in `.silo/silo.yaml` become policy set in the
app, where the agent cannot edit them.

### Container computers

A container computer shares the host's kernel. It is faster and less isolated than a virtual
machine, and it runs on Linux hosts. It stays a choice you can make, for a machine used only for
development that holds nothing sensitive. It is never the default and is labelled for what it
is. One thing to state where it is offered: the host also holds Silo's own secrets and GitHub
tokens for its computers, so an escape reaches them. An idea for later: the part that holds
secrets and swaps them into requests runs on another device, and the container host's traffic is
forced through it, so the host itself holds nothing worth taking.

## Access

- **GitHub, signed in.** Per computer, per repository, read-only unless you allow changes. Silo
  mints tokens limited to that. This is the main path.
- **Tokens you bring.** A personal token becomes an ordinary secret bound to GitHub's hosts.
  You can have several, each assigned to the computers you choose, each as wide as you made it on
  GitHub. There is no separate "token mode".
- **Secrets.** Per computer, each bound to the hosts allowed to receive it, never readable inside.
- **Forks.** A fork can be handed any access its parent has. More than that asks you.
- **Network.** You set a policy per computer, with presets. The agent can ask for an addition
  through the same approvals. No pop-up per connection.
- **Agent logins** (a Claude or Codex sign-in) are files inside the computer today and are copied
  into forks and exports. The aim is that a login does not travel by accident; the design is
  not decided yet. Until then: a fork keeps its parent's login, because the same agent already
  controls both, and an export of a Linux computer removes known login files from the exported
  computer unless you ask for them. Traces may remain in the file: a copy is a fork, and the disk
  layers it shares with the computer are written into the file whole (checked on 2026-11-10, see
  [the evidence](direction/evidence/T14.txt)). Exports of a checkpoint, of a macOS computer and of a
  container computer keep the logins in the file and warn first.

## Trust on your own device

- Every call that changes something is recorded with who made it: the app, the command line, or
  which computer.
- Programs on your device get an identity and a scope later. That needs signed releases, which
  wait (see below). Computers do not need it: their identity is their channel.
- Malware that runs as you can still read disk images and key files directly. Identity raises
  the bar and makes actions attributable; it does not make that case safe.

## Wanted from the roadmap

Every gap and every "in progress" or "next" item in the [roadmap](ROADMAP.md) is wanted: the
egress policy, reflected-secret redaction, the audit log, the review of the `/.msb` folder,
finishing the Linux host, the protocol reference, and the move to Cua Driver. Not now: project
governance, and anything tied to funding.

## Set aside

| What | Status |
| --- | --- |
| CI runner (`silo-runner`) | Out of the product for now, kept on the roadmap. Its general form comes first: something outside starts a task in a fresh fork and gets the result. A schedule, a webhook or GitHub CI is then one trigger. |
| Hosted reviewer | Set aside. |
| OpenClaw and Hermes recipes | Set aside. |
| Signed and notarized releases (Apple Developer ID) | Left out for now. |
| `.silo` project folder and `silo up` from it | Dropped; see [Recipes](#recipes). |

## Order of work

1. Scopes, approvals and the audit log: done.
2. The address inside every computer, then onboarding that installs and opens your agent: done.
3. macOS and Linux made consistent: the app page, the contents and the network window are done;
   the upgrade path (rebuild on the newest image) is not.
4. Merge as described above, and recipes as a property of a computer: done.
5. The network policy: done. Reflected-secret redaction and the `/.msb` review: next, with the
   other open items in the [roadmap](ROADMAP.md#13-after-the-direction-build).

## Still open

- How agent logins are stored, forked and shared.
- Limits. Decided: no fixed cap on how many computers an agent makes. The agent can read the
  device's memory, disk and running computers and judge for itself; a call gets a warning when
  the device is getting short, and fails when going on would freeze the device or fill the
  disk. The default thresholds are in [the policy](SiloUI-POLICY.md#limits). Open: whether the app
  should also show this.
- The first-run flow for agent programs other than Claude Code and Codex (T3 Code, a plain
  terminal).
- Whether to keep removing logins from exports by default, given that traces may remain, until the
  runtime can flatten a fork's disk layers.
- Whether a computer should be able to read every saved recipe's name, or only its own recipe.

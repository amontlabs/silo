# The first run: from install to the first agent working in main

Research of 24 November 2026, by an agent, for polarzero. It writes out the first run of the first
build as a transcript, counts its minutes and its questions, and says where the plan makes it
longer than it has to be. Nothing here is a decision, and nothing under `plan/` was edited.

Numbers come from [`macos-first-run.md`](macos-first-run.md) (sizes and checksums read on Apple's
servers today, install times from other tools' users), from the first-build decisions file 10
(Apple's guest provisioning in macOS 27), and from [`onboarding.md`](onboarding.md) (how other tools
do it). Where a number is a report by someone else and not a measure, the text says so.

## What the first run has to beat

| Tool | What you type before an agent works | Sign-ins | What it gives |
|---|---|---|---|
| Claude Code on your Mac | `curl … \| bash`, `claude`, sign in | Anthropic | the agent on your own machine; since 2.1.283 a classifier reviews its actions by default ("auto" mode) |
| Docker Sandboxes (`sbx`, which replaced `docker sandbox` in 2026) | `brew install docker/tap/sbx`, `sbx login`, `sbx run claude` in the project folder, a network policy to pick, `/login` inside | Docker account (required), then Anthropic | a Linux microVM per agent, started with `--dangerously-skip-permissions`; a blocked host is denied and an approval is queued for next time |
| OrbStack, then a Linux machine | install the app, `orb create ubuntu`, `ssh orb` | none | a Linux machine with your user name, sharing your Mac's files; the image comes in under a minute |
| Tart, macOS guest | `brew install openai/tools/tart`, `tart clone ghcr.io/cirruslabs/macos-tahoe-base:latest`, `tart run` | none | a macOS virtual machine with a user and SSH on, from a 24 to 27 GB image (30 to 32 GB for macOS 27); no checkpoints |
| Conductor | install the app | reuses the `gh`, `claude` and `codex` logins already on the Mac | agents in Git worktrees on your Mac, no virtual machine |

Docker's `sbx` is the closest in shape: an agent with every check off inside a virtual machine. It
asks for two sign-ins and copies none of your `~/.claude` setup in, and Docker's own post of August
2026 says an empty sandbox loses to running on the host. None of the tools read shows the size of the
download and the time it will take before it starts.

The full table, with sources, is in [`onboarding.md`](onboarding.md).

Silo cannot be as fast as Claude Code on the Mac or Docker's `sbx`: Apple forbids shipping macOS
installed (`computer-image`), so every Mac downloads Apple's restore image. For macOS 27.2.1 that is
25.6 GB. Silo can be as fast as Tart, the fastest way to a macOS virtual machine today, since Tart's
prebuilt images are no smaller than Apple's restore image. It can ask much less of your attention
while it waits, and leave main set up for agents where Tart leaves a bare system.

## The goal, stated as two numbers

1. **Wall time to the first agent working in main**: the download, plus about 18 minutes for the
   install, the first start and setting main up. At 300 Mbps that is about 31 minutes; at 100 Mbps
   about 56; at 1 Gbps about 22. With a restore image already on the Mac, about 18. Until it is measured
   on polarzero's Mac, Silo states no time in public (`first-run-time-macos`), and the first run
   shows its own estimate from the speed it measures.
2. **Your attention**: under 3 minutes and at most 8 answers, all asked in the first minute, so you
   can leave the Mac for the rest. This is the number to hold the design to, since the wall time is
   mostly Apple's.

`first-run-wait` (open) already says the first run asks its questions while macOS downloads. This
file adds: **all** of them, in the first minute, and then nothing until main is ready.

## The transcript

polarzero's Mac: an M3 Pro with 36 GB of memory, macOS 27.2, 412 GB free, on a 300 Mbps link.

```text
$ brew install amontlabs/tap/silo
==> Installing silo
🍺  silo 0.1.0 installed. Run `silo` to make main.

$ silo
Silo is not set up on this Mac yet. This makes main, a macOS virtual machine where your
agents work with full access. Nothing on your Mac is shared into it.

Checking this Mac
  ✓ macOS 27.2 on Apple M3 Pro
  ✓ 36 GB of memory. Main gets 16 GB and 6 cores; change it with `silo set main --memory`.
  ✓ 412 GB free. Main needs 61 GB to be made and about 35 GB once it is.
  ✓ Apple's servers answer at 288 Mbps.

Main is made from macOS 27.2.1, which Silo downloads from Apple (25.6 GB, about 13 min)
and installs (about 6 min, measured by others). Found nothing to reuse on this Mac.

Start the download now? [Y/n] y
  downloading macOS 27.2.1   ▏                     0.1 / 25.6 GB   35 MB/s   13 min left
The download goes on if you close this window, or if your Mac sleeps and wakes.
While it runs, 6 questions. Each has a default; Enter takes it.
```

The download starts before the questions, after one yes. `first-run-failure` (open) checks the
disk, memory, chip, system and network first; the transcript shows those checks, in four lines.

"Found nothing to reuse" is a proposal: Tart keeps restore images in `~/.tart/cache/IPSWs/`, named
by their SHA-256, and VirtualBuddy in `~/Library/Application Support/VirtualBuddy/_Downloads/`.
Apple's CDN gives each image's SHA-256 before the download. When an image is there and matches,
Silo copies it with an APFS clone (no space used) and skips the 25.6 GB. People who try Silo are
the people likeliest to have one of those tools.

```text
1/6  Your user in main
     Main's user is ana, with the same home folder path as on your Mac (/Users/ana), so
     paths in your settings stay right. [Enter]
2/6  Where your agents work
     Changes outside these folders are shown first each morning.  [~/work] ~/code
3/6  Copy folders from your Mac into main, once
     Found 9 Git repositories under ~/code. Each is copied with its history and your
     uncommitted changes. Files Git ignores (.env, node_modules) stay on your Mac.
       [x] ~/code/app        412 MB   2 uncommitted files
       [x] ~/code/client-b    88 MB
       [ ] ~/code/old-site   1.1 GB
       … 6 more (space toggles, a selects all)                          [Enter]
4/6  Set up main for agents
     Main starts as a fresh macOS with nothing added. Silo can run these for you over SSH,
     each from its vendor, as you would by hand:
       [x] Xcode Command Line Tools (git, clang)        from Apple, about 1 GB
       [x] Homebrew                                     from brew.sh
       [x] Claude Code                                  from claude.ai/install.sh
       [x] Codex CLI                                    with Homebrew
       [x] tmux, so a run outlives a closed terminal    with Homebrew
       [ ] the 31 Homebrew packages you installed on your Mac (brew leaves)     [Enter]
5/6  Keep your Mac awake while main runs
     On power, Silo keeps your Mac from going to sleep on its own while main is running,
     as `caffeinate -i` does. A MacBook with its lid closed still sleeps on battery, and
     main stops working until it wakes.  [Y/n]
6/6  Reach main from every program on your Mac
     Silo adds one line at the top of ~/.ssh/config:  Include ~/.silo/ssh/config
     Then `ssh main` works in your terminal, your editor and your agent apps.
     Main's entry forwards nothing from your Mac: no SSH agent, no ports.  [Y/n]

That's all. Nothing else is asked until main is ready. You can leave.
  downloading macOS 27.2.1   █████▋               6.8 / 25.6 GB   36 MB/s    9 min left
```

Each question, and where it comes from:

| Question | Plan entry | Note |
|---|---|---|
| 1. User | `moving-in-on-macos` | Apple's provisioning may refuse a user name equal to the Mac's (FB23241619, unverified fix). File 10's workaround makes a setup user first and renames; the question stays the same |
| 2. Where agents work | none | New. Gives the morning screen its first filter, from a fact (a path) |
| 3. Folders to copy | `files-imported-from-your-mac` (first build), `first-run-offers-to-bring-in` (later) | The later decision lists programs, settings and skills too. The first build can offer repositories alone, which is most of the value |
| 4. Set up main | `blank-computer`, `main-from-the-mac-or-a-recipe` (later) | New for the first build; see the challenge below |
| 5. Keep awake | none | New; [`daily-loop.md`](daily-loop.md), gap 2 |
| 6. SSH entry | `reaching-a-computer`, `computer-reached-over-ssh` | The `Include` line must be the first line of `~/.ssh/config` (first-build decisions, finding 7) |

The network question of `safe-fresh-install` is not asked, because the first build has no network
of Silo's own: main runs on Apple's NAT. The first run says so in its last screen instead of asking
a question whose answer it cannot enforce.

```text
  downloading macOS 27.2.1   ████████████████████  25.6 / 25.6 GB   checked against Apple's SHA-256
  installing macOS           ████████████████████  done in 6 min 12 s
  kept the installed system, so the next macOS computer skips the download and the install
  first start                main answers on SSH after 1 min 48 s
  finishing over SSH         your user, SSH key only, no sleep or screen lock in main
  copying 2 folders in       ~/code/app, ~/code/client-b   500 MB
  setting up main            Command Line Tools · Homebrew · Claude Code · Codex · tmux   4 min
  first checkpoint           13:42 · before any agent ran

Main is ready. 31 min since you started, 2 of them yours.

  ssh main.keep        a shell in main that stays when you close the terminal (tmux)
  ssh main             a plain shell

Sign in to each agent program inside main, the vendor's way:
  claude                    opens a sign-in link; paste the code back
  codex login               opens a sign-in link on your Mac (Silo forwards port 1455)

Main's network is open, with nothing between it and the internet yet. It cannot reach your
Mac's files. Silo takes a checkpoint every 10 minutes while main runs.

next: ssh main.keep
```

The "31 min" is the sum of this transcript's steps at 300 Mbps: 13 for the download, 6 for the
install, 2 for the first start, 1 to copy, 4 to set up main (the Command Line Tools take 1 to 3
minutes and run beside the rest) and about 5 for the checks, the
questions and the finish. The set-up step is the one that adds most, and it is the one the plan
leaves to you; see below.

## Signing in, inside main

Each agent program is signed in the vendor's own way inside main (`logins`, `unmodified-agent-
programs`). What happens, from [`macos-first-run.md`](macos-first-run.md):

Abridged; the wording comes from Claude Code's authentication page:

```text
ana@main ~ % claude
Welcome to Claude Code. Select login method:
❯ 1. Claude account with subscription
Browser didn't open? Use the url below to sign in:
https://claude.ai/oauth/authorize?code=true&client_id=…
Paste code here if prompted >
```

- Over SSH the login keychain is locked. Claude Code's documentation says it then keeps the login in
  `~/.claude/.credentials.json`. An issue says it asks for `/login` again over SSH (#29816, closed
  "not planned" by a bot). This is Mac test 2 of the first-build decisions, and the first run's last
  screen depends on it: if the login does not stay, "sign in once" is false.
- Codex keeps its login in `~/.codex/auth.json`. `codex login` waits for the browser's answer on
  port 1455 of the machine it runs on. `--device-auth` works only after you switch device codes on
  in ChatGPT's settings. So the smooth path is Silo forwarding that one port, for the length of the
  sign-in, which is what `sign-in-from-a-computer` describes, and that feature is not in the first
  build. Without it, the first run should print the device-code way and the ChatGPT setting it
  needs.
- `claude setup-token` gives a token valid for a year that only makes model requests. It is the
  vendor's way for machines nobody logs into, and it suits a main that is copied into forks later,
  since it is a value Silo could hold as a secret (`secrets-held-outside`, second build). Not for
  the first run: it is one more concept on the first day.

## The second macOS computer, and every one after

`computer-image` keeps the installed system. A fork or a fresh macOS computer then starts from a
copy of it, with no download and no install. What it still costs is the first start with
provisioning (about 2 minutes) and the set-up step, unless that step's result is kept too. Keeping
the disk after step 4 as well, as a checkpoint named "main as set up", lets a fresh computer start
with Homebrew and the agent programs in about 2 minutes. That is what a recipe will do properly
(`made-from-a-recipe`); until then, a kept checkpoint does it for free.

## Where the plan makes the first run longer

### `blank-computer` with no recipe in the first build

`blank-computer` (open) says a computer no recipe set up "has no agent program, language or tool".
Recipes come in the third build. So in the first build, main starts with no `git`: macOS's
`/usr/bin/git` is a stub that opens a dialog to install the Command Line Tools, and over SSH that
dialog never shows. Nobody can tell why `git clone` hangs. The fix people use is Homebrew's
installer with `NONINTERACTIVE=1`, which installs the Command Line Tools through `softwareupdate`
without a screen, and needs `sudo` without a password or primed.

Proposed: for the first build, a fixed first-run step, "set up main for agents", that runs the
vendors' own installers over SSH as you would, after showing the list. It is not a recipe (it is not
kept, nothing is pinned, nothing is checked against a checksum beyond what each installer does), so
it does not need `recipe-pieces` or `recipe-audit`. When recipes arrive, the step becomes the
recipe "macOS with Claude Code" of `silo-ships-recipes`. It touches `unmodified-agent-programs`
(installed the vendor's way: yes) and `claude-code-fetched-by-recipe` (Silo ships no copy: yes,
it runs Anthropic's installer). Whether passwordless `sudo` for main's user is the default is the
open product choice file 10 already names.

### `first-run-wait` leaves the questions open-ended

`first-run-wait` says the first run "asks its questions" while macOS downloads. Without a limit,
the questions grow with every feature that wants a say at the first run: network, rules, secrets,
budgets, extensions. Proposed: add to it "The first run asks at most eight questions, each with a
default, all before the download ends." A feature that wants a ninth question asks it the first
time it is used instead.

### `moving-in-on-macos` against Apple's provisioning

Covered by first-build decisions file 10. The question for the first run: if the workaround fails,
main's user is not your user name, and paths in your settings that name `/Users/ana` break. The first
run should then say so and list the settings that name the old path, as `moving-in-on-macos` already
asks for a path main does not have.

### `nothing-to-set-up` and `safe-fresh-install` in a build with no network of its own

Both say nothing is allowed out on a fresh install. The first build cannot keep that: Apple's NAT
lets main reach everything. The first run has to say so, as the last screen above does, or it
promises a boundary that is not there. When the network stack arrives, the question comes back:
open, limited or closed. For the first day, open with a proposal of rules from use
(`network-rules-from-use`, taken) is the choice that gets an agent working; closed means the first
`claude` call fails until you allow api.anthropic.com, which is a bad first minute. The plan's
first-run feature already proposes "the rules your agent programs need", which covers it if the
proposal comes before the first call.

## Failures during the first run, as they read

`first-run-failure` (open) gives the shape: check first, say what to do, keep what is done, never
show a half-made main as ready. The three likeliest, written out:

```text
✗ This Mac runs macOS 26.5. Main needs macOS 27 or later on your Mac.
  Update in System Settings > General > Software Update, then run `silo` again.
  Nothing was downloaded or changed.
```

```text
✗ 38 GB free on Macintosh HD. Making main needs 61 GB, and about 35 GB once it is made.
  Disk would show what takes space on this Mac; for now, About This Mac > Storage.
  Nothing was downloaded or changed.
```

```text
✗ Main did not answer on SSH within 5 minutes of its first start.
  Silo kept a picture of its screen: ~/.silo/first-run/screen-13-31.png
  It stopped main and started again from the installed system, which also failed.
  This is the known case where Apple refuses your user name for main. Run
  `silo setup --user ana-main` to try with another name.
  The download and the install are kept; the next try takes about 4 minutes.
```

The third is step 6 of file 10's procedure, written for the person who meets it.

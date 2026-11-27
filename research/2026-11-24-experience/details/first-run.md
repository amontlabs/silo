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
asks for two sign-ins and copies none of your `~/.claude` setup in. Docker's own post of August
2026, "Empty sandboxes break developer experience", says: "Isolation only survives contact with
developers when it is at least as convenient as skipping it". Tart's quick start says it "will
download a 25 GB image". None of the tools read shows the time the download will take before it
starts.

The full table, with sources, is in [`onboarding.md`](onboarding.md).

Silo cannot be as fast as Claude Code on the Mac or Docker's `sbx`: Apple forbids shipping macOS
installed (`computer-image`), so every Mac downloads Apple's restore image. For macOS 27.2.1 that is
25.6 GB. Silo's download is no larger than Tart's, since Tart's prebuilt images are no smaller than
Apple's restore image. Silo then installs macOS from that image, which by others' reports costs
about 5 to 10 minutes more than starting from a prebuilt image. Silo can ask much less of your
attention while it waits, and leave main set up for agents where Tart leaves a bare system.

## The goal, stated as two numbers

1. **Wall time to the first agent working in main**: the download, plus about 18 minutes for the
   install, the first start and setting main up, partly from other tools' users' reports. At
   300 Mbps that is about 31 minutes; at 100 Mbps about 56; at 1 Gbps about 22. With a restore image already on the Mac, about 18. Until it is measured
   on polarzero's Mac, Silo states no time in public (`first-run-time-macos`), and the first run
   shows its own estimate from the speed it measures.
2. **Your attention**: under 3 minutes and at most 8 answers, all asked in the first three minutes,
   while the download runs, so you can leave the Mac for the rest. This is the number to hold the
   design to, since the wall time is mostly Apple's.

`first-run-wait` (open) already says the first run asks its questions while macOS downloads. This
file adds: **all** of them, in the first three minutes, while the download runs, and then nothing
until main is ready.

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
and installs (a few minutes; not measured on this Mac yet). Found nothing to reuse on this Mac.

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
     The repositories you copy in next go in this folder, and agents work in them.
     Changes outside your repositories are shown first each morning.  [~/work] [Enter]
3/6  Copy folders from your Mac into main, once
     Found 9 Git repositories under ~/work. Each is copied with its history and your
     uncommitted changes. Files Git ignores (.env, node_modules) stay on your Mac.
       [x] ~/work/app        412 MB   2 uncommitted files
       [x] ~/work/client-b    88 MB
       [ ] ~/work/old-site   1.1 GB
       … 6 more (space toggles, a selects all)                          [Enter]
4/6  Set up main for agents
     Main starts as a fresh macOS with nothing added. Silo can run these for you over SSH,
     each from its vendor, as you would by hand:
       [x] Xcode Command Line Tools (git, clang)        from Apple, about 1 GB
       [x] Homebrew                                     from brew.sh
       [x] Claude Code                                  from claude.ai/install.sh
       [x] Codex CLI                                    with Homebrew
       [•] tmux, so a run outlives a closed terminal    with Homebrew, always
       [ ] the 31 Homebrew packages you installed on your Mac (brew leaves)     [Enter]
5/6  Keep your Mac awake while main runs
     On power, Silo keeps your Mac from going to sleep on its own while main is running,
     as `caffeinate -i` does. With its lid closed, a MacBook sleeps unless it is on power
     with an external display attached. Main stops until it wakes.  [Y/n]
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
| 1. User | `moving-in-on-macos` | Apple's provisioning may refuse a user name equal to the Mac's (FB23241619, no fix reported). File 10's workaround provisions a setup user, `silo-setup`, creates `ana` with `sysadminctl`, then deletes `silo-setup`; the question stays the same |
| 2. Where agents work | none | New. Gives the morning screen its first filter, from a fact (a path): changes outside the repositories copied in, here `~/work/app` and `~/work/client-b` |
| 3. Folders to copy | `files-imported-from-your-mac` (first build), `first-run-offers-to-bring-in` (later) | The later decision lists programs, settings and skills too. The first build can offer repositories alone, which is most of the value |
| 4. Set up main | `blank-computer`, `main-from-the-mac-or-a-recipe` (later) | New for the first build; see the challenge below. tmux cannot be unticked, because `ssh main.keep` needs it ([`daily-loop.md`](daily-loop.md), gap 1) |
| 5. Keep awake | none | New; [`daily-loop.md`](daily-loop.md), gap 2 |
| 6. SSH entry | `reaching-a-computer`, `computer-reached-over-ssh` | The `Include` line must be the first line of `~/.ssh/config` (first-build decisions, finding 7) |

The network question of `safe-fresh-install` is not asked, because the first build has no rules
to enforce an answer. If the first build runs main on Apple's NAT, as first-build.md proposes as a
fallback when gvproxy is slow (Mac test 5), main reaches everything. The first run says the network
is open in its last screen instead of asking a question whose answer it cannot enforce.

```text
  downloading macOS 27.2.1   ████████████████████  25.6 / 25.6 GB   checked against Apple's SHA-256
  installing macOS           ████████████████████  done
  kept the installed system, so the next macOS computer skips the download and the install
  first start                main answers on SSH after 1 min 48 s
  finishing over SSH         your user, SSH key only, no sleep or screen lock in main
  copying 2 folders in       ~/work/app, ~/work/client-b   500 MB
  setting up main            Command Line Tools · Homebrew · Claude Code · Codex · tmux   4 min
  first checkpoint           13:42 · before any agent ran

Main is ready. 31 min since you started, 2 of them yours.

  ssh main.keep        a shell in main that stays when you close the terminal (tmux)
  ssh main             a plain shell

Sign in to each agent program inside main, the vendor's way:
  claude                    opens a sign-in link; paste the code back
  codex login --device-auth prints a code to enter at a link; switch device codes on
                            in ChatGPT's settings first

Main's network is open, with nothing between it and the internet yet. It cannot reach your
Mac's files. Silo takes a checkpoint every 10 minutes while main runs.

next: ssh main.keep
```

The 10-minute interval between checkpoints is this research's choice. The plan gives none, and
the first-build research says "every few minutes".

The "31 min" is the sum of this transcript's steps at 300 Mbps: 13 for the download, 6 for the
install (others' reports, not measured on this Mac), 2 for the first start, 1 to copy, 4 to set up main (the Command Line Tools take 1 to 3
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
  port 1455 of the machine it runs on. `codex login --device-auth` works only after you switch
  device codes on in ChatGPT's settings. The smoother path is Silo forwarding that one port for the
  length of the sign-in. That is `sign-in-from-a-computer`, which needs `held-requests-and-asks` and
  comes later. So the first build's last screen prints the device-code way and the ChatGPT setting
  it needs.
- `claude setup-token` gives a token valid for a year that only makes model requests. It is the
  vendor's way for machines nobody logs into. Silo does not hold it: `logins` says Silo holds no
  login of a subscription, for any provider.

## The second macOS computer, and every one after

`computer-image` keeps the installed system. A fork or a new macOS computer then starts from a
copy of it, with no download and no install. What it still costs is the first start with
provisioning (about 2 minutes) and the set-up step, unless that step's result is kept too. Keeping
the disk after step 4 as well, as a checkpoint named "main as set up", lets a new computer start
with Homebrew and the agent programs in about 2 minutes. That is what a recipe will do properly
(`made-from-a-recipe`); until then, a kept checkpoint does it for free.

## Where the plan makes the first run longer

### `blank-computer` with no recipe in the first build

`blank-computer` (open) says a computer no recipe set up "has no agent program, language or tool".
Recipes come in the third build. So in the first build, main starts with no `git`: macOS's
`/usr/bin/git` is a stub that opens a dialog to install the Command Line Tools, and over SSH that
dialog never shows. `git clone` fails with "no install could be requested (perhaps no UI is
present)", a message that does not say what to do. The fix people use is Homebrew's
installer with `NONINTERACTIVE=1`, which installs the Command Line Tools through `softwareupdate`
without a screen, and needs `sudo` without a password or primed.

Proposed: for the first build, a fixed first-run step, "set up main for agents", that runs the
vendors' own installers over SSH as you would, after showing the list. It is not a recipe (it is not
kept, nothing is pinned, nothing is checked against a checksum beyond what each installer does), so
it does not need `recipe-pieces` or `recipe-audit`. When recipes arrive, the step becomes the
recipe "macOS with Claude Code" of `silo-ships-recipes`. It fits `unmodified-agent-programs`
(installed the vendor's way). It is a first-build exception to `claude-code-fetched-by-recipe`.
Silo ships no copy of Claude Code, as that entry asks. But running `claude.ai/install.sh` names no
version and checks no checksum, and it skips the three-day wait of `recipe-audit`. Main's user has
`sudo` without a password: `full-access-inside` (taken) says nothing inside asks agents for
approval, and a password prompt for `sudo` is one. That answers the open point file 10 names.

### `first-run-wait` leaves the questions open-ended

`first-run-wait` says the first run "asks its questions" while macOS downloads. Without a limit,
the questions grow with every feature that wants a say at the first run: network, rules, secrets,
budgets, extensions. Proposed: add to it "The first run asks at most eight questions, each with a
default, all in its first three minutes, while macOS downloads." A feature that wants a ninth question asks it the first
time it is used instead.

### `moving-in-on-macos` against Apple's provisioning

Covered by first-build decisions file 10. The question for the first run: if the workaround fails,
main's user is not your user name, and paths in your settings that name `/Users/ana` break. The first
run should then say so and list the settings that name the old path, as `moving-in-on-macos` already
asks for a path main does not have.

### `nothing-to-set-up` and `safe-fresh-install` in a build with no rules

Both say nothing is allowed out on a fresh install. The first build cannot keep that, since it has
no rules. If the first build runs main on Apple's NAT, as first-build.md proposes as a fallback
when gvproxy is slow (Mac test 5), main reaches everything. The first run has to say so, as the last screen above does, or it
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
✗ Main did not answer on SSH within the time limit of its first start.
  Silo kept a picture of its screen: ~/.silo/first-run/screen-13-31.png
  Silo stopped main and started it once more from the installed system. It did not answer.
  The download and the install are kept. Run `silo setup` to try again.
```

The third is step 6 of file 10's procedure, written for the person who meets it. It reports what
Silo saw and does not guess the cause. The time limit is set from measured first starts (file 10,
test 2). A refused user name is a different case: Apple's `guestProvisioningInvalidUsername` is
caught before the start and handled by the setup-user path of file 10, which keeps your name.

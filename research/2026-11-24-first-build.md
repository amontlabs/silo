# The smallest first build of Silo

An agent's proposal, 24 November 2026, for polarzero to review. It changes nothing in `plan/`: no
verdict, no answer and no order is edited. Every id below is an entry of the plan, and
`plan show <id>` gives it. The counts come from reading `plan/` at `1b8f1917a` with a script, and
can be checked the same way.

This is the second version. The first was written before the rest of the day's research. This one
takes in macOS 27's guest provisioning and the drafts #350 to #360, and says under "What changed
since the first version" which of them moved what.

## In short

Build first the part of Silo that lets you run Claude Code or Codex with every check off on your
Mac overnight and lose nothing: main as a macOS virtual machine, reached over SSH, with checkpoints
taken on their own, restore, and a record of which files changed between two checkpoints. It is
read from the `silo` command. There is no app, no rules, no model traffic and no Linux yet.

That is 14 features of the plan and 3 new ones. Before they can be built, 70 open decisions need a
verdict: the 62 the 14 features need, the 7 new decisions this proposal adds, and
`record-queried-with-ordinary-tools`. The other 243 open decisions can wait. Eleven of the 70 are
about how Silo is built, and agents could take them with a judge, as `owners-part-on-engine`
allows. Draft #352 gives a second opinion and the exact text for 56 of them.

The same PR writes the new entries into `plan/`, cuts three `needs`, and moves what the first build
needs to the front of `order.json`, so the Workshop shows polarzero these decisions first. No
verdict is written.

Five things the plan does not say yet decide whether that night goes well:

- **Your Mac must run macOS 27.** It is the only version where a new macOS computer gets its user
  and SSH with nobody clicking (`VZMacGuestProvisioningOptions`). Your Mac ran macOS 26.5 on
  21 November, so updating it is the first step, for the build and for the Mac tests.
- **The night has to survive the Mac and the terminal.** A run started over SSH dies when the
  terminal closes, and main stops when the Mac sleeps. A closed MacBook sleeps unless it is on
  power with an external display.
- **Main starts with nothing in it.** A fresh macOS has no `git`, no Homebrew and no agent program,
  and nobody sees the dialog that `git` opens over SSH.
- **Restore puts every agent back.** With several agents in main and no forks yet, bringing back one
  deleted folder costs everyone's night. Reading one path from a checkpoint fixes the common case.
- **The disk fills.** Main's disk image, its checkpoints and the engine's files share one APFS
  volume, and when it is full, the engine can no longer restore or remove a checkpoint.

## Where the plan stands

| | Count |
|---|---|
| Entries | 626 |
| Decisions | 492: 160 taken by polarzero, 23 by agents, 3 dropped, 306 with no verdict |
| Features | 134, none with a verdict |
| Features ready to build | 0 |

Every feature needs at least one decision with no verdict, except one. `access-model-checked` needs
only taken decisions, so it is the one feature polarzero could take today and an agent could start
at once.

The owner is shown the open decisions in the order of the stage `ground`. The next one is
`word-agent`. The decisions the first build needs sit among the first 157 of the 293 open decisions
of that stage, so in the current order about 100 decisions that the first build does not need come
before its last one. Most of those are about words, principles and later parts, such as
`bad-paths-principle`, `escalation-question` and the boundary's holds.

## The first build

### What you do with it

You install Silo on a Mac running macOS 27. Silo downloads Apple's restore image for macOS 27
(25.6 GB, about 13 minutes at 300 Mbit/s), or reuses one already on the Mac, such as Tart's. It
installs macOS in a virtual machine and keeps that installed system before its first start. On the
first start, Apple's provisioning creates the user and turns SSH on. Silo then installs, over SSH
and after showing you the list, the Command Line Tools, Homebrew, tmux, Claude Code and Codex, each
with its vendor's own installer. It writes two SSH entries, `main` and `main.keep`, and keeps the
Mac awake while main runs on power.

You run `ssh main.keep`, which opens tmux in main. You clone two repositories, start Claude Code
with `--dangerously-skip-permissions` in one tmux window and Codex with its sandbox off in another,
and close the terminal. The agents go on. You go to bed.

In the morning, Codex has deleted a folder of the other repository and broken Homebrew.
`silo checkpoints` lists the checkpoints taken every five minutes, each named by its time.
`silo changes 02:40 03:10` prints what was created, changed and deleted between two checkpoints,
with what macOS writes by itself hidden. `silo look 02:40 ~/work/app/src` shows the folder as it
was, and `silo put-back 02:40 ~/work/app/src` copies it back into main after taking a checkpoint of
now. Claude Code's night is untouched. For Homebrew, which spreads over the whole disk, you run
`silo restore 02:40`: Silo stops main, saves where you were, puts the whole disk back with the
conversations, and starts main again. You take each conversation up again with `claude --resume`.

That case is the promise of `the-shape` without any of the parts that make Silo large. It answers
the most frequent incident in the bug trackers, an agent deleting files on its own machine (#352,
finding 8; #354, the ranked pains). It answers none of the incidents in the news, which destroyed
something outside the machine through a token. Those need the boundary, which is the third build.

### The features

In the order they would be built:

| Feature | Why it is in the first build |
|---|---|
| `access-model-checked` | Its needs are all taken, `model-of-access-first` asks for it first, and it is small |
| `engine-program-and-clients` | Everything talks to the engine |
| `engine-matches-wanted` | Main comes back after the Mac restarts. Apple's framework is not safe in a daemon, so main comes back once you log in, and the feature should say so |
| `computers-main-on-a-mac` | The computer itself |
| `engine-outlived-by-computers` | Without a helper per computer, every crash of the engine is a power cut of main in the middle of the night. It adds 4 open decisions |
| `agent-programs-run-unmodified` | You install the agent programs the vendors' way |
| `agent-programs-reach-every-computer` | `ssh main` is the way in, and it is how every editor and agent app reaches main |
| `computers-stopped-and-paused` | `restore-to-a-checkpoint` needs it |
| `checkpoints-taken-on-their-own` | The way back |
| `thinning-old-checkpoints` | The engine's tables are fixed, so with no thinning the checkpoint table fills in days. It adds 2 open decisions |
| `restore-to-a-checkpoint` | The way back for the whole computer |
| `record-of-a-computer` | Needed by every line of the record |
| `file-changes-in-the-record` | The first lines worth reading, and they come from comparing checkpoints, which the first build already takes |
| `files-imported-from-your-mac` | Nothing is shared from your Mac (`device-not-shared`), so this is how a file you have gets in |

Seven entries the plan did not have belong to the first build. This PR adds them to `plan/`, each
with its choice in its notes, as one feature or as decisions with a feature:

| New entry | What it says | From |
|---|---|---|
| The command line first: `silo-command` | Every feature above is reached from the `silo` command from the day it is built. With no argument it prints what needs you; it prints text for a person and JSON lines with `--json` | #354 (`details/cli.md`) |
| Which Mac Silo needs: `mac-silo-needs` | Apple silicon and macOS 27 or later. On an older Mac the first run says so and starts nothing | #352 (05, 10) |
| Main set up for agents: `main-set-up-for-agents` | The first run installs the Command Line Tools, Homebrew, tmux and the agent programs over SSH, after showing the list. It becomes the shipped recipe later | #354 |
| A run outlives its terminal, and the Mac stays awake: `runs-outlive-the-terminal`, `mac-kept-awake`, `agents-work-through-the-night` | A second SSH entry per computer opens tmux. While a computer runs on power, Silo holds the Mac awake, warns about a closed lid, and writes every sleep and wake in the record | #354, #360 |
| Reading one path from a checkpoint: `putting-back-a-path`, `a-path-from-a-checkpoint` | `silo look` shows a path as it was in a checkpoint, and `silo put-back` copies it into the running computer after a checkpoint of now | #354 |
| Space the engine keeps for itself: `space-the-engine-keeps` | A reserve file only the engine may delete, and checkpoints stop before the volume is full, so a full disk never leaves you unable to restore | #352 (02, 07) |
| Main's network in the first build: `network-before-rules` | Open from when you make main, with your yes, and shown as open with no boundary in the first run and in `silo status` | #352 (06, 12), #354 |

`silo put-back` is a partial rewind of main outside Git. `one-run-grain` and
`putting-back-a-runs-paths` (taken) already allow putting back paths; `fork-from-the-moment` (taken)
reads against it. See "Challenges" below.

### What is left out, and why

| Left out | Where it comes |
|---|---|
| Forks, the bin, a fork becoming main (`forking-a-computer`, `dropping-into-the-bin`, `fork-replacing-main`) | Second build. A fork needs `fork-identity`, `fork-and-sign-in` and `what-a-fork-is-given` |
| Rules, holds, secrets, a limited or closed network (`allow-hold-or-deny`, `open-limited-or-closed`, `secrets-held-outside`) | Second and third builds. `open-limited-or-closed` alone pulls in 56 open decisions, because it needs the whole rule engine |
| Model traffic and spend (`model-traffic-read`, `spend-per-conversation`) | Third build. Spend is among the top pains (#354), but reading traffic needs 77 open decisions and TLS in a macOS computer is untried (#358 found request smuggling past the #347 spike) |
| Linux beside main, recipes (`computers-linux-beside-main`, `made-from-a-recipe`) | Third build. Main on macOS does not need them |
| Processes and runs (`processes-in-the-record`, `what-each-run-wrote`) | Second build. They need the probe inside macOS, which `macos-ships-facts-first` holds back |
| Opinions, classifiers, extensions, views, receipts, tasks, the phone | After the record has a few kinds of lines to judge |
| The simulator of the whole product (`simulated-core-histories`) | Built beside the first build, not before it. `core-simulator` (taken) is built with the engine from the first day |

## What has to be decided first

### Three needs to cut

Without these cuts, the first build pulls in recipes and fork sign-ins. Each cut is an edit of one
`needs` list.

| Feature | Need to remove | Why |
|---|---|---|
| `agent-programs-run-unmodified` | `agent-program-data-file`, `offered-programs-list` | A program you install yourself runs without a data file, as `unmodified-agent-programs` already says. The data file needs `recipe-pieces` and `recipe-audit`, which drag in the recipe part. The features that read the data file, such as `agent-programs-logins-kept-out-of-copies`, should need it instead |
| `restore-to-a-checkpoint` | `conversation-taken-up-again` | A restore puts the disk back whether or not a program takes up its conversation. The decision already says Silo claims it only for the programs it has checked. It needs `conversation-resumes-in-a-copy`, which needs `fork-and-sign-in`, a fork decision |

`conversation-resumes-in-a-copy` could also be split: the case of a restore on the same computer
needs no new sign-in, while the case of a fork does.

### The decisions

The 7 new decisions are in `plan/` with their choices. Draft #352 has a ruling sheet for the 56 of
the first version: 9 to take as written, 45 to take with an edit,
1 to replace (`core-state-stays-small`) and 1 to split (`computer-image`), each with its exact text.
Read its "Contradictions" section first: it says which text to adopt where two second opinions
touch the same entry, and it asks for four engine entries to be ruled in one sitting
(`numbered-events`, `call-ids`, `formats-carry-versions`, `engine-saved-state`).

**Could be taken by agents with a judge** (sets nothing, about how Silo is built, no word of
polarzero's in its notes): `core-dependencies`, `core-knows-no-agents`, `core-state-stays-small`,
`engine-inputs`, `engine-state`, `engine-source-checks`, `events-until-the-recorder`,
`record-speed-cost`, `sources-number-their-lines`, `tigerstyle-by-place`, `own-cost-targets`.
`events-until-the-recorder` decides whether checkpoints go on when the recorder stops, so it is worth
polarzero's eye anyway (#352, "Contradictions").

**For polarzero**, grouped so that one sitting covers a part:

- How Silo is built, with polarzero's own words in the notes: `how-the-engine-is-built`,
  `measuring-the-store`, and `engine-saved-state`, which #352 moves here because the engine
  stopping on a failed flush is something you see.
- The engine's shape and its promises: `engine-shape` (sets six words), `deterministic-core`,
  `engine-limits`, `engine-reconciles`, `how-effects-work`, `numbered-events`, `call-ids`,
  `formats-carry-versions`, `who-can-call-the-engine`, `network-stack`.
- Main: `main-on-macos-first`, `one-main-or-several`, `neutral-core`, `computer-image`,
  `first-run-time-macos`, `files-through-a-channel`, `pause-every-computer`, `what-stop-ends`.
- Main when the engine stops (for `engine-outlived-by-computers`): `processes`,
  `when-the-engine-stops`, `which-process-holds-the-boundary`, `where-rules-run`.
- Reaching main and the agent programs: `computer-reached-over-ssh`, `reaching-a-computer`,
  `documented-surfaces`, `unmodified-agent-programs`.
- Going back: `checkpoint-per-system`, `fork-shares-disk-blocks`, `checkpoint-storage`,
  `restore-saves-first`, `what-a-restore-leaves`, `why-destructive`, `meanings-for-computers` (sets
  ten words), `thinning-checkpoints`, `kinds-of-other-actions`.
- The record: `word-line`, `word-gap`, `sources`, `kinds-of-lines`, `who-made-each-change`,
  `changes-to-the-record`, `enforcing-and-recording`, `observing-and-waiting`,
  `record-path-and-core`, `git-history-inside`, `file-changes-from-outside`,
  `record-overhead-limit`.
- The product: `silo-is-light`.

Three of #352's edits matter most for the first build:

- `thinning-checkpoints` keeps every checkpoint of the last 2 hours today. For the overnight case it
  should keep every one of the last 24 hours, then thin, and never remove the save made before a
  restore or a checkpoint whose comparison is not written yet.
- `restore-saves-first` should stop main first, then save, so the save holds what the agents wrote
  up to the stop.
- `what-a-restore-leaves`: the key Silo uses to reach main is in `authorized_keys` on main's disk,
  so a restore brings back whatever key was there. For the first build, that key never changes for
  the life of the computer.

### Build order

Recommended edits to `plan/order.json`, none made here:

This PR makes these edits to `plan/order.json`:

1. In the stage `ground`, everything the first build needs (110 entries, 70 of them open) comes
   first, in its old order, with each new decision after what it needs.
2. In the stage `app`, the 17 features come first, in the order of the table, with the new ones
   beside the features they serve. `simulated-core-histories` now follows them.
3. `what-to-say`, `presenting-it` and `objections` move to the end of `app`. They are about the
   public, and the first user is polarzero (`who-it-is-for`).
4. `matching-reports-to-facts`, `precaution-on-unsure`, `how-a-hold-ends`, `reason-in-a-refusal`,
   `denied-tool-call-mechanism`, `denial-per-program` and `terms-on-changing-a-reply` move to just
   before the first feature that needs them. They sat at the top of `app`, ahead of the engine.

`plan check` passes with the full history.

## What changed since the first version

| Draft | What it changes in this proposal |
|---|---|
| macOS 27 (WWDC 2026, session 224) | `VZMacGuestProvisioningOptions` (full name, user name, password, automatic login, Remote Login) passed in `VZMacOSVirtualMachineStartOptions` on the first start. Apple honours it only while the guest is not set up yet, and it needs macOS 27 on the Mac and in the guest. The Setup Assistant risk of the first version is gone, at the price of a minimum macOS |
| #352, second opinions | The ruling sheet for the 56. libfsapfs, which the first version named, could not open a volume with 512 snapshots and refused a half-written APFS checkpoint, so the reader has to be Silo's own. The engine must call `F_FULLFSYNC` itself, because Zig's `fsync` leaves data in the drive's cache on a Mac. gvproxy maps an address in the computer to your Mac's 127.0.0.1 by default, which Apple's NAT does not reach |
| #354, the first run and the daily loop | The night's gaps: the terminal, sleep, an empty main, the cost of a whole restore. `silo look` and `silo put-back` |
| #355, fixes with Linux prototypes | Nothing for the first build. It matters for the second, where secrets are swapped only in their credential slot |
| #358, red team | gvproxy's API is reachable from inside without a token: one `curl` to `192.168.127.1` made it listen on all the Mac's addresses. `network-stack` needs the token, whichever build uses gvproxy |
| #359, the Mac test kit | The tests below can be run with one command. Its VM tests need macOS 27 |
| #360, notes | People who run agents overnight use a Mac mini that stays on; a closed laptop sleeps. `simulators-for-parallel-agents`: two agents in one macOS computer clash over the iOS simulator |
| #350, #351, #353, #356 | The pains, the record and budgets, destructive calls and boundary questions. They shape the second and third builds, not this one |

## What only a Mac can answer

Nothing has run on a Mac yet. #352 merged the open questions into 21 Mac tests, and #359 turned them
into a kit: `./kit run quick` needs no virtual machine and runs on macOS 26.5; everything else needs
macOS 27. These are the ones that can change the first build, in the order to run them:

1. **The first run on macOS 27.** Download, install, provisioned first start, timed. Provisioning
   once with your Mac's own user name and once with another, since a developer reported that the
   same name fails (FB23241619), and `agent-programs-moving-in` wants the same name.
2. **Claude Code and Codex signed in over SSH.** Sign in, disconnect, reconnect, restart main,
   restore a checkpoint. Claude Code's documentation says that with the keychain locked, as over
   SSH, the login goes to a file; an issue reports it asking for `/login` again. This settles
   whether "start Claude Code and go to bed" works.
3. **APFS clone time against fragmentation.** #347 and #352 measured on XFS that a clone of an image in
   500,000 pieces made one 4 KiB write inside wait 10.7 s, and that taking a checkpoint after each
   step kept the image near its worst. APFS keeps one record per piece too. The checkpoint's
   target should be the longest a write in main waits, not the clone's time. macOS 27's
   DiskImageKit adds overlay layers to disk images, but the layers of an attached disk cannot change
   while the computer runs (#352 read UTM's source), so a layer per checkpoint is not an option; a
   layer per boot is.
4. **Is a clone of a running disk a moment in time,** in each caching and synchronization mode of
   the disk's attachment, or does a checkpoint need a pause.
5. **The cost of `F_FULLFSYNC`,** grouped and not, and whether an overwrite after a clone takes new
   space. It sets the engine's latency target and whether the space reserve is needed.
6. **gvproxy under a macOS computer.** DHCP, MTU, `ssh main` through a Unix socket, `git clone` and
   `npm install` against Apple's NAT, with its API token set and its map to 127.0.0.1 removed.
   macOS 27 also adds `VZVmnetNetworkDeviceAttachment` with networks an app makes itself (DHCP,
   port forwarding). That gives a private network for main with no NAT to your Mac, but no rule per
   request, so it is not the boundary `network-stack` asks for. It can be measured as a third
   option for the first build, where main's network is open anyway.
7. **Starting main from a launchd agent and from a daemon,** and restarting a Mac with FileVault on.
   It decides whether main can come back before you log in.
8. **What macOS writes by itself.** An idle main for a night, to set what `silo changes` hides by
   default and the reference for every cost target.
9. **Reading main's disk from outside.** #352 built a 250-line prototype that compares two
   checkpoints by the address of each leaf block, never mounting the disk. It matched a full walk
   on 711 of 711 pairs of snapshots and read 6,260 blocks instead of 45,014. It has not run on a
   disk of millions of files. A disk it cannot read, such as one the agent encrypted with
   FileVault, is a gap, never "nothing changed".

## Challenges to decisions in force

As `AGENTS.md` asks. Each stays in force until polarzero changes it.

- **`fork-from-the-moment`** says that on a main shared by several agents you do not rewind; you
  fork from the moment, and the result leaves the fork through Git. `one-run-grain` and
  `putting-back-a-runs-paths`, also taken, already allow putting back one run's paths, so the new
  `putting-back-a-path` rests on those. The case: at 03:00 Codex
  deletes `~/work/app/src`, which it never committed, while Claude Code works through the night in
  another repository. The first build has no fork, so the only way back is `silo restore`, which
  undoes Claude Code's night too. With forks (second build) the way is a fork at 02:40, then
  copying the folder out, which is not Git either, since the folder was never committed. If polarzero reads
  `fork-from-the-moment` as forbidding `silo put-back`, the way is a decision that replaces it with
  both sentences.
- **`model-of-access-first`** says the first engine code is the part that decides who can get
  what. In the first build almost nothing is granted: your Mac's way in over SSH, and files you
  import. A model of access checked on every short history is worth most when there are secrets,
  hosts and rules to grant, which is the second build. The feature is small and ready, so the cost
  of building it first is low; the risk is that its tables are shaped before any real grant exists
  and are redone when secrets arrive. A possible replacement: the access model is written first,
  as it says, but against the grants of the second build (secrets, hosts, the SSH way in), written
  down before its code.
- **`network-stack`** has no verdict, but `computers-main-on-a-mac` needs it, so main cannot start
  until Silo has a network stack of its own. gvproxy as a driver needs its API token and its map to
  127.0.0.1 removed (#352, #358). If Mac test 6 shows it is slow or fragile, main could start on
  Apple's NAT or a vmnet network for the first build, shown as having no boundary, and move to the
  stack in the second build. `network-stack` would then have to say so.
- **`app-as-text`** needs `now-in-the-app`, so the command line waits for the app. For the first
  build the order is the other way: the command line is the only surface, as
  `silo-command-reads-the-app` (taken) allows.
- **`engine-pace`** ("slowly on the engine") has no verdict, and its notes hold polarzero's own
  words. This proposal does not ask to build faster. It asks that the slow, checked work be done on
  the pieces the first build uses, and not on the 243 decisions it does not.

#352 lists eleven more challenges, among them `core-simulator`, `idle-memory` (a macOS guest may
not answer Apple's balloon) and `agents-drive-the-screen` (on macOS 27 a program started over SSH
cannot get Accessibility). None of them blocks the first build.

## After the first build

These are sketches, to show that the first build leads somewhere.

- **Second build: forks and the edge.** `forking-a-computer`, `dropping-into-the-bin`,
  `agent-programs-conversation-in-a-fork`, an open or closed network split out of
  `open-limited-or-closed` without host lists, `hosts-reached-and-denied` from the network stack,
  `processes-in-the-record` on macOS, and `secrets-held-outside` with #355's swap in the credential
  slot. This is where a fork with no secrets and a closed network lets an agent try a tool you do
  not trust.
- **Third build: traffic, rules and Linux.** `allow-hold-or-deny`, `held-requests-and-asks`,
  `model-traffic-read`, `model-calls-in-the-record`, `spend-per-conversation`, holds on named
  destructive calls (#353), `made-from-a-recipe`, `computers-linux-beside-main`.
- **Then the app's places and the phone** (`now-in-the-app`, `timeline-in-the-app`,
  `what-needs-you`, `phone-app`), once the record holds files, processes, hosts and model calls to
  show. #354 asks that an ask nobody answered stays on the morning list instead of vanishing.

## Sources

- The plan at commit `1b8f1917a` of `amontlabs/silo`, read with a script (`needs` followed through
  every entry).
- Drafts #347 and #350 to #360 of `amontlabs/silo`, read on 24 November 2026.
- Apple, WWDC 2026 session 224, on `VZMacGuestProvisioningOptions`, DiskImageKit and
  `VZVmnetNetworkDeviceAttachment`, as relayed from the boundary research and #352 (file 10). Not
  read here first hand.
- Apple, `VZDiskImageStorageDeviceAttachment` and ASIF:
  https://developer.apple.com/documentation/virtualization/vzdiskimagestoragedeviceattachment and
  https://eclecticlight.co/2025/06/12/macos-tahoe-brings-a-new-disk-image-format/
- Apple, on `VZFileHandleNetworkDeviceAttachment` and a network stack of one's own:
  https://developer.apple.com/forums/thread/702319
- boxlite's gvproxy backend, on the vfkit protocol over Unix datagram sockets on macOS:
  https://docs.rs/crate/boxlite/0.9.5/source/src/net/gvproxy/mod.rs

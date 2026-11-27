# The first build, weighed against what sets Silo apart

An agent's comparison, 24 November 2026, for polarzero. It changes no verdict, answer or order.
Every id is an entry of the plan, and `plan show <id>` gives it. The counts come from following
`needs` through every entry with a script, on the tree of #348, which already holds that draft's
new entries and its cut needs.

## In short

#348 proposes a first build with main as a macOS computer, checkpoints taken on their own, restore,
and which files changed. The day's research found that what nobody else does is the record from
outside the computer: what the agents ran, changed, sent and spent, and their own claims checked
against it (#351, #371, the competitor survey). In #348 that record arrives with the second and the
third builds. A first user of #348 gets less of an account of the night than a macOS VM product that
already exists and logs each model request.

Four first builds are compared below:

| | A. #348 plus the record of model traffic | B. Linux main on the Mac first | C. #348 as it is | D. No VM: a record on the Mac itself |
|---|---|---|---|---|
| The computer | macOS main | Linux main | macOS main | none |
| Going back | checkpoints, restore | checkpoints, restore | checkpoints, restore, one path put back | none |
| The record | files, hosts, model calls, spend, a receipt per conversation | the same, with processes | files | model calls, from inside the agent's reach |
| Features | 19 | 20 | 17 | not counted |
| Open decisions before building | 88 | 94 | 70 | not counted |
| Steps of #370's specs reused | 1 to 4, most of 5 | 1, and the host side of the record | 1 to 5 | none |
| The part only Silo has | in the first build | in the first build, fullest | from the second build | no: the record is inside the blast radius |

**Recommended: A.** It keeps the computer polarzero works on (`who-it-is-for`) and #348's way back,
and adds the part only Silo has, for 19 more open decisions, most of them already answered with a
trial behind them. Build the boundary first against a Linux test computer, while the Mac kit (#359)
answers the macOS questions. If the Mac kit shows that macOS 27's provisioning or gvproxy under a
macOS computer does not work, B is the fallback, and it keeps every piece built for the boundary
and the recorder.

To rule on A in one sitting, two more pages: [decisions.md](decisions.md) gives a second opinion
on each of the 19 decisions A adds, with the exact text to take, and [specs.md](specs.md) says what
A changes in #370's build specs and specifies its one new step.

## One night, under each option

You leave Claude Code and Codex in main for the night. Claude Code ends with «All tests pass.», but
the whole suite exited 1 and it ran only the auth tests after that. Codex deletes a folder outside
the repository and sends a log to a paste site. The night spends a third of the week's plan.

| In the morning, you see | A | B | C |
|---|---|---|---|
| The deleted folder, and the checkpoint before it | yes | yes | yes, and `silo put-back` |
| Each command the agent ran, with the exit code its program sent to the model | yes, as a report read from the traffic | yes | no |
| «All tests pass.» beside the suite's exit 1 | yes | yes | no |
| The paste site reached, and how much was sent | yes | yes | no |
| What each conversation spent, and the plan's share used | yes | yes | no |
| A process left running after the run ended | no: it needs a probe in macOS | yes, from the probe | no |
| Mac or iOS work in main, with Xcode | yes | no | yes |

A reads the exit codes from the tool results that the agent program sends back to its model. That
is the agent program's own report, so it is marked as a report, but it is read outside the computer
and nothing inside can edit it after it is sent. #371's receipt showed both: "the exit sent to the
model and the exit the probe saw". A has the first, B has both.

## A. #348, plus the record of model traffic

**Features.** #348's 17, without `a-path-from-a-checkpoint` and `files-imported-from-your-mac`,
plus `model-traffic-read`, `model-calls-in-the-record`, `spend-per-conversation` and a receipt read
from the `silo` command. `receipts-of-work` as written needs processes, hosts with rules, the app
and `how-a-path-comes-back`, so the receipt is a split of it: the same receipt, holding only what
the record has. Put-back and import come right after; both are small once the reader exists.

**Decisions.** 88 open: #348's 70 without `putting-back-a-path`, plus these 19:
`ending-the-encrypted-connection`, `forwarded-byte-for-byte`, `what-silo-changes-in-traffic`,
`reading-each-agent-program`, `subscription-and-key-alike`, `provider-sees-silos-connection`,
`anthropic-reads-intermediate`, `logins`, `fork-and-sign-in`, `program-that-cannot-be-read`,
`hidden-reasoning-stays-closed`, `recognising-a-conversation`, `what-traffic-gives`,
`how-spend-is-shown`, `network-place`, `what-a-receipt-is`, `what-a-receipt-holds`,
`agent-summary-beside-receipt`, `conversation-text-kept`. Most have a trial in #347, #367, #369 or
#371 in their notes.

One need has to be cut, as #348 cut three. `model-traffic-read` needs `secrets-held-outside`, which
needs `open-limited-or-closed` and with it the rule engine: 18 more open decisions and three more
features. Reading traffic does not need a secret held outside. A subscription's sign-in stays in
the program (`logins`), and an API key the program keeps in main is read like any other request.
Holding that key outside is the second build. `fork-and-sign-in` comes in through `logins` and is
about forks, so `logins` could drop it from its needs the same way.

**Effort, in #370's steps.** Steps 1 to 4 as specified. Step 5 without `look`, `put-back` and
`import`. One step changes and one is added:

- Step 2 carries main's network on Silo's stack from the first day, as `network-stack` answers,
  and not on Apple's NAT as #370 does until later. The record of hosts and model calls is a fact
  only if every packet passes the boundary. That pulls #359's test 07 (gvproxy under a macOS
  computer) and #358's fix (gvproxy's API reachable from inside without a token) into step 2.
- A new step, model traffic: ending TLS for the providers' hosts, Silo's authority in main
  (#359's test 21), conversations, prices and the receipt. It has no spec yet. Its parts exist as
  trials: the Zig boundary spike of #347, the 100-line patch against smuggling of #358, the reading
  of Codex's WebSocket in #347's holds, and #371's matching of tool calls and claims.

**Risks.**

- Everything #348 risks on the Mac, plus tests 07 and 21 becoming blocking. If macOS will not trust
  Silo's authority without a person at the screen, each agent program gets it in its own
  environment (`NODE_EXTRA_CA_CERTS`, `CODEX_CA_CERTIFICATE`), which test 21's outcomes already
  describe.
- Reading the traffic of a Claude subscription is documented by Anthropic for company proxies, but
  one clause of its usage policy leaves "authorized" undefined (`anthropic-reads-intermediate`).
  For polarzero's own use the record stays on polarzero's Mac. Before Silo goes to anyone else,
  Anthropic's answer is needed. Reading an API key's traffic, and a ChatGPT sign-in's, is fine on
  the providers' own words.
- The plan's share of a subscription is read from headers known from community captures, not from
  Anthropic's documentation (#351). Spend in dollars at API prices does not depend on them.
- Four programs that call their model through the vendor's servers (Cursor, Amp, Kiro, Windsurf)
  give no model calls (#367, #369). Claude Code and Codex, which polarzero runs, do.

**Against other products.** It lands as a macOS computer on your Mac whose account of the night
comes from outside it. The macOS VM products found give a computer, an allowlist and checkpoints
by hand, and the closest one logs each model request with its tokens. None compares files between
checkpoints, takes checkpoints on its own while agents work, or checks an agent's claim against what it ran. The
Linux microVM products on the Mac log the network at most.

## B. A Linux main on the Mac first

**Features.** The same record as A, with `processes-in-the-record` from the Linux probe, on a main
that is a Linux computer under libkrun. No feature of the plan says this: `computers-linux-beside-main`
needs a macOS main and a recipe. It needs a new feature, or `main-on-linux` (stage later) moved
forward with an answer that starts on Linux, and `main-on-macos-first` answered the other way.
`processes-in-the-record` needs `processes-from-inside-macos` too; the count leaves that out, as
B has no macOS computer.

**Decisions.** 94 open, counted with a stand-in feature for the Linux main that needs what
`computers-linux-beside-main` needs, apart from the macOS main and the recipe. Against A, it drops
`mac-silo-needs`, `first-run-time-macos`, `main-on-macos-first` and `files-through-a-channel`, and
adds `linux-vm-engine`, `work-on-top-of-libkrun`, `libkrun-patches`, `engine-patch-review`,
`main-on-linux`, `blank-computer`, `x86-linux-computers`, `containers-on-macos`,
`storage-between-computers` and `processes-from-inside-linux`.

**Effort.** Step 1 and the host side of the record and the boundary carry over. Steps 2, 4 and 5
are written again for Linux: a Zig helper on libkrun's C interface, #365's bitmap of written blocks
in place of APFS clones, and an ext4 comparison in place of the APFS reader. The probe needs a
kernel of Silo's own configuration, since the stock libkrunfw kernel has no eBPF, kprobes or BTF
(#351). Then a macOS main is built later, so steps 2, 4 and 5 are written twice in all.

**Risks.** Fewer unknowns on the Mac: no macOS 27, no provisioning, no APFS reader, no Endpoint
Security entitlement. More of it has run already: #347, #367, #371 and the boundary spike all ran on
Linux. Against that, `linux-vm-engine`'s five measurements have not run on a Mac, and polarzero's
own work in main cannot use Xcode or macOS apps.

**Against other products.** It lands beside the Linux microVM products on the Mac, which are free
and well known, and the record and the receipt are the only reason to choose it. It gives up the
macOS computer, which is what draws developers of Mac and iOS software (#361).

## C. #348 as it is

**Features and decisions.** 17 features, 70 open decisions (#348).

**Effort.** Five steps, all specified in #370, each with its Mac tests.

**Risks.** The Mac unknowns of #348: macOS 27's provisioning, a write's wait during an APFS clone,
whether a clone of a running disk is a moment in time, the reader on a large disk. None about
traffic or terms.

**What a first user gets.** A full way back on macOS, which answers the most frequent pain found
(#350, #351: files deleted on the agent's own machine), and the list of files that changed. No
account of what the agents ran, sent or spent.

**Against other products.** Checkpoints on their own, on a Mac, of a macOS computer, are new. The
rest is what macOS VM products already give, and one of them logs model requests, which C does not.

## D. A record on the Mac itself, with no VM

A proxy on the Mac in front of the agent programs, with no computer of their own. It is the
smallest build, and it fails on the decisions in force: the record would sit inside the agent's
reach (`security-framing`, `rules-inside-are-no-boundary`, `full-access-inside`), a child process
can skip the proxy, as the $453 runner did (#351), and there is no way back. It also competes with
the agent programs' own logs. Not recommended. Its one use is as a test bench for the boundary
while the Mac kit runs, which is part of the build order below.

## Recommended build order for A

The boundary and the recorder run on the Mac, outside every computer, and they do not depend on
the computer's system. So they can be built before main exists.

1. #370's step 1: the engine, the recorder and `silo`.
2. The boundary and model traffic, tested against a Linux test computer or #371's rig, while
   polarzero runs the Mac kit. This is where `model-of-access-first`'s checked access model gets
   real grants to check: hosts read and the authority's key.
3. #370's step 2, with main on Silo's stack and the boundary in front of it.
4. #370's steps 3 and 4: a night, checkpoints and restore.
5. #370's step 5 without `look` and `put-back`, and the receipt in `silo`.
6. Right after: `a-path-from-a-checkpoint`, `files-imported-from-your-mac`, then #348's second
   build.

If the Mac kit fails at step 3 on provisioning or gvproxy, steps 1 and 2 stand, and B's Linux main
takes step 3's place.

## Plan edits this would need

Proposals, none made in this PR:

- A new feature in `views`, split from `receipts-of-work`: a receipt of each conversation in the
  `silo` command, from what the record has, and saying what it does not have yet (processes, rules).
- `model-traffic-read` without `secrets-held-outside` in its needs, and `logins` without
  `fork-and-sign-in`.
- In `order.json`, the 19 decisions above inside #348's first-build block of `ground`, and the
  three traffic features and the receipt inside its block of `app`.
- `network-before-rules` (#348) and #370's step 2: main on Silo's stack from the first day.
  `network-stack`'s answer already says "never Apple's NAT".

## What only a Mac can answer for A

On top of #348's list: #359's test 21 (whether Claude Code and Codex trust an authority in main's
keychain, and whether one can be added over SSH), test 07 (gvproxy under a macOS computer), and one
not in the kit yet: Claude Code signed in with a subscription, in main, through the boundary, for a
whole night, with its conversations recognised and its plan's headers read.

## Sources

- The plan at `1b8f191`, and #348's tree at `b45be1c`, read with a script that follows `needs`.
- Drafts #347, #348, #350, #351, #352, #358, #359, #364, #365, #366, #367, #369, #370 and #371 of
  this repository.
- The competitor survey and the providers' terms, in polarzero's private research of the same day.

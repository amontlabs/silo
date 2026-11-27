# Fact check

This checks `first-run.md`, `daily-loop.md`, `cli.md` and `plan-map.md`. A script listed every
backticked id in the four files and looked each one up under `plan/` at `1b8f191`, reading its
answer and verdict. Each claim about an entry was then read against that entry's JSON. Numbers,
quotes and product claims were traced to `onboarding.md`, `complaints.md`, `notify-review-cli.md`
and `macos-first-run.md`, and to the prior research named in the brief (`2026-11-24-first-build.md`,
the first-build decisions README and file 10, record-ux, the incidents survey). PR #347 was read
on its branch. Four claims were checked on the web: Claude Code's Remote Control page, Docker's
approvals page, and the ipsw.me size of 27.2.1, which is 25,595,954,706 bytes. All four hold. The
mockups were checked for sums, times and counts. The text was checked against `guides/writing.md`
and `plan/words.json`. Every id the files name exists, except the ones `plan-map.md` marks as
placeholders for new entries. Every taken or open state given in `plan-map.md`'s table is right.

## Wrong

- **first-run.md, question 5:** "A MacBook with its lid closed still sleeps on battery". Its
  sources say the same in other places. daily-loop.md, gap 2, has "Silo cannot stop a lid-closed
  laptop on battery from sleeping", and cli.md, `silo doctor`, has "On battery, closing the lid
  puts your Mac to sleep". The source (`macos-first-run.md`, section 5) says that closing the lid
  sleeps the Mac on power too, unless an external display is attached, and that `caffeinate`
  cannot stop it. It should say: "With its lid closed, a MacBook sleeps unless it is on power with
  an external display attached. Main stops until it wakes."
- **daily-loop.md, loop table:** "A run keeps going when your terminal closes, and while your
  Mac's lid is closed". Gap 2 itself says Silo cannot make the second part true. Change it to
  "while your Mac is awake", and say that a closed lid is warned about.
- **first-run.md, question 2:** the default is `[~/work]` and the answer typed is `~/code`, so the
  folders copied in are `~/code/app` and `~/code/client-b`. daily-loop.md and cli.md then use
  `~/work/app` and `~/work` throughout. Pick one path. `~/work` matches record-ux.
- **cli.md, `silo` first build and `silo changes`:** "Changed outside ~/work" lists
  `deleted ~/work/shared-fixtures/`, which is inside `~/work`. In record-ux the rule watches
  "outside `~/work/app`". Either the folder the agents work in is `~/work/app`, or the deleted
  folder must sit elsewhere.
- **cli.md, `silo checkpoints`:** "A checkpoint you take by hand with a note is never thinned by
  age; no entry says so yet". `thinning-checkpoints` thins only "automatic checkpoints", so the
  plan already keeps one taken by hand. The edit that is still needed is for the checkpoint saved
  before a restore, which `restore-saves-first` says "is thinned like the others". plan-map.md's
  "Before bed" row and its list of edits to `thinning-checkpoints` repeat the error.
- **cli.md, `silo` first build:** "the incidents survey (19 of 41 incidents undone, 14 of them
  files destroyed on the agent's own machine)". `incidents.md` says 14 of the 41 incidents
  destroyed files on the machine the agent ran on, and 19 of the 38 in Silo's reach are undone.
  The 14 are not a subset of the 19. It should say: "14 of 41 incidents destroyed files on the
  agent's own machine".
- **cli.md, `silo checkpoints`:** "every 10 min since 04:58 (13)". From 05:00 to 06:50 there are
  12 checkpoints. "61 checkpoints" also does not fit `thinning-checkpoints` for a main made on
  Monday at 13:42. The folded lines show about 22, and the hourly ones from Monday 14:00 to 22:00
  add 9, which makes about 31.
- **cli.md, `silo restore`:** "4 h 18 of work". From 02:40 to the save at 07:02 is 4 h 22.
- **cli.md, `silo checkpoints`:** "Mon 13:42 the first, before anything was installed". In
  first-run.md that checkpoint comes after the Command Line Tools, Homebrew, Claude Code, Codex and
  tmux are set up. first-run.md calls it "before any agent ran", which is the right label.
- **first-run.md, `blank-computer` section:** "Nobody can tell why `git clone` hangs". The source
  says the stub fails over SSH with "no install could be requested (perhaps no UI is present)". It
  fails with a message and does not hang. Say it fails with a message that does not say what to do.
- **first-run.md, "What the first run has to beat":** "None of the tools read shows the size of
  the download". Tart's quick start says it "will download a 25 GB image" (`onboarding.md`,
  section 3). What none of them shows is the time.
- **first-run.md, question table:** "FB23241619, unverified fix". The sources say "no fix
  reported". "File 10's workaround makes a setup user first and renames" is also off: file 10
  creates `ana` with `sysadminctl` and then deletes `silo-setup`. Nothing is renamed.
- **cli.md, opening:** "The words are the plan's: computer, … fork, checkpoint, restore, drop, …
  pause, stop, … alert, ask, hold, … receipt". Of these, only main, bin, record, fact, report,
  opinion, rule, precaution and run are in `words.json`. The others are proposed in open decisions
  (`meanings-for-computers`, `meanings-for-the-boundary`, `meanings-for-what-silo-does`,
  `what-a-receipt-is`). Say "the plan's words, some still proposed in open decisions".

## Unsupported

- **daily-loop.md, gap 2:** "Apple's framework does not hold the Mac awake while a virtual machine
  runs (macos-first-run.md, section 5)". The source marks this **[unverified]**: "No source says
  so", and it is to be checked with `pmset -g assertions`. Write "no source says that it does", and
  name the Mac test.
- **first-run.md:** "Silo can be as fast as Tart, the fastest way to a macOS virtual machine
  today". The source says Lume's first create takes about 8 minutes. It also says that building
  from the restore image "costs about 5 to 10 extra minutes of install" against a prebuilt image.
  The sources support "Silo's download is no larger than Tart's". They do not support "as fast".
- **daily-loop.md, gap 3:** "One background Claude Code session waited on a prompt for about 8.5
  hours". In `complaints.md`, item 47, that session waited on a **permission prompt**, which every
  check off removes. The primary page could not be reached. The item does not show an agent waiting
  on a question to its user, which is what gap 3 is about. Keep #73125 as the evidence, and either
  drop the 8.5 hours or say what that prompt was.
- **daily-loop.md, gap 3:** "The user reports agree that the daily failure … is a run silently
  blocked". `complaints.md` says this in its patterns section, from four items (#47, #48, #50,
  #51). It is one researcher's reading. Say "four reports describe…".
- **daily-loop.md, lock screen:** "it is how 1Password and Duo guard approvals against push
  fatigue". In the source, Duo Verified Push makes you type a code, and only when it sees a risk.
  The 1Password flow is marked partly unverified. Face ID on Allow comes from Apple's
  `.authenticationRequired`. Cite that, and the source's A2 "step-up for risky answers".
- **daily-loop.md, gap 5:** "quiet hours are the setting people ask for first". No source ranks
  requests. The source lists Pushover, GitHub Mobile and iOS Focus as having quiet hours.
- **daily-loop.md, gap 1:** "Claude Code's own Remote Control page says … to 'use tmux over SSH'".
  The page says: "start it inside `tmux` or `screen` to keep the session running after you
  disconnect from SSH". Quote that.
- **daily-loop.md, gap 5:** Remote Control "pushes to the phone only 'when actions required'". The
  page has a second toggle, "Push when Claude decides". The skip while you are at the terminal is
  right: it "skips mobile push notifications while you are typing in or focused on the connected
  terminal".
- **first-run.md:** "Docker's own post of August 2026 says an empty sandbox loses to running on the
  host". The post's words are "Isolation only survives contact with developers when it is at least
  as convenient as skipping it", under the title "Empty sandboxes break developer experience".
  Quote it.
- **first-run.md, transcript:** "installs (about 6 min, measured by others)". This shows a time
  that nobody measured on this Mac. `first-run-time-macos` says Silo states no time in public until
  it is measured. A screen of the product is close to public. Show the measured download estimate
  only, and say "install: a few minutes, not yet measured on this Mac".
- **first-run.md and daily-loop.md, many places:** "Silo takes a checkpoint every 10 minutes".
  Neither the plan nor the first-build research gives an interval. First-build.md says "every few
  minutes", and the decisions README sizes retention at a 5-minute interval. Mark 10 minutes as
  this research's choice.
- **cli.md, command table:** the builds given for `silo alerts` (second), `silo receipt` (second)
  and `silo runs` (second, also in daily-loop.md) do not match `2026-11-24-first-build.md`. It puts
  opinions, views and receipts "after the record has a few kinds of lines to judge", and the app's
  places, Now among them, after the third build. Processes alone come in the second build.
  `alerts-on-a-change` needs `rule-actions-on-the-computer` and `precautions-you-set`. Only
  `stop-run` and `secret add` fit the second build.
- **cli.md, `silo` second and third builds:** "◇ codex stuck" and "◇ … mismatch" are classifier
  opinions. The same build note applies.

## Contradicts the plan

- **first-run.md, `claude setup-token`:** "a value Silo could hold as a secret
  (`secrets-held-outside`)". `logins` (open) says "Silo holds no login of a subscription, for any
  provider". A setup-token is a subscription's OAuth token. Drop it, or raise it as a challenge to
  `logins`.
- **first-run.md:** "Whether passwordless `sudo` for main's user is the default is the open product
  choice file 10 already names". `full-access-inside` (taken) says "Inside, agents have full access
  (`sudo`) and nothing asks them for approval". A sudo password prompt is an approval, so the taken
  decision already answers it: passwordless. Say so, and file 10's open point closes.
- **first-run.md, transcript and sign-in section:** the last screen says "codex login opens a
  sign-in link on your Mac (Silo forwards port 1455)". The text below says that this is
  `sign-in-from-a-computer`, "and that feature is not in the first build". That feature needs
  `held-requests-and-asks`, which is in the third build. The same screen also says main's entry
  forwards "no ports". The first-build screen should print the device-code way and the ChatGPT
  setting it needs.
- **first-run.md, `blank-computer` section:** "`claude-code-fetched-by-recipe` (Silo ships no copy:
  yes, it runs Anthropic's installer)". That entry also says the version is named and checked
  against a checksum, and `recipe-audit` waits three days before it takes a new version. Running
  `claude.ai/install.sh` unpinned meets "ships no copy" and skips both of the other two. Say so as
  a first-build exception and do not claim a fit.
- **first-run.md, failure 3:** "This is the known case where Apple refuses your user name …
  Run `silo setup --user ana-main`". File 10, step 5, catches `guestProvisioningInvalidUsername`
  before the start and falls back on its own to a setup user that keeps your name. A timeout on
  SSH is not that error, and Silo cannot know that it is. That makes the line an opinion shown as
  a fact. A new name also breaks `moving-in-on-macos`. The failure should report the timeout and
  the picture, and offer a retry. The "5 minutes" limit is also invented: file 10 sets it from
  test 2.
- **first-run.md and cli.md, "the first build has no network of Silo's own: main runs on Apple's
  NAT"**. This is stated as settled. `network-stack` (open) says "Never Apple's NAT". First-build.md
  offers NAT only as a fallback "if the test shows [gvproxy] is slow". Write "if the first build
  runs on Apple's NAT (Mac test 7)…".
- **daily-loop.md, gap 6, and cli.md, `put-back`:** "Paths put back from a checkpoint", into main.
  `fork-from-the-moment` (taken) says that on a shared main "you do not rewind … the past is
  read-only, you continue from it in a fork … the result leaves the fork through Git". Putting a
  path back in main is a partial rewind of main that does not go through Git. The proposal has a
  case: it is close to `putting-back-one-run` (open), and the plan already allows a whole restore
  of main. Even so, it should be raised as a challenge to `fork-from-the-moment` for the first
  build, not only as a new feature.
- **daily-loop.md, gap 1:** a second SSH entry, `main.keep`, per computer. `reaching-a-computer`
  (open) says "one named entry per computer". The gap names it as "touches". It should say it
  changes that answer.
- **daily-loop.md, gap 4, "Away":** the agent gets "held for the user, who is away until 07:00; not
  sent". The proposal says "a hold becomes a deny". In `meanings-for-the-boundary`, a deny is
  "refused, and what comes back says denied", and a hold "waits for your answer". The words sent
  back should say denied, for example "denied: the user is away until 07:00".
- **daily-loop.md and cli.md, "refused asks stay on the list", "2 asks refused while you were
  away":** an ask is "a request that waits for your answer". Once it is refused it is not an ask,
  and `hold-sentence` closes it. Call the line "requests refused as held, not answered".
- **plan-map.md, item 10:** "the plan-limit headers … could come before the rest of the traffic
  features (`plan-limit-reached`)". `plan-limit-reached` needs `model-traffic-read` and
  `alerts-on-a-change`, so it cannot come before them. "`budget-on-a-computer` needs only spend per
  computer" goes against its `needs` list, which holds `spend-per-conversation` and
  `recognising-a-conversation`, and against its name ("on a computer or a conversation"). Both are
  proposed edits to `needs`, and the item should say so.
- **daily-loop.md and cli.md:** these files treat `what-silo-does-not-build` as in force ("Silo
  must not wrap the agent program"). That entry has no verdict. Its answer also says "an answer to
  an ask or to an agent's question … [is] all you send it from Silo". So the plan does let you
  answer an agent's question from Silo. That is the natural action for gap 3's "waiting on you"
  line, and gap 3 does not offer it.

## Inconsistent

- **first-run.md:** "all asked in the first minute". At the end of the questions the bar reads
  "6.8 / 25.6 GB 36 MB/s". At 35 MB/s that point comes about 3 minutes in. The proposed sentence
  for `first-run-wait`, "all before the download ends", is also a different limit from "in the
  first minute" (plan-map.md uses both). Pick one.
- **first-run.md against cli.md:** at the first run, "Main gets 16 GB and 6 cores". `silo status`
  shows "8 of 12 cores · 18.2 of 24 GB".
- **cli.md:** "three agents were left working in main". Its later-build output, like record-ux, has
  codex in the fork `try-bun`. daily-loop.md also says "three agents in main". The `~/.zshrc`
  change in the first-build output is codex's change in `try-bun` in record-ux, and bun appears
  under `/opt/homebrew`, although bun.sh installs it with curl.
- **cli.md, the deletion window "02:46 → 02:50" and the kept "02:46" checkpoint:** with checkpoints
  only every 10 minutes, on the tens as the rest of the output shows, there is no 02:46 checkpoint.
  In record-ux it exists because of `checkpoint-at-each-turn` (open, not in the first build). In
  the first-build output use "02:40 → 02:50" and `silo look 02:40:…`. The `next:` line of the
  first-build `silo` already uses 02:40.
- **cli.md:** the first-build `silo` lists no `~/.gitconfig` change, but `silo changes` for the same
  night does. "macOS writes thousands of files by itself overnight … counted in the 7,412" does not
  match `silo changes` "macOS itself 812 files". Under `~/work/app`, "96 files" is more than the
  lines under it, which add up to 91 (12 + 2 + 41 + 36).
- **cli.md, later-build `silo`:** the new host `bun.sh` from record-ux sketch (a) is missing.
  daily-loop.md gap 5 puts "a host seen for the first time" in the morning list.
- **cli.md:** `next: silo open 1` names a command that is not in the command table.
- **daily-loop.md, "What the day looks like":** at 04:30 "main pauses … You leave it until morning".
  The same night has claude #1 working until 05:52 and running `npm test` at 05:31. Pause main
  later, or use a different alert.
- **daily-loop.md, phone against Mac:** on the phone, "Allow asks for Face ID" because an allow
  grants. Gap 7's Mac notification has Allow and Deny buttons with no step-up. Either give the Mac
  the same step-up (Touch ID or password on Allow), or make Allow open Silo.
- **daily-loop.md, gap 1:** `RemoteCommand tmux new-session -A -s keep` is one session. A second
  `ssh main.keep` mirrors the first terminal instead of opening a new one. With three agents, each
  needs its own window or session (`-s keep-$N`, or `new-window`). `main.keep` also depends on tmux
  being in main, and only question 4's optional set-up installs it.

## Writing

- **daily-loop.md:** "The user reports agree" and "the third pain of the user reports" (also in
  plan-map.md). In the plan, "report" means what a program inside a computer told Silo. Write
  "users' complaints" or "issues". plan-map.md also says "the third pain" and then cites
  `complaints.md` theme 2.
- **Lines written to be quoted:** daily-loop.md, "Idle hides the one idle that matters."; "pushed
  to a phone, it teaches you to ignore Silo."; "That one line explains a quiet night.". cli.md, "A
  restore that says … is a restore people read before they type `y`.". plan-map.md, heading 10,
  "Spend, the first pain users name, comes third". That is also unsupported: the complaints file
  lists spend third among its patterns.
- **first-run.md:** "A fork or a fresh macOS computer then starts from a copy of it". In the
  proposed words, a fresh computer is "made from a recipe", and the first build has none. Write
  "a new macOS computer".
- **Labels made up for the page:** "the morning column" and "interrupt and morning" (daily-loop.md,
  plan-map.md). Define them once, or write "alerts that wait for the morning list".
- "sandbox" appears only as the name of Docker's product, and "safe" only inside entry ids. Neither
  word is used for Silo.

## Applied

Every item above was applied to `first-run.md`, `daily-loop.md`, `cli.md`, `plan-map.md` and the
README, with the choices the brief gave where an item needed one. The paths are `~/work`, and the
morning filter is changes outside the repositories `~/work/app` and `~/work/client-b`. Checkpoint
times in the first-build output are on the tens, and the 10-minute interval is marked once per
file as this research's choice. Put-back is kept, and is raised as a challenge to
`fork-from-the-moment` in `daily-loop.md`, gap 6, and in `plan-map.md`. The screens were not
redrawn. `screens/README.md` has a note that the Mac banner shows no step-up, and that the
lock-screen ask is from a night when you are not away. Its table still says "the morning column"
for `phone-list.png`, which was left as it is.

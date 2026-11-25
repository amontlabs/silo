# Phone approvals, morning-after review, CLI conventions: research for Silo

Researched 2026-11-24. Each concrete claim carries a URL. **[unverified]** marks a claim I could not
confirm from a primary page (from memory, secondary coverage, or a page that did not render).
Silo terms used below: *hold* (a request stopped at the VM boundary until the user answers),
*checkpoint*, *budget*, *main* (the VM).

---

## A. Phone approvals and push notifications

### A1. What the agent tools ship today

**Claude Code Remote Control** (https://code.claude.com/docs/en/remote-control)
- Connection: "Your local Claude Code session makes outbound HTTPS requests only and never opens
  inbound ports on your machine. When you start Remote Control, it registers with the Anthropic API
  and polls for work." Traffic is TLS; "multiple short-lived credentials, each scoped to a single
  purpose and expiring independently."
- Push: two toggles only, "Push when Claude decides" (proactive) and "Push when actions required"
  (permission prompts and questions). "Beyond the two on/off toggles below, there is no per-event
  configuration."
- **Presence suppression**: "Claude Code skips mobile push notifications while you are typing in or
  focused on the connected terminal." A `CLAUDE_CLIENT_PRESENCE_FILE` marker file extends that: a
  screen-lock listener creates the file on unlock and deletes it on lock, and pushes are skipped while
  it exists. This is a cheap, honest "am I at the desk?" signal Silo can copy on macOS directly
  (Silo runs on the Mac, so it can read screen-lock state itself).
- **Timeouts and defaults**: "Claude Code keeps permission prompts and `AskUserQuestion` questions
  open until you answer them." Other forwarded dialogs wait five minutes by default, "then closes the
  dialog and continues with the dialog's no-action default", tunable with `dialogExpiry`.
  So: approvals never time out into "yes"; lesser dialogs time out into "no action".
- **Re-auth on the phone** (enterprise device trust): enrolment once per device; "When the sign-in
  ages past 18 hours, the next Remote Control interaction shows a single Face ID, Touch ID, Windows
  Hello, or passkey prompt." Unenrolled devices cannot view or steer sessions.
- Survives sleep: "if your laptop sleeps or your network drops, Claude Code reconnects automatically
  when your machine comes back online". Limit: "the local process must keep running".
  Heartbeat failure: disconnects after "could not reach the Remote Control server for about 30 minutes".

**Claude Code hooks** (https://code.claude.com/docs/en/hooks)
- `Notification` hook matchers include `permission_prompt`, `idle_prompt`, `elicitation_dialog`,
  `agent_needs_input`, `agent_completed`, `quota_auto_resume_fired` and more. Input carries
  `message`, optional `title`, `notification_type` (example: `"message": "Claude needs your
  permission"`). "Notification hooks can't block or modify notifications."
- `PermissionRequest` hook can decide: `decision.behavior` = `allow` | `deny`, with `updatedInput`
  (allow), `updatedPermissions` (allow), `message` and `interrupt` (deny). Exit code 2 is *not* a
  decision. With `"onFailure": "block"`, a failed hook denies.
- Implication: third parties (ntfy, Pushover, Happy) bolt phone approvals on by blocking inside
  this hook until a phone answers. Silo does not need the agent's cooperation: its holds sit at the
  VM boundary, so they work for any agent, including ones with no hook system. This is a real
  differentiator worth saying on the home page.

**Codex** (https://openai.com/index/work-with-codex-from-anywhere/)
- Mobile (preview, in the ChatGPT app) connects to a machine "where Codex is already running", via a
  relay that keeps machines reachable "without exposing them directly to the public internet"; files,
  credentials and permissions "stay on the machine where Codex is operating". You can "approve
  commands" and review "screenshots, terminal output, diffs, test results, and approvals".
- Codex CLI `notify` runs an external program on `agent-turn-complete`, user-level config only
  (https://developers.openai.com/codex/local-config, https://developers.openai.com/codex/config-reference).
- Push notifications for approvals are a community request, not a documented feature
  (https://community.openai.com/t/feature-idea-codex-mobile-control-inside-chatgpt/1380412). **[unverified whether shipped]**

**Happy** (https://github.com/slopus/happy)
- Wraps the harness: `happy claude` starts Claude Code. "End-to-end encrypted mobile app"; relay
  `happy-server` "for encrypted sync"; `happy-push-server`, a "Cloudflare Worker that delivers push
  notifications via APNs/FCM". Pairing by QR code (App Store listing,
  https://apps.apple.com/us/app/6748571505). Alerts "when Claude Code and Codex needs permission or
  encounters errors" (README text via search; **[unverified exact wording]**).

**Omnara** (https://github.com/omnara-ai/omnara)
- Old open-source version abandoned: "built as a wrapper around the Claude Code CLI, which became
  unfeasible to maintain with Claude Code's constant updates." Lesson for Silo: wrapping a fast-moving
  agent CLI is fragile; intercepting at a boundary you own (network, filesystem, VM) is not.

### A2. Approval patterns outside AI agents

**GitHub deployment reviews** (https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments,
https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/review-deployments)
- Up to six reviewers; one approval suffices. Optional comment. Reject = workflow fails.
- "Prevent self-review": "users who initiate a deployment cannot approve the deployment job".
- Wait timer 1 to 43,200 minutes (30 days).
- Unanswered: older GHES docs say "If a job is not approved within 30 days, it will automatically
  fail" (https://docs.github.com/en/enterprise-server@3.7/actions/managing-workflow-runs/reviewing-deployments);
  3.1 docs said "canceled". **[current github.com behaviour unverified]**. Either way: **no answer = no.**
- GitHub Mobile: push for "requested to approve a deployment for a protected environment", approve
  or reject from the app, pending ones in the Notifications tab, and a "Working Hours" schedule for
  pushes (https://github.blog/changelog/2021-04-01-reviewing-deployments-on-github-mobile,
  https://github.blog/changelog/2021-04-01-new-push-notification-types-schedules-on-github-mobile).

**Tailscale**
- Device approval: new devices are held; the device "can't send or receive traffic on the network
  until it's approved"; admins approve from a "Needs approval" badge; automation via a
  `nodeNeedsApproval` webhook plus API; pre-approved auth keys skip the hold
  (https://tailscale.com/kb/1099/device-approval). The CLI says it in one line:
  `Machine is not yet approved by tailnet admin.` (source:
  https://github.com/tailscale/tailscale/blob/main/cmd/tailscale/cli/status.go).
- Tailscale SSH *check mode*: a fresh identity-provider login before connecting, remembered for
  `checkPeriod` (default 12h; "always" re-auths every time) (https://tailscale.com/kb/1193/tailscale-ssh).
  Caveat seen in the wild: check mode broke an unattended weekly backup
  (https://forums.unraid.net/topic/191441-done-tailscale-ssh-change-check-mode-to-accept-mode/).
  This is exactly Silo's tension: an approval that is fine at noon blocks a 3am job.

**Push MFA and the fatigue attack**
- Uber 2022: attacker with a bought password sent repeated pushes, then posed as IT on WhatsApp
  saying the only way to stop them was to accept one; Uber reportedly did not use number matching
  (https://www.darkreading.com/cyberattacks-data-breaches/uber-breach-external-contractor-mfa-bombing-attack,
  https://www.infoq.com/news/2022/09/Uber-breach-mfa-fatigue).
- CISA: number matching is the mitigation for "MFA fatigue" when phishing-resistant MFA is not
  possible; investigate denied pushes; teach users to report bulk requests
  (https://www.cisa.gov/sites/default/files/publications/fact-sheet-implement-number-matching-in-mfa-applications-508c.pdf).
- Microsoft Authenticator: number matching on for all pushes; "users can't opt out"; on the *same
  device* that initiated the sign-in it falls back to Approve/Deny because "the prompt only shows on
  the device that initiated the sign in" (https://learn.microsoft.com/en-us/entra/identity/authentication/how-to-mfa-number-match).
  "Additional context" shows the requesting application name and IP-based location
  (https://learn.microsoft.com/en-us/entra/identity/authentication/how-to-mfa-additional-context).
- Duo Verified Push: type a code from the login screen; Duo steps up to it only when risk is
  detected, because adding friction to every login is hard to sell
  (https://duo.com/blog/verified-duo-push-makes-mfa-more-secure,
  https://duo.com/blog/duo-risk-based-authentication-now-in-ga).
- 1Password new-device sign-in: open and unlock the app on a trusted device, select Allow, get a
  verification code, type it on the new device (https://support.1password.com/sso-trusted-device
  and search excerpt; **[exact flow partly unverified]**).

**Transfer to Silo.** The MFA-bombing threat has a direct agent analogue: a prompt-injected agent
can generate hold after hold ("let me reach pastebin.com", "just this once") until a sleepy user
taps Allow. The defences carry over one to one:
1. **Context on the card**: what, where, why, which run (MS "additional context").
2. **Collapse and rate-limit**: one card per distinct request; repeats increment a counter, never a
   new buzz. A burst of holds is itself a signal ("12 holds in 4 min: run paused").
3. **Step-up for risky answers**: Face ID (`.authenticationRequired`, below) for allow; a typed or
   matched token only for the highest class (e.g. sending data to a new host, raising a budget).
4. **Denies are cheap and logged**: deny never needs auth; repeated denials are surfaced in the
   morning review (CISA: investigate denied pushes).

### A3. The notification plumbing

**Apple** (developer.apple.com JSON docs, fetched directly)
- Interruption levels: `active` is default and "won't break through system notification controls";
  `timeSensitive` "can break through system controls such as Notification Summary and Focus. The
  user can turn off the ability"; `critical` "bypasses the mute switch", "requires an approved
  entitlement", always shown even under Do Not Disturb
  (https://developer.apple.com/documentation/usernotifications/unnotificationinterruptionlevel).
  `passive` = silent, into Notification Center **[page text not returned; from memory]**.
  Critical alerts are for health/safety: Silo should not expect that entitlement.
- Action options: `.authenticationRequired` "can be performed only on an unlocked device ... the
  system prompts the user to unlock the device" (so Face ID gates Allow without opening the app);
  `.foreground` launches the app, "Do not use this option simply to bring your app to the
  foreground"; `.destructive` renders red **[from memory]**
  (https://developer.apple.com/documentation/usernotifications/unnotificationactionoptions).
- Grouping and summary: `threadIdentifier` "groups related notifications"; `relevanceScore` picks
  "the summary's featured notification"; `summaryArgument` adds context to the summary
  (https://developer.apple.com/documentation/usernotifications/unnotificationcontent).
- APNs: may store an undelivered push up to `apns-expiration` (≤30 days), but "APNs stores only one
  notification per bundle ID" while offline; `apns-collapse-id` merges repeats into one notification
  (https://developer.apple.com/documentation/usernotifications/sending-notification-requests-to-apns).
  So a phone that was offline overnight gets the *latest* push only: the app must fetch the queue of
  holds on open, never rely on pushes as the record.

**ntfy** (https://docs.ntfy.sh/publish/)
- Up to three actions per notification: `view` (open URL), `http` (fires a request, POST by
  default, headers and body allowed, `clear=true` dismisses only if the request succeeds),
  `broadcast` (Android only), `copy`. Action buttons are supported on Android, iOS and web per the
  docs' "Supported on" marker (https://github.com/binwiederhier/ntfy/blob/main/docs/publish.md).
- Priorities 1–5: 5 "long vibration bursts ... pop-over", 2 "no vibration or sound; hidden until the
  drawer is pulled down", 1 under "Other notifications".
- Scheduled delivery `At: tomorrow, 10am` (10 s to 3 days); same sequence ID replaces a scheduled
  message; cancel by `DELETE /<topic>/<sequence_id>`. Useful for "deliver digest at 8:00" and for
  withdrawing a push once the hold is answered elsewhere.
- Caveat for Silo: an `http` action that carries an approval must be authenticated; a bare topic URL
  is a shared secret. ntfy is a fine *bring-your-own* channel, not the secure answer path.

**Pushover** (https://pushover.net/api)
- Priority −2 (badge only), −1 (quiet), 0 (normal; during the user's quiet hours delivered as −1),
  1 (bypasses quiet hours), 2 emergency: repeats every `retry` ≥30 s until acknowledged, for at most
  `expire` ≤10,800 s (3 h), capped at 50 retries; returns a `receipt`; optional `callback` URL on
  acknowledgement; `cancel_by_tag`. "The first user in a group to acknowledge cancels retries for
  everyone." Acknowledging is not approving: ack only stops the nagging.

**Slack** (https://docs.slack.dev/interactivity/handling-user-interaction,
https://docs.slack.dev/reference/block-kit/composition-objects/confirmation-dialog-object)
- Must ack a button click with HTTP 200 "within 3 seconds"; `response_url` works 5 times in 30 min;
  `replace_original: true` rewrites the card in place (so the card shows "Allowed by X at 03:12").
- Confirm dialog: title ≤100, text ≤300, confirm/deny labels ≤30 chars, `style: danger` (red).

**PagerDuty** (https://docs.pagerduty.com/incident-management/incidents/dynamic-notifications)
- High urgency escalates if unacknowledged; low urgency "cannot escalate". Urgency can come from
  severity, and "can increase but not decrease". Support hours: separate urgency inside/outside
  hours. Practice: critical/error for immediately actionable, warning for "can wait", suppress info.

**Vercel** (https://vercel.com/docs/notifications)
- Web + email by default; push opt-in per device. "If you read a failure notification in the
  dashboard before Vercel sends the email, Vercel does not send that email." Some kinds cannot be
  fully muted ("critical"): you may drop one channel, not all. Misconfigured-domain notices "will be
  batched".

**Sentry** (https://docs.sentry.io/product/alerts/create-alerts/issue-alert-config/,
https://docs.sentry.io/product/alerts/notifications/)
- "Throttling (previously called the action interval)": an alert's actions don't run again for an
  issue until the window (5 min to 30 days) elapses. Weekly report every Saturday by email.

### A4. What makes a hold answerable in five seconds from a lock screen

Synthesis from the sources above; the specific layout is my proposal.

1. **Title = the decision, not the event.** "Allow `npm` to reach registry.npmjs.org?" not
   "Silo needs your attention". Claude Code's own example message ("Claude needs your permission")
   is the anti-pattern: it forces an unlock to learn what is asked.
2. **Body = three facts**: who (run name + agent), what exactly (host, path, amount), why the hold
   fired (rule name). MS's "additional context" is the precedent.
3. **Two or three buttons, safe one first**: `Deny` (no auth), `Allow once` (Face ID via
   `.authenticationRequired`), optionally `Open` (`.foreground`) for anything needing a diff.
   Never "Allow always" on the lock screen: widening a rule is a desk decision.
4. **Interruption level from consequence, not from Silo's eagerness**: run is *blocked* on the hold
   → `timeSensitive`; run continues on other work → `active`; FYI → `passive` / digest.
   PagerDuty's rule (urgency may rise, never fall) maps well: a hold that has blocked the run for an
   hour can be re-sent once at higher urgency; never the other way.
5. **One card per distinct question** (`apns-collapse-id`, `threadIdentifier` per run). Retract
   the push when answered anywhere (ntfy sequence-ID delete, Slack `replace_original`).
6. **Presence**: skip the phone while the user is at the Mac (Claude Code's presence file; Silo can
   read lock state natively).
7. **Defaults when nobody answers**: every source that gates something real resolves silence as
   *no* (GitHub: fail/cancel after 30 days; Claude Code: permissions stay open forever, lesser
   dialogs take the no-action default after 5 min). Silo should say per hold class what silence
   means, show it on the card ("If unanswered by 07:00: denied, run continues without it"), and
   keep the run busy elsewhere while waiting rather than idling the budget.
8. **Quiet hours**: Pushover demotes normal pushes during quiet hours and lets high priority
   through; GitHub Mobile has working hours; iOS Focus filters `active`. Silo needs only one rule:
   *nothing wakes the user unless the user said this class may*. Everything else waits for the
   morning review.

---

## B. Morning-after review: summarising unattended work

### B1. What exists

- **GitHub PR review**: mark a file as viewed, it collapses; "If the file changes after you view the
  file, it will be unmarked as viewed"; a progress bar counts viewed files
  (https://docs.github.com/en/enterprise-server@3.16/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/reviewing-proposed-changes-in-a-pull-request);
  "Changed since last view" badge (https://github.blog/news-insights/product-news/mark-files-as-viewed/).
  Lesson: review state is per file and invalidated by change; you only re-read what moved.
- **GitHub Actions job summaries**: Markdown written to `GITHUB_STEP_SUMMARY` appears on the run's
  summary page so you see test results "without someone opening the logs"; 1 MiB per step, 20 per job
  (https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands).
- **Graphite inbox**: an "email client" for PRs with default sections *Needs your review, Approved,
  Returned to you, Merging and recently merged, Drafts, Waiting for review*; customisable filters
  (https://graphite.dev/docs/use-pr-inbox, https://graphite.com/docs/pr-page-overview). Lesson:
  sort by *who must act*, not by time.
- **Linear Pulse**: a feed of project updates plus a personal AI summary delivered daily or weekly to
  the inbox, also as a short audio "Daily Pulse" (https://linear.app/changelog/2025-04-16-pulse,
  https://linear.app/changelog/2025-11-13-pulse-on-mobile).
- **Sentry**: weekly report, throttled alerts (above).
- **Devin Session Insights** (https://docs.devin.ai/product-guides/session-insights): metrics (ACU
  usage, user message count, session size; "Sessions classified as L or XL are flagged as
  unhealthy"), an issue list with impact rating, a colour-coded timeline (red high-impact, yellow
  medium, green "value provided"), actionable config suggestions, and an "Improved Prompt" that can
  start a new session. Lesson: the review is also about *what to change so next night goes better*.
- **Codex**: launch post promises "verifiable evidence of its actions through citations of terminal
  logs and test outputs" (https://openai.com/index/introducing-codex/, read via localized copies;
  **[English wording unverified]**). Cloud: "Inspect changed files and check results, request
  follow-up changes, and commit or open a pull request" (https://learn.chatgpt.com/docs/cloud).
  A 2026 paper reports agents turning killed processes into "confirmed" results after context
  compaction (https://www.alphaxiv.org/abs/2607.13071): the agent's own summary is not evidence.
- **Cursor cloud agents**: Slack notification on completion with a link to the PR; web/mobile at
  cursor.com/agents to review diffs and merge (https://cursor.com/docs/integrations/slack,
  https://docs.cursor.com/background-agent/web-and-mobile).
- **Jules**: plan shown for approval "before any code changes are made"; "You'll be notified when
  the task completes or needs your input" (https://jules.google/docs/).
- **Factory**: review posts inline comments plus a summary comment
  (https://github.com/Factory-AI/droid-action); hooks for desktop/Slack alerts on awaiting input
  (https://docs.factory.ai/).

### B2. What a good "while you were away" looks like (proposal for Silo)

Silo has something none of the above have: an **independent record** (checkpoints, the change log,
budget meter, hold log) kept outside the agent. The review should lead with that record and put the
agent's narrative second, labelled as the agent's claim.

```
While you were away  ·  23:10 → 07:42  ·  main

  Needs you (2)
    ⏸  hold  egress to api.stripe.com from run "billing-tests"   waiting 5h12m
    ✗  run "migrate-db" stopped: budget reached ($4.00 of $4.00)

  Done (3 runs)
    ✓  fix-flaky-auth     14 files  +212 −88   tests: 41 passed     $1.10
    ✓  docs-typos          3 files   +9  −9    no tests ran          $0.08
    ~  refactor-cache     31 files  +640 −402  tests: 2 failed       $2.71

  Silo saw
    7 checkpoints · 4 holds (1 allowed by you at 02:14, 2 denied by rule, 1 waiting)
    biggest change: refactor-cache rewrote src/cache/** (open: silo diff refactor-cache)

  Next
    silo hold show 1          answer the waiting hold
    silo review refactor-cache
```

Rules behind it, each from a source above:
- **Order by who must act** (Graphite), then by outcome; FYI last.
- **Facts Silo measured** (files, lines, tests actually run, dollars, holds) next to each run; "no
  tests ran" said out loud, because it is the claim summaries most often blur (Codex evidence,
  compaction paper).
- **Viewed state per run and per file**, cleared when it changes again (GitHub).
- **Every line ends in a command** (clig.dev "suggest commands").
- **One digest, sent once**, at a time the user picks; if they already looked on the Mac, don't send
  it to the phone (Vercel's read-suppresses-email).
- **Throttle repeats** into counts (Sentry throttling).
- **Retro, not just report**: what made the night worse (budget hit, repeated denied holds) with a
  suggested rule change (Devin "Actionable Feedback").

---

## C. CLI design references

### C1. clig.dev (https://clig.dev/), the rules that matter most for `silo`

- "Human-readable output is paramount"; detect a TTY; `--json` for machines, `--plain` for
  line tools; `-q` to "suppress all non-essential output". Primary output to stdout, messages and
  errors to stderr.
- Color "with intention"; disable when not a TTY, when `NO_COLOR` is set, `TERM=dumb`, or
  `--no-color`. No animations when stdout is not interactive.
- "Print something to the user in <100ms." Show progress for long work; print before a network call.
- "Catch errors and rewrite them for humans"; "Put the most important information at the end of
  the output."; group repeated errors.
- Exit 0 on success, non-zero otherwise; "Map the non-zero exit codes to the most important failure
  modes."
- "Suggest commands the user should run."
- Prompt only if stdin is a TTY; `--no-input` disables prompts; "Never _require_ a prompt."
- Severe actions: make the user type the name; offer `--force` / `--confirm=<name>` for scripts.
- "If you change state, tell the user"; "Make it easy to see the current state of the system" (like
  `git status`).
- Ctrl-C exits fast; say what a second Ctrl-C does; "Make it crash-only."

NO_COLOR itself: "when present and not an empty string (regardless of its value), prevents the
addition of ANSI color" (https://no-color.org/).

### C2. Real outputs and conventions

**git status** (run locally, git 2.x):
```
On branch master
Changes to be committed:
  (use "git restore --staged <file>..." to unstage)
	new file:   a

Changes not staged for commit:
  (use "git add <file>..." to update what will be committed)
  (use "git restore <file>..." to discard changes in working directory)
	modified:   a

Untracked files:
  (use "git add <file>..." to include in what will be committed)
	b
```
Every section carries its own next-step hint in parentheses. Short and machine forms are separate
flags with a stable contract: `git status -sb` →
```
## master
AM a
?? b
```
and `git status --porcelain=v2 --branch` →
```
# branch.oid d7d06c5d...
# branch.head master
1 AM N... 000000 100644 100644 0000000... 587be6b4... a
? b
```
Typos get a suggestion, not an action: `git: 'comit' is not a git command. See 'git --help'.` /
`The most similar command is commit`.

**gh** (run locally):
- `gh help exit-codes`: 0 success, 1 failure, 2 cancelled, 4 "requires authentication"; commands may
  add more, e.g. `gh pr checks` documents "8: Checks pending" — a distinct code for *not finished yet*,
  which is exactly what `silo wait` / `silo status` need for "a hold is waiting".
- `--json fields` + `--jq` + `--template`; `--json` with no fields lists available fields.
  "When connected to a terminal, the output is automatically pretty-printed."
- Env: `NO_COLOR`, `CLICOLOR=0`, `CLICOLOR_FORCE`, `GH_FORCE_TTY`, `GH_PAGER`, `GH_NO_UPDATE_NOTIFIER`
  ("checks for new versions once every 24 hours ... displayed on standard error").
- `gh pr checks --watch --fail-fast -i 10`: watch mode with a refresh interval and early exit.
- `gh auth status` reports per host with a ✓/X line and the reason ("The token in GH_TOKEN is
  invalid."): status explains *why*, not just *what*.

**tailscale** (source, https://github.com/tailscale/tailscale/blob/main/cmd/tailscale/cli/status.go and up.go;
docs https://tailscale.com/kb/1080/cli)
```
100.1.2.3   device-a  apenwarr@   linux     active; direct <ip-port>, tx 1116 rx 1124
100.7.8.9   device-c  danderson@  windows   idle; tx 1214 rx 50
100.0.1.2   device-d  ross@       iOS       -
```
- No header by default (`--header` adds `IP Hostname Owner OS Status`), tabwriter columns.
- Non-running states print one sentence and exit 1: `Tailscale is stopped.`, `Logged out.` +
  `Log in at: <url>`, `Machine is not yet approved by tailnet admin.`
- Health problems print as comments: `# Health check:` / `#     - <message>`; hints too:
  `# To see the full list of exit nodes ... run \`tailscale exit-node list\``. Comment-prefixed
  lines keep the table greppable while still talking to humans.
- `tailscale up` prints to stderr `To authenticate, visit:` + indented URL, then `Success.`
- `--json` is documented "(WARNING: format subject to change)": say so if Silo's JSON is not yet
  stable rather than break scripts silently.

**docker pull** (https://docs.docker.com/reference/cli/docker/image/pull/): one line per layer
(`e756f3fdd6a3: Pull complete`), then `Digest: sha256:...`, then
`Status: Downloaded newer image for debian:latest`; `--quiet` suppresses it. Per-item progress for
multi-part downloads, a final one-line verdict.

**limactl** (https://lima-vm.io/docs/usage/, https://lima-vm.io/docs/reference/limactl_list/)
- First run is an interactive picker: `? Creating an instance "default"` → `Proceed with the current
  configuration` / `Open an editor ...` / `Choose another template ...` / `Exit`; ends with
  `INFO[0029] READY. Run \`lima\` to open the shell.` — the last line is the next command.
- `limactl list --format json|yaml|table|'{{ go template }}'`, `--list-fields`, `--all-fields`;
  `--json` kept as legacy alias.

**tart** (https://tart.run/quick-start/): `tart clone` implicitly pulls when the image is absent;
`ssh admin@$(tart ip tahoe-base)` — a command that prints exactly one value, designed for `$( )`.
Silo's `ssh main` is nicer still, but a `silo ip`-style single-value command is the scripting hook.

**OrbStack** (https://docs.orbstack.dev/machines/): one short verb `orb`; `orb` alone enters the
default machine, `orb -m ubuntu2 -u root` picks one; built-in SSH server multiplexes all machines.
**[default-machine rule and `orb list` output unverified]**.

**Homebrew** (https://docs.brew.sh/Manpage): `HOMEBREW_NO_ENV_HINTS` "do not print any hints about
changing Homebrew's behaviour with environment variables" (hints are on by default and can be
silenced); `HOMEBREW_NO_COLOR` defaults to `$NO_COLOR`; `HOMEBREW_COLOR` forces color on non-TTY;
auto-update once every 24h by default; `brew doctor` "will exit with a non-zero status if any
potential problems are found"; `brew info --json=v2` — a versioned JSON schema.

**fly** (https://docs.fly.io/flyctl/status): `fly status --json`, `--watch` with `--rate 5`.

**railway** (https://docs.railway.com/reference/cli-api): global `--json`; CI uses `RAILWAY_TOKEN`
instead of interactive login.

**vercel** (https://vercel.com/docs/cli/global-options)
- `--non-interactive`: "When the CLI detects that it is running under an agent, non-interactive mode
  is the default." Worth copying: inside `main`, an agent may itself call `silo`.
- Exit codes: 0 success or help, 1 operational failure, 2 usage error; help for a nonexistent path
  exits 2; `vercel inspect --wait` exits 1 when the wait times out.
- `--no-color` / `NO_COLOR=1` remove "color or emoji output".
- Prefer env var over `--token` because args "can be visible in process lists and logs".

### C3. Proposed conventions for `silo` (synthesis)

- `silo status` reads like `git status`: state first, then one section per thing needing you, each
  with a parenthesised next command; one sentence + non-zero exit when the VM is not running
  (Tailscale). Health issues as `#` lines.
- Exit codes: 0 ok, 1 failure, 2 usage, plus distinct codes for "a hold is waiting" and "budget
  reached" so scripts and `silo wait` can branch (gh's `8: Checks pending`).
- `--json` on every read command with a documented, versioned shape (brew `--json=v2`); stdout data,
  stderr chatter.
- Long downloads (VM image): per-part progress lines with size and rate in a TTY, a single final line
  otherwise (docker, clig.dev). First line within 100 ms.
- First run ends with the next command (`READY. Run \`ssh main\``), like lima.
- Hints on by default, silenced by one env var (brew), never in `--json`.
- No prompt unless stdin is a TTY; detect an agent caller and default to non-interactive (vercel).
- Answering a hold from the CLI mirrors the phone: `silo hold allow 1` / `silo hold deny 1`, typed
  confirmation only for the severe class (clig.dev).

---

## D. Decisions worth challenging or settling in `plan/` (for the orchestrator)

These are product questions the research raises; I did not read `plan/`, so some may be decided.
1. **What silence means per hold class.** Every gate surveyed resolves silence as no. Silo should
   write that down, show it on the card, and keep the run useful while waiting.
2. **Hold fatigue is the MFA-bombing threat.** Collapse duplicates, rate-limit, pause a run that
   floods holds, step-up auth for allow, free deny.
3. **Push is a doorbell, not the record.** APNs keeps one push per app while offline; the phone must
   fetch the hold queue. The record lives with Silo.
4. **Wake rules are the user's, per class.** Default: nothing wakes the user; blocked-run holds are
   time-sensitive only if opted in.
5. **Evidence before narrative** in the morning review: Silo's measured record first, the agent's
   summary labelled as its claim.

# When Silo alerts you

## The choice

An agent's part, 22 November 2026. At 14:02 main's disk passes 90% and stays there all afternoon. In
the same minute an install in main reaches 12 hosts that main never reached before.

The options:

- On a change (recommended). You get one alert at 14:02 for the disk, and one alert for 12 new
  hosts. The disk stays in your list of what needs you until it drops back under 90%. It costs
  reminders: once you set the alert aside, nothing tells you again while the disk stays full.
- Each time. An alert each time a rule is triggered, and each minute the disk stays above 90%.
  It costs dozens of alerts an hour, and the habit of ignoring them.

Recommended: on a change. Alerts that repeat what you already know teach you to stop reading them,
and the list of what needs you already keeps what is still true.

## Notes

Marked Notes in the design record.

## From option A's first-run transcript

Agent, 25 November 2026, from `research/2026-11-25-first-run-option-a/` (gap 4 there). `new-host-raises-an-alert` (in force) gives an alert for each host seen for the first time. On a first night every host is new: #371 saw Codex reach four hosts at start, before any work. The first morning then opens on about twenty such alerts, and the paste site an agent sent a log to is one of them. Two things no entry says: hosts reached by the first run's own installers count as seen, and the first-seen hosts of one night are merged into one line with their count, each still an alert. This entry merges repeats of one alert; it does not say how alerts of one kind on many hosts are merged.

## Native builds through the proxy

Agent, 25 November 2026, from `research/2026-11-25-native-builds-through-the-proxy/`. The hosts each program reaches at start, before any work, measured through #378's proxy. Claude Code 2.1.332 with a key: `api.anthropic.com` only, and `downloads.claude.ai` when it updates itself. Codex 0.172.0 with an API key: `api.openai.com`, `chatgpt.com`, `ab.chatgpt.com`, `github.com` and `api.github.com`. Signed in with ChatGPT, Codex reached `chatgpt.com` only. These are the hosts the first run can count as seen, so that they raise no first-seen alert on the first night.

## One night's first-seen hosts

Agent, 25 November 2026, from `research/2026-11-25-first-morning-hosts/` (#382's gap 4). "Alerts of
one kind in one computer that come close together" does not say whether hosts reached hours apart
in one night are close together. On the measured first night they are about 57 hosts over the
night. Read as one night, they are one alert with its count, and the paste site comes first when
the hosts in it are ordered by the bytes sent to each. Read as minutes, they are dozens of alerts.
Recommended: while you are away, alerts of one kind merge until you next look. Ordering inside the
merged alert is in `new-host-alert-is-a-rule`'s notes.

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

## A program that refuses Silo's certificate (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-tls-refusals`. A refused certificate
is a condition that lasts, as this entry describes: it starts at a program's first refusal on a
host, and ends when that program makes a request to that host, or after 30 minutes with no
refusal. Codex refused 47 times in 5 minutes; under this entry it is one alert, "Codex on main does
not trust Silo's certificate for api.openai.com: nothing it sends there gets through", and one
line in the morning: "Codex on main did not trust Silo's certificate for api.openai.com: 47
refusals from 18:53 to 18:58, still refusing at the end. Give it Silo's authority (SSL_CERT_FILE
or the system's store)." The program is named from the User-Agent its TLS client sent on an
earlier connection that worked; a client never seen working is shown by its TLS fingerprint.

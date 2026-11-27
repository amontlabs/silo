# When a Google sign-in's traffic passes unread, what Silo counts of the plan

## The choice

An agent's part, 25 November 2026, from `research/2026-11-25-google-plan-count`. You run Gemini CLI
in main, signed in with your Google AI Pro account, which allows 1,500 model requests a day shared
by every device. Overnight two forks work on a task with it. In the morning you want to know how
many of the day's requests they used, and you would have liked them to stop at 500.

Two drafts answer differently. #368 proposes `google-sign-in-unread`: Silo passes the traffic of a
Google sign-in on unread, because Google suspends accounts whose sign-in third-party tools use. #407
found that Google counts a plan in requests, so a boundary that reads the traffic knows a computer's
share exactly and can hold it as a rule. Both cannot hold. Through an unread connection the boundary
sees no request, no model, no 429 and no remainder. Real Gemini CLI 0.71.1 made 37 requests in one
run, 33 of them model requests, over 2 connections. From outside, the boundary counted 39 changes of
direction, which mixes the TLS handshakes, the 4 other calls and the model requests, with no model
named.

Gemini CLI can say what it used. Pointed at a collector with its OpenTelemetry export, it sent one
`api_request` event per model request, with the model, the role (main, router, loop detector), the
tokens and the status. In two runs all 41 model requests were reported. At the daily limit it
reported the 429 with Google's whole error, `QUOTA_EXHAUSTED` and the retry delay included. It is
still the program's word. Killed hard it lost 1 to 3 requests, and `GEMINI_TELEMETRY_ENABLED=false`
in its environment, which an agent in the computer can set, turns it off.

The options:

- **Count what Gemini CLI reports** (recommended). Google's traffic stays unread, as #368 asks.
  Silo sets the export when it sets up Gemini CLI, with prompts left out, and the app says "312
  requests today on gemini-3.5-flash, as Gemini CLI reports them". A precaution budget in requests
  denies the computer's next connections to `cloudcode-pa.googleapis.com` when the report reaches
  it. `plan-limit-reached` can fire from the reported 429. It costs a listener for one standard
  format, which #407 already needs for Claude Code, and a count that an agent can silence: a computer
  whose report stops while its connections go on is shown as such. It gives no figure of the
  account's remainder, since the program does not report what Google tells it.
- **Read the traffic.** The count is exact, per model as Google counts it, and a budget can be a
  rule. Gemini CLI fetches the account's remainder itself every 30 s or more, so Silo would also show
  what the whole account has left. It costs the risk `google-sign-in-unread` describes: Google's
  terms forbid third-party tools to reach these services with the sign-in, and a second suspension
  is for good.
- **Count nothing,** as `program-that-cannot-be-read` answers today. It costs every Gemini CLI user
  on a Google plan the count, any budget and the limit alert.
- **Estimate from the connections.** It counts the handshakes and the other calls as requests, names
  no model, and cannot tell a request refused at the limit from one served. Not recommended.

Recommended: count what Gemini CLI reports. It keeps #368's protection, gives the count #407 found
to be the plan's own unit, and is the same mechanism as #407's `program-reports-its-own-spend`.

## What each option lets Silo say

| | Count what Gemini CLI reports | Read the traffic | Count nothing |
|---|---|---|---|
| The computer's requests today | "312 requests on gemini-3.5-flash, as Gemini CLI reports them" | "312 requests on gemini-3-flash" | "Gemini CLI on a Google sign-in is not read here" |
| The account's remainder | nothing | "1,188 left on the account, 30 s ago" | nothing |
| A budget of 500 requests | a precaution: the next connections are denied when the report reaches 500 | a rule: request 501 is refused | none |
| The daily limit | "Google's daily limit reached on gemini-3.5-flash, retry in 1 h", from the report | the same, read from the 429 | nothing |
| Risk to the Google account | none from Silo: the program sends the report, Silo calls nothing of Google's | the one #368 describes | none |

The model names differ. Gemini CLI reports its own name (`gemini-3.5-flash`) and sends Google
another (`gemini-3-flash`), from a mapping in its code. Google counts the second. Over a run the two
counts match per model once the mapping is known, so the app shows the program's name.

## What it changes in other drafts

- `google-sign-in-unread` (#368): its answer stands. It could add "and counts what Gemini CLI
  reports, as `google-plan-counted-unread` answers", and `needs` this entry when both merge.
- `program-reports-its-own-spend` (#407): its answer names dollars and Claude Code. For Google the
  unit of the plan is a request, so the report should count requests there. The two can become one
  entry when both are taken.
- #407's notes on `how-spend-is-shown` and `subscription-and-key-alike` say a computer's share of a
  Google plan is a fact Silo counts. That holds only where Silo reads the traffic, which
  `google-sign-in-unread` rules out for a Google sign-in. Under this entry it is the program's report.

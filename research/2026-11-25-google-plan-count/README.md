# Counting a Google plan without reading it

Research of 25 November 2026, by an agent, for polarzero. Two drafts disagree on Google. #368
proposes `google-sign-in-unread`: the traffic of Gemini CLI signed in with a Google account passes
the boundary unread, because Google suspends accounts whose sign-in third-party tools use. #407 says
a Google plan counts model requests per day, so the boundary knows a computer's share exactly and a
budget on it can be a rule. This measures what each lets Silo know, with real Gemini CLI 0.71.1,
and proposes how to reconcile them.

| File | Holds |
|---|---|
| [`prototype/fake_code_assist.py`](prototype/fake_code_assist.py) | A stand-in for Google's Code Assist API, with a daily quota per model counted in requests |
| [`prototype/relay.py`](prototype/relay.py) | A relay that passes the encrypted connection on unread and notes what it can see |
| [`prototype/otlp_collector.py`](prototype/otlp_collector.py) | #407's collector, taking the chunked and gzip bodies Gemini CLI sends |
| [`prototype/run_gemini.sh`](prototype/run_gemini.sh), [`compare.py`](prototype/compare.py) | One run of real Gemini CLI, and what was served against what was seen and reported |
| [`prototype/results/`](prototype/results/) | The output of every run reported here |

## What was found

- **The two drafts cannot both hold.** Through an unread connection the boundary sees no request,
  no model, no 429 and no remainder. Over 2 connections Gemini CLI made 37 requests, 33 of them
  model requests. The relay counted 39 changes of direction, which mixes the TLS handshakes, the
  calls that are not model requests (`loadCodeAssist`, `listExperiments`, `retrieveUserQuota`) and
  the model requests, and names no model. #407's "a share per computer is a rule" needs the traffic
  read, which #368 rules out.
- **Gemini CLI reports every model request it makes, if asked.** With its OpenTelemetry export
  (`GEMINI_TELEMETRY_OTLP_PROTOCOL=http`) pointed at a local collector, it sent one
  `gemini_cli.api_request` and one `api_response` per model request: 8 of 8 and 33 of 33, read or
  unread, including the router's call to a lighter model and the loop detector's. Each names the
  model, the role, the input, output, cached and reasoning tokens, and the status.
- **It reports the daily limit, whole.** At the limit its `api_error` event carried Google's 429 as
  served: `RESOURCE_EXHAUSTED`, `QUOTA_EXHAUSTED`, the `quotaId` and `RetryInfo` with the delay. So
  `plan-limit-reached` can fire on an unread Google computer, which #407 found impossible for an
  unread Claude one, whose report has no reset time.
- **It does not report the account's remainder.** No event carries what `retrieveUserQuota`
  returns. Only reading the traffic shows what the whole account has left.
- **It names models its own way.** It reported `gemini-3.5-flash` where it sent `gemini-3-flash` on
  the wire, from a mapping in its code (`LEGACY_CCPA_FLASH_MODEL`). Google counts the wire's name.
- **It is the program's word.** Killed hard it lost 1 of 4, 3 of 12 and 1 of 19 model requests.
  `GEMINI_TELEMETRY_ENABLED=false` in its environment silenced it: 0 of 8.
- **By default it sends the whole conversation.** `log_user_prompts_enabled` is on unless set
  otherwise: 18,041 characters of prompts and replies in a run of 8 requests. With
  `GEMINI_TELEMETRY_LOG_PROMPTS=false`, none. Silo should set it off: the point of leaving the
  traffic unread is not to take the conversation another way without deciding it.

| Run | Model requests served | Reported | Other requests | Unread: connections, changes of direction | Results |
|---|---:|---:|---:|---|---|
| 6 tool steps | 8 | 8 | 4 | | [1](prototype/results/1-whole-run.txt) |
| 30 tool steps | 33 | 33 | 4 | | [2](prototype/results/2-long-run.txt) |
| 30 steps, `SIGKILL` after 3 s | 4 | 3 | 4 | | [3](prototype/results/3-killed-after-3s.txt) |
| the same after 6 s | 12 | 9 | 4 | | [3](prototype/results/3-killed-after-6s.txt) |
| the same after 9 s | 19 | 18 | 4 | | [3](prototype/results/3-killed-after-9s.txt) |
| `GEMINI_TELEMETRY_ENABLED=false` | 8 | 0 | 4 | | [4](prototype/results/4-switched-off.txt) |
| daily quota of 4 per model | 6, 1 refused | 6, 1 as `api_error` 429 | 4 | | [5](prototype/results/5-daily-quota-reached.txt) |
| 6 steps over TLS, unread | 8 | 8 | 4 | 2, 14 | [6](prototype/results/6-unread-6-steps.txt) |
| 30 steps over TLS, unread | 33 | 33 | 4 | 2, 39 | [6](prototype/results/6-unread-30-steps.txt) |
| prompts left out | 8 | 8 | 4 | | [7](prototype/results/7-prompts-off.txt) |

"Served" is what the stand-in logged, which is also what a boundary reading the traffic counts.

## What it changes in the plan, by entry id

- **New open decision `google-plan-counted-unread`**, after `hook-is-a-report`: when a Google
  sign-in's traffic passes unread, what does Silo count of the plan? Recommended: the requests
  Gemini CLI reports, with prompts left out, driving a precaution budget in requests and the limit
  alert. Its notes set the options side by side with what each lets the app say.
- For entries that live only on other drafts (to be added when they merge):
  - `google-sign-in-unread` (#368): stands as written. When both merge it gains a line pointing
    here, and this entry `needs` it.
  - `program-reports-its-own-spend` (#407): the same mechanism. Its answer counts dollars, which is
    right for Claude; for Google the plan's unit is a request. The two can become one entry.
  - #407's notes on `how-spend-is-shown` and `subscription-and-key-alike`, and its README's line for
    `budgets-in-dollars-or-a-window`, say Google's share per computer is a fact and a rule. That needs the traffic read; with
    `google-sign-in-unread` it is the program's report and a precaution. With a Gemini API key or
    Vertex AI, which Silo reads, it stays a fact, but those have no plan's daily window.

No verdict, answer of a taken entry or list was changed.

## How to run it

On Linux with Python 3, openssl and Gemini CLI 0.71.1 (`npm i @google/gemini-cli@0.71.1`; set
`GEMINI` to its path). No network beyond loopback: the sign-in is a made-up access token and the
stand-in answers every call.

```
P=research/2026-11-25-google-plan-count/prototype
STEPS=30 $P/run_gemini.sh /tmp/g1 && python3 $P/compare.py /tmp/g1
STEPS=30 KILL_AFTER=6 $P/run_gemini.sh /tmp/g2 && python3 $P/compare.py /tmp/g2
DAILY=4 STEPS=10 $P/run_gemini.sh /tmp/g3 && python3 $P/compare.py /tmp/g3
TLS=1 RELAY=1 STEPS=30 $P/run_gemini.sh /tmp/g4 && python3 $P/compare.py /tmp/g4
$P/run_gemini.sh /tmp/g5 GEMINI_TELEMETRY_ENABLED=false && python3 $P/compare.py /tmp/g5
```

## Not verified

- Google itself. The stand-in's answers follow the bundle's parsing, and its 429 follows #407's
  reading. Whether Google counts a refused request, a router's call or the loop detector's against
  the day was not checked.
- Antigravity CLI, which replaced Gemini CLI for unpaid and Google One users (#368). Whether it
  exports the same events is not known.
- Whether Google counts by the wire's model name or by family. The quota page says "model requests";
  the bundle keeps one bucket per wire name and maps it back.
- Gemini CLI's own usage statistics to Google were turned off in every run. Whether they tell Google
  that the export points at Silo was not read.
- Longer runs, where `retrieveUserQuota` and other calls repeat and the unread count drifts further.

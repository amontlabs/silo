# The program's own spend report as a budget source

Research of 25 November 2026, by an agent, for polarzero. #407 found that Claude Code, with its
OpenTelemetry export pointed at a collector, reports every model call it makes with its tokens and
price, and matched the bill on 38 of 38 calls. It proposed the open decision
`program-reports-its-own-spend`: count that report, for traffic Silo passes on unread (#393's
computer set for a Claude plan), and let it drive a precaution budget. This folder takes the next
steps: it puts the report beside what the boundary still sees of that traffic, measures every way
an agent in the computer can silence or forge the report, and tries how the two are reconciled,
shown and used by a budget.

The prototype is in Python, beside #374's Zig boundary rather than inside it: the boundary's part
here is a byte relay that writes down sizes and times, and the reconciliation reads its record.

| File | Holds |
|---|---|
| [`prototype/passthrough.py`](prototype/passthrough.py) | The boundary for unread traffic: forwards TLS as it came, and writes each exchange's bytes up and down and times, as facts |
| [`prototype/listener.py`](prototype/listener.py) | Silo's OTLP listener (HTTP, JSON), with the computer each export came from and when it arrived |
| [`prototype/fake_anthropic_usage.py`](prototype/fake_anthropic_usage.py) | #407's stand-in for the Messages API, now over TLS, with kept-alive connections and scripted agent commands |
| [`prototype/run.sh`](prototype/run.sh), [`cases.sh`](prototype/cases.sh) | A real Claude Code 2.1.332 on a made-up plan token, one run per way of silencing the report |
| [`prototype/tamper.py`](prototype/tamper.py) | The report an agent could send in place of the honest one, made from a recorded run |
| [`prototype/reconcile.py`](prototype/reconcile.py) | Matches events to exchanges, checks each against the sizes, and gives the figures the app would show |
| [`prototype/budget.py`](prototype/budget.py) | A precaution budget replayed on a recorded run, counting the report alone or the checked count |
| [`prototype/results/`](prototype/results/) | Every figure below |

## What was found

- **Anyone in the computer can silence the report, and nothing inside the computer can stop it.**
  Of 20 ways tried, 10 silenced it from the start: the `CLAUDE_CODE_ENABLE_TELEMETRY` variable set
  to 0 or empty, either exporter set to `none`, the endpoint or the protocol changed, the same in
  the user's `settings.json`, the endpoint changed over Silo's managed settings, and a hard kill while exports were held back. Settings Silo writes
  to `/etc/claude-code/managed-settings.json` win over the variable and over the user's and the
  project's settings, but not over a changed endpoint, and an agent with `sudo` can rewrite that
  file anyway.
- **Some things that look like switches are not.** The project's `.claude/settings.json`,
  `OTEL_SDK_DISABLED`, `DISABLE_TELEMETRY`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` and a 1 ms
  shutdown timeout all left the report whole. A settings file rewritten by the agent in the middle
  of a run did not take effect until the next start. So Silo can turn off Anthropic's own
  telemetry and keep the report.
- **Anyone in the computer can also post events to the listener.** The agent's own tool call sent
  a made-up `api_request` with `curl`; the listener took it like any other. A budget on the report
  alone then stopped the computer early, on spend that never happened.
- **The boundary's sizes catch every forgery tried, except silence, which it counts instead.** A
  request carries its whole prompt, so its bytes up bound the input tokens it can hold. A cache
  read is a prefix sent before, so it cannot be longer than the computer's longest earlier
  request. With those two checks, every event of a report with token counts divided by 10, with
  inputs set to zero, or with cache writes relabelled as reads (40 times cheaper, same size) was
  flagged, 31 of 31 in each, and none of the 148 honest events was flagged.
- **The exchanges with no event can be estimated from the conversation's growth.** Each request
  repeats the one before and adds to it. Counting the added bytes as a cache write and the rest as
  a cache read, at the bytes per token this computer's honest events showed, put the 16 calls
  after a report went silent at $0.1932, where the stand-in billed $0.1964. Pricing them by the
  average dollars per byte instead gave $0.4082, twice the truth.
- **A budget on the checked count held in every case; on the report alone, in none of the
  tampered ones.** Under a $0.30 precaution on the 31-call run, the report alone stopped the honest
  run at $0.33 and never stopped a silenced or forged one ($0.49). The checked count stopped the
  honest run and the half-silenced one at $0.3008, and the three forged ones at $0.3506.
- **Claude Code's report hides the conversation by default.** Prompts and replies arrive as
  `<REDACTED>`; tool results carry their sizes, not their contents. Silo needs none of the
  `OTEL_LOG_*` switches that would send them.

## The report against the sizes, run by run

`prototype/cases.sh` runs Claude Code 2.1.332 signed in with a made-up `sk-ant-oat01-…` token,
its Anthropic traffic through `passthrough.py` over TLS to the stand-in, and its export at
`listener.py`, with Silo's settings in the environment: `CLAUDE_CODE_ENABLE_TELEMETRY=1`, both
exporters `otlp`, `http/json`, the listener's address. Each case then adds what an agent could
change. Nothing reached a real provider. The `tampered-*` rows are the `long-visible` run with its
report rewritten by `tamper.py`. From [`results/table.md`](prototype/results/table.md):

| Run | What the agent did | Calls billed | Events | The report as reconciled | Billed | Reported |
|---|---|---:|---:|---|---:|---:|
| baseline | nothing | 7 | 7 | whole | 0.1738 | 0.1738 |
| long | nothing, 30 tool steps | 31 | 31 | whole | 0.4851 | 0.4851 |
| env-off | `CLAUDE_CODE_ENABLE_TELEMETRY=0` | 7 | 0 | silent | 0.1738 | 0 |
| env-unset | the same variable empty | 7 | 0 | silent | 0.1738 | 0 |
| logs-none | `OTEL_LOGS_EXPORTER=none` (metrics still sent) | 7 | 0 | silent | 0.1738 | 0 |
| both-none | both exporters `none` | 7 | 0 | silent | 0.1738 | 0 |
| endpoint-away | `OTEL_EXPORTER_OTLP_ENDPOINT` elsewhere | 7 | 0 | silent | 0.1738 | 0 |
| protocol-grpc | `OTEL_EXPORTER_OTLP_PROTOCOL=grpc` | 7 | 0 | silent | 0.1738 | 0 |
| protocol-proto | `http/protobuf` (3 exports arrived, unreadable as JSON) | 7 | 0 | silent | 0.1738 | 0 |
| user-off | the variable at 0 in the user's `settings.json` | 7 | 0 | silent | 0.1738 | 0 |
| project-off | the same in the project's `.claude/settings.json` | 7 | 7 | whole | 0.1739 | 0.1739 |
| sdk-disabled | `OTEL_SDK_DISABLED=true` | 7 | 7 | whole | 0.1738 | 0.1738 |
| disable-telemetry | `DISABLE_TELEMETRY=1` | 7 | 7 | whole | 0.1767 | 0.1767 |
| nonessential | `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` | 7 | 7 | whole | 0.1766 | 0.1766 |
| late-interval | exports held for an hour, normal exit | 7 | 7 | whole, all at exit | 0.1738 | 0.1738 |
| short-shutdown | the same, and a 1 ms shutdown timeout | 7 | 7 | whole | 0.1738 | 0.1738 |
| late-killed | exports held for an hour, killed after 9 s | 18 | 0 | silent | 0.3269 | 0 |
| managed-on-env-off | Silo's settings in managed settings, the variable at 0 | 7 | 7 | whole | 0.1738 | 0.1738 |
| managed-on-project-off | managed settings, the project's settings at 0 | 7 | 7 | whole | 0.1739 | 0.1739 |
| managed-on-endpoint-away | managed settings, the endpoint changed in the environment | 7 | 0 | silent | 0.1738 | 0 |
| midrun-user-off | the agent writes the user's `settings.json` at step 2 | 9 | 9 | whole | 0.1885 | 0.1885 |
| midrun-project-off | the same in the project's settings | 9 | 9 | whole | 0.1885 | 0.1885 |
| forged-event | the agent posts a made-up event with `curl` at step 1 | 5 | 6 | contradicted: 1 event with no exchange | 0.1545 | 0.4345 |
| tampered-deflate | every token count divided by 10 | 31 | 31 | contradicted: 31 flagged | 0.4851 | 0.0484 |
| tampered-output-only | inputs set to zero | 31 | 31 | contradicted: 31 flagged | 0.4851 | 0.1860 |
| tampered-relabel | cache writes reported as reads | 31 | 31 | contradicted: 31 flagged | 0.4851 | 0.2894 |
| tampered-silence-half | the report stops after 15 calls | 31 | 15 | silent since exchange 16 of 31 | 0.4851 | 0.2888 |

The midrun cases show only that a change does not reach a running Claude Code. An agent that
writes the setting and then starts a new Claude Code, or a subagent process of its own, silences
that one, as `user-off` shows.

## How the two counts are reconciled

For a computer whose model traffic passes unread, the boundary still writes down each connection
to the provider: when bytes went up, how many, when bytes came back, how many. In HTTP/1.1 kept
alive, a burst up followed by a burst down is one model call. `reconcile.py` does four things.

1. **Matches each event to one exchange.** The event's start (its time less its `duration_ms`)
   lies within 1 s of the exchange's first byte up, and the event arrived after the exchange
   ended. In the honest runs every event matched, and every exchange of 2,000 bytes up or more
   was a model call.
2. **Checks each match against the sizes.** Three checks, each a contradiction between what the
   program said and what the boundary saw:
   - `input-below-bytes`: the request's bytes up are more than 8 per prompt token reported
     (honest runs: 4.12 to 4.13);
   - `output-below-bytes`: the reply's bytes down are more than 40 per output token reported
     (honest runs: 6.2 to 15.2);
   - `cache-read-never-sent`: the cache read reported is longer than the computer's longest
     request in the hour before, converted to tokens at this exchange's own bytes per token, so
     the check needs no tokenizer.
3. **Counts what does not match.** An exchange with no event is spend the report has not told
   (yet). An event with no exchange is spend the boundary never saw.
4. **Estimates the exchanges with no event, or with a flagged one,** from the conversation: bytes
   beyond the previous request are a cache write, the rest a cache read, the bytes down are
   output, each converted at this computer's median bytes per token over its honest events.

The report's state follows: **whole**, **partial** (some exchanges not told yet), **silent since**
a given exchange (three or more in a row with no event), or **contradicted** (an event flagged or
with no exchange).

| Estimate for the exchanges with no event | Half-silenced run (billed $0.1964) | Silenced from the start (billed $0.1738) |
|---|---:|---:|
| The conversation's growth, at this computer's own bytes per token | $0.1932 | $0.1668, at the defaults (4 bytes up, 8 down per token) |
| The average dollars per byte up of the told calls | $0.4082 | none: nothing told |
| A floor: 8 bytes per token, all at the cache read price | $0.0312 | $0.0094 |

The floor is not shown in the app: it is a twentieth of the truth here, and with an image in the
prompt (many bytes, few tokens) it is not even a floor.

## How it is shown, and what a budget counts

What the app would show for main on a Claude plan, overnight, in the three states:

- Report whole: "$4.12 today, as Claude Code reports it. Its 212 calls match the connections Silo
  saw."
- Report silent: "$2.80 as Claude Code reported it until 02:14. Since then, 96 connections to
  Anthropic with no report; Silo estimates them at $1.90 from their sizes."
- Report contradicted: "Claude Code reported $0.48. The connections Silo saw were larger than
  that: Silo estimates $4.34." The events that disagree are listed, each with the sizes.

The dollar figure a precaution budget counts is the checked count: the report for the events the
sizes agree with, and the estimate for every other exchange. From
[`results/budget-0.30.txt`](prototype/results/budget-0.30.txt), a $0.30 precaution on the 31-call
run, denying the next exchange once the count reaches it:

| Run | Counting the report alone | Counting the checked count |
|---|---|---|
| long, honest | denied call 19, billed $0.3269 | denied call 17, billed $0.3008 |
| long-visible, honest | denied call 25, billed $0.3987 | denied call 17, billed $0.3008 |
| half silenced | never denied, billed $0.4851 | denied call 17, billed $0.3008 |
| token counts divided by 10 | never denied, $0.4851 | denied call 21, billed $0.3506 |
| inputs set to zero | never denied, $0.4851 | denied call 21, billed $0.3506 |
| cache writes relabelled | never denied, $0.4851 | denied call 21, billed $0.3506 |
| a made-up event added | denied call 4, billed $0.1386 | never denied, $0.1545 |

The report alone stops late even when honest: the stand-in answers in tens of milliseconds and
the report arrives every 500 ms, so several calls leave before it is counted. The checked count
estimates those calls until their events arrive. The three forged reports stop later than the
honest one because the estimate uses the defaults there (no honest event to learn from), which
undercount here by about 11%.

## What it changes in the plan, by entry id

- **`program-reports-its-own-spend`** (#407, open): the answer now says the precaution counts the
  checked count, not the report alone, and that the app shows the report's state. Notes added with
  the measurements, and a correction: the report is a `report` in the words' sense, not an
  `opinion`. The case for it is stronger than #407 said for honest programs, and the report alone
  is weaker: a budget on it is undone by one variable, and a forged event can stop a computer.
- **`how-spend-is-shown`** (open): notes on the three states and their sentences.
- **`claude-key-or-plan`** (#393, not on this branch): "A budget cannot be set on a computer whose
  Claude traffic passes unread" would become "only a precaution budget, on the checked count". To
  be added there when #393 and #407 merge.
- **`facts-before-reports`** (a principle in force) is followed: the report is used only for what
  cannot be seen from outside, and each of its events is checked against what can.

No verdict, answer of a taken entry or list was changed.

## How to run it

On Linux with Python 3, `openssl`, Claude Code 2.1.332 on the path, and root for the managed
settings cases:

```
prototype/cases.sh /tmp/runs                     # every case, about 4 minutes
for m in silence-half deflate relabel output-only; do
  python3 prototype/tamper.py /tmp/runs/long-visible /tmp/runs/tampered-$m $m; done
python3 prototype/table.py /tmp/runs
python3 prototype/budget.py /tmp/runs/long 0.30
```

## Not verified

- Every figure comes from a stand-in provider whose tokenizer is a JSON length divided by 4. The
  4.12 bytes per prompt token of the honest runs is that tokenizer, not Anthropic's. The input
  check's limit of 8, and the defaults of the estimate on a computer with no honest event, need
  real requests. Images and PDFs in a prompt carry many bytes per token and would trip
  `input-below-bytes` on an honest event.
- `cache-read-never-sent` assumes the cache holds only what this computer sent. Anthropic's cache
  is shared by the organisation, so a second computer of the same account with the same system
  prompt can read a prefix this computer never sent, and the check would flag an honest event.
  Not measured.
- One connection per call in sequence over HTTP/1.1 is what the stand-in got. Whether Claude Code
  multiplexes calls over HTTP/2 to the real `api.anthropic.com`, which would need another way to
  cut exchanges, was not checked. Parallel subagents were not run.
- The forgeries were made from a recorded run (`tamper.py`), except the posted event, which the
  agent sent live. A forger who knows the checks could relabel less than 0.5% of a prompt, or
  report the right totals with a cheaper model's name; the model check was not built.
- Codex's and Gemini CLI's own telemetry, and Claude Code's native build on a Mac.
- `OTEL_EXPORTER_OTLP_HEADERS` could carry a token Silo gives the program, but an agent in the
  same computer can read the program's environment, so it was not tried as a defence.

# May Silo count the spend a program reports about itself

## The choice

An agent's part, 25 November 2026, from `research/2026-11-25-plan-windows`. You run Claude Code on
your Max plan in main, and main is set for Claude with a plan, so Silo passes its Anthropic traffic
on unread (#393's `claude-key-or-plan`). Overnight it works through a long task. In the morning Silo
can show its connections, their sizes and timing, and nothing of what it spent: no dollars, no
share of the plan's window, and no budget could have stopped it.

Claude Code can say what it spent. Pointed at a collector with its OpenTelemetry export, a real
Claude Code signed in with a plan token sent one event per model call with the model, the input,
output, cache read and cache write tokens and a price. In two runs all 38 calls matched what the
provider served, token for token. It is still the program's word: killed hard it lost 1 or 2 calls
(7% to 20% of output tokens), and one variable in its environment, which an agent in the computer
can set, turns it off.

The options:

- Count it, as the program's report (recommended). Silo sets the export when it sets up Claude Code
  on a computer set for a plan, listens for it, and shows the spend marked "as Claude Code reports
  it". A precaution budget in dollars can deny the computer's next Anthropic connections when the
  report reaches it, and says it rests on the report. It costs a listener for one standard format,
  which Codex can also export, and a figure that an agent can silence: a computer whose report stops
  while its connections go on is shown as such.
- Count nothing, as `program-that-cannot-be-read` answers. It costs every plan user of Claude Code
  the spend and any budget while Anthropic's answer is no, which is when they need it most.
- Estimate from the connections' sizes and timing. #374 found that visible characters miss about
  80% of a call's spend, because reasoning streams as nothing, and through an unread connection
  even those are hidden.

Recommended: count it as the program's report. It is exact when the program is honest, says so
when it is not, and is the only figure there is for a computer whose traffic passes unread.

`program-that-cannot-be-read` weighed reading a program's own files and recommended against: a
reader per program, kept up with each version, and files an agent can rewrite. This differs on the
first point. OpenTelemetry is one format, the program sends it, and Silo reads no file. It shares
the second: it is a report, so it can only drive precautions.

## What it does not give

No event carries a figure of the plan's window, and none could: the provider tells Claude Code the
account's figure, never a computer's. So on such a computer there is no reserve on the window
(#376's `budgets-in-dollars-or-a-window`) and no share of it. With a real sign-in each event also
carries the account and organization ids (read in Claude Code 2.1.332, not run), which would tell
Silo which computers share one plan.

## The report checked against the connections (25 November 2026)

An agent's part, from `research/2026-11-25-spend-report`. Claude Code 2.1.332 on a made-up plan
token, its traffic passed on unread over TLS to a stand-in provider, its report sent to Silo.

What an agent in the computer can do to the report:

- Silence it. Of 20 ways tried, 10 did: `CLAUDE_CODE_ENABLE_TELEMETRY` at 0 or empty, an exporter
  set to `none`, another endpoint or protocol, the same in the user's settings, and a hard kill
  while exports were held back. Settings Silo writes as managed settings win over the variable and
  the user's and project's settings, but not over a changed endpoint, and the agent has `sudo`.
- Forge it. The agent posted a made-up call to Silo's listener with `curl` from its own tool call,
  and the listener took it. On the report alone, a budget then stops the computer on spend that
  never happened.

What the boundary still sees settles most of that. A request carries its whole prompt, so its size
bounds the tokens it holds, and a cache read cannot be longer than an earlier request. Those checks
flagged every call of three forged reports (counts divided by 10, inputs at zero, cache writes
reported as reads: 31 of 31 each) and none of 148 honest calls. Calls with no report are estimated
from the conversation's growth: after the report went silent halfway, 16 calls billed at $0.1964
were estimated at $0.1932.

So this answer now counts the checked figure, not the report alone. Under a $0.30 precaution, the
report alone never stopped a silenced or forged run ($0.49 spent), while the checked figure stopped
the honest and half-silenced runs at $0.30 and the forged ones at $0.35.

Not verified: every byte-per-token figure rests on the stand-in's tokenizer; an image in a prompt
would trip the size check on an honest call; Anthropic's cache is shared by the organisation, so a
second computer could honestly read a prefix this one never sent; HTTP/2 was not tried.

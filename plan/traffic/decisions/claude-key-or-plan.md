# Claude with an API key or with a plan, per computer

## The choice

An agent's part, 25 November 2026. You use Claude Code on your Max plan in main. A friend you give
Silo to runs Claude Code with an API key in a fresh computer. Anthropic has said that a proxy may
not read a plan's traffic, or has not answered yet. Both programs talk to api.anthropic.com, and the
boundary decides whether to read a connection when it opens, before the request that would show
which sign-in it carries (`subscription-and-key-alike`). So something has to say, before the
connection, which computers are read.

`anthropic-reads-intermediate` answers this with "computers whose API key it holds as a secret".
Option A of #373 leaves `secrets-held-outside` out of the first build, so in that build Silo holds
no key, and that answer has nothing to go by. This entry is what replaces it.

The options:

- **A setting per computer: Claude with an API key, or with a plan** (recommended). A computer set
  for a key is read, and a plan's sign-in sent from it is refused with a sentence that says how to
  fix it. A computer set for a plan passes Anthropic traffic on as bytes. It costs one question per
  computer, asked once for main and inherited by forks, and, in a computer set for a plan, the
  turns, tool calls, spend and plan's share of Claude Code. Built and measured below: about 60
  lines in the boundary.
- **Hold the API key outside the computer, as the current answer says.** Silo reads Anthropic
  traffic only from computers whose key it holds. It needs no setting, and it brings
  `secrets-held-outside` into the first build, with `open-limited-or-closed` and the rule engine:
  the 18 decisions and 3 features #373 left out.
- **Read no Anthropic traffic until Anthropic says yes.** Nothing to set. It costs spend, budgets
  and the morning review's check of claims for every Claude user, API keys included, which the
  terms allow to be read.
- **Guess from the sign-in.** The boundary sees, by name and unread, the connections a sign-in
  makes. A plan's sign-in and a Console sign-in that creates an API key go through the same pages,
  and a `claude setup-token` made on another machine leaves no trace in the computer. Not
  recommended: a wrong guess reads a plan.

Recommended: the setting. It is the only option that keeps API keys read in the first build without
the secret store, and it states a fact the user knows, how they signed in, not a policy.

## What each answer from Anthropic changes

- **Yes, a proxy may read a plan's traffic.** Every computer is read. The setting then only refuses
  a plan in a computer set for a key, which nobody needs: drop it, and keep the boundary code for a
  later no.
- **No.** This entry applies as written. The app says, in a computer set for a plan, that Claude
  Code's conversations are not read there, and that an API key gives them.
- **No answer yet.** `anthropic-reads-intermediate` recommends reading polarzero's own traffic
  meanwhile, so polarzero's computers set for a plan are read. Whether computers of anyone else set
  for a plan are read before an answer is a separate choice for polarzero; this setting serves
  either way.
- **Yes, if Silo names itself in each request.** Computers set for a plan are read, with one header
  added, which `forwarded-byte-for-byte` forbids today, so that is a new decision. This setting does
  not change.
- **Other providers.** Codex with a ChatGPT sign-in talks to chatgpt.com and with a key to
  api.openai.com, and a Google sign-in to other hosts than a Gemini key. Whatever their answers,
  the boundary can tell them apart by host, with no setting.

## Built and measured (25 November 2026)

An agent's part, from `research/2026-11-25-key-or-plan/`. The setting was added to #374's boundary
and five real Claude Codes (2.1.332) ran a task at once against #374's stand-in provider, one per
computer. Computers set for a plan trusted only the provider's own authority, not Silo's, so
Claude Code could work there only if Silo did not end the connection.

| Computer set for | Claude Code signed in with | Result |
|---|---|---|
| an API key | an API key | read: 12 calls recorded with spend |
| an API key | a plan | refused at once, in 0.6 s; Claude Code printed Silo's sentence and exited 1 |
| a plan | a plan | worked; 14 connections recorded by host and size, no call read |
| a plan | an API key | worked; passed unread too: the setting, not the request, decides |
| no setting (today) | a plan | read: 12 calls recorded |

No token or key reached the record. The stand-in served 48 calls; Silo read 24.

Three things the trial showed:

- **Anthropic sees Claude Code's own connection in a computer set for a plan.** The boundary passes
  the program's own TLS handshake on, so the fingerprint is Claude Code's, not Silo's
  (`provider-sees-silos-connection`).
- **A key set wrongly is not caught.** In a computer set for a plan, an API key's spend passes
  uncounted, and Silo cannot know. The app should say, on a computer set for a plan, that a key
  there is not counted.
- **Claude Code's own calls with the plan's sign-in are refused too** in a computer set for a key:
  `GET /api/claude_code/settings` and `/api/claude_code/policy_limits`, before the first model call.
  Nothing broke because of it, since the first model call stops the program anyway.

Not checked: real Anthropic; the path through #378's proxy, where the host comes from `CONNECT`
before any encryption, which is simpler than reading it from the handshake; changing the setting
on a running computer; Claude Code builds other than 2.1.332.

## The program's own report (25 November 2026)

An agent's part, from `research/2026-11-25-plan-windows/`, added on merge because #407 wrote it before #393 had landed. "A budget cannot be set on a computer whose Claude traffic passes unread" holds for budgets read from traffic. `program-reports-its-own-spend` would allow a precaution budget there. The app's sentence for such a computer should also say that the plan's window is not shown.

## More research on this (27 November 2026)

An agent's part, 27 November 2026. These reports bear on this entry, and its notes did not point to them. The answer is unchanged.

- `research/2026-11-25-red-team-round-two/` (#435), finding 5, read in the code and not run: the prototype's reader of the sign-in splits header lines on CRLF only, and Anthropic's host also ends a header at a bare LF. A plan's sign-in can then pass through a computer set for a key. `research/2026-11-25-untrusted-parsers/`'s strict reader closes it.

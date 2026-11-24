# What does Silo read of an agent program whose vendor's server calls the model

## The choice

An agent's part, 24 November 2026. You run Cursor's agent in main. It sends the whole conversation
to `api2.cursor.sh`, in Cursor's own protocol, and Cursor's server calls the model. Main has a
budget of 50 dollars.

The options:

- Read a vendor only in a format Silo has (recommended). GitHub Copilot's server speaks the
  providers' formats, so Silo reads it once a live check shows it. Cursor, Windsurf, Amp and Kiro
  speak their own protocols, so their model traffic is allowed or denied by the rules and never
  read. The app says that their conversations, spend and tool calls are not covered, and under a
  budget their traffic is denied. It costs these programs most of what Silo does with model
  traffic.
- Write a reader for each vendor's protocol. Silo decodes Cursor's protobuf and the others. It
  costs a reader per program, where `adapters-follow-providers` chose readers per format. Cursor
  and Windsurf do not document their protocols, so each of their updates can break the reader
  without warning, and the spend Silo shows would be a guess at the vendor's own prices.
- Do not offer these programs. It costs the people who use them, who still install them the
  vendor's way (`unmodified-agent-programs`) and get the same result as the first option.

Recommended: read a vendor only in a format Silo has. A reader of an undocumented protocol fails
silently after an update, and a budget that counts wrong is worse than one that says it does not
count.

## Notes

### What each vendor's program sends (24 November 2026)

An agent's part, 24 November 2026, from the research in
`research/2026-11-24-every-agent-program`. Each program ran on Linux against a stand-in for its
vendor's host, with a fake key, so none got past its own sign-in.

| Program | Host it sends the conversation to | What it sends |
|---|---|---|
| Cursor CLI 2026.11.22 | `api2.cursor.sh` | Cursor's own RPC (Connect with protobuf). Its agent streams both ways over HTTP/2, unless the setting `network.useHttp1ForAgent` is on or Cursor's server switches it off |
| Amp 0.0.1795536036 | `ampcode.com` | Amp's own API. Amp's announcement "No more BYOK" says your own API keys are no longer used for its model calls |
| Copilot CLI 1.0.115 | `api.github.com` for a token, then `api.githubcopilot.com` | The model calls were not reached. Third parties document that the Copilot API answers in OpenAI's and Anthropic's formats |
| Kiro CLI | AWS (`q.us-east-1.amazonaws.com` and others) | Not run. Kiro's documentation names only proxy variables, no setting for a company certificate |
| Windsurf | Codeium's servers | Not run: an editor with no command-line agent. Its documentation treats a company's inspecting proxy as a trust problem for IT to solve |

All three that ran trusted Silo's certificate from the system's store, and worked over HTTP/1.1.
So Silo can always see their hosts, sizes and timing, and can deny their traffic. What it cannot do
without a reader is count their spend or see their tool calls.

These vendors also run tools on their own servers: a web search in the vendor's server never
crosses the boundary at all, whatever Silo reads.

### Copilot's own route and Kiro, further (24 November 2026)

An agent's part, 24 November 2026, from the research in `research/2026-11-24-agent-programs-follow-ups`.

- **Copilot CLI 1.0.115 with a GitHub token** asks `api.github.com/copilot_internal/user` for
  its plan and hosts (`api.individual.githubcopilot.com` for an individual plan), then managed
  settings, then the model list at `GET /meta` on that host. A stand-in for `/meta` was not
  accepted, so its model calls were not reached. It notices a refusal that does not come from
  GitHub: a 404 without GitHub's headers is logged as "does not appear to originate from GitHub. Is
  a proxy or firewall intercepting this request?"
- **Copilot with its own key needs no GitHub sign-in.** With `COPILOT_PROVIDER_TYPE`,
  `COPILOT_PROVIDER_BASE_URL` and `COPILOT_PROVIDER_API_KEY`, it is an ordinary client of
  Anthropic, OpenAI (Chat Completions, Responses, or Responses over a WebSocket), Azure or Google.
  It ran the whole scenario that way, denial included. So Copilot can be read today in that mode.
- **Kiro CLI 3.6.0** was installed from its release archive into a folder of its own. With a fake
  `KIRO_API_KEY` it calls AWS-style JSON services on `management.<region>.kiro.dev` and
  `runtime.<region>.kiro.dev`, with the key as a bearer token. Its own code names the model call
  `KiroRuntimeService.GenerateAssistantResponse`, answered as an AWS event stream, the framing of
  Bedrock. It trusted Silo's certificate from the system's store. It stopped at its model list, so
  no model call was seen.
- **Kiro asks for its own spend.** Each start calls `GetUsageLimits`, whose answer is Kiro's
  account of the credits used. Silo could read that answer as it passes, without reading the
  conversation, and show what the vendor says was spent.

# Can the boundary read the model traffic of each agent program

To find out. A first test on 19 November 2026 says yes for Claude Code and Codex.

What has to be checked is narrow: that each agent program accepts a certificate from Silo. Companies inspect traffic this way, and agent programs offer a setting for a company certificate, so this is expected to work. It is a test to run once per program.

## For each agent program and provider

- Does the program accept Silo's certificate, or does it refuse the connection?
- Which transport does it use (plain requests, a stream, a persistent socket), and can each be read?
- Does the sign-in flow of a subscription still work?
- Does reading leave requests unchanged byte for byte?

## What a first test found (19 November 2026)

Claude Code 2.1.327 under a Claude subscription and Codex 0.170.0 under a ChatGPT sign-in were each run once on a Mac, behind an ordinary inspecting proxy that stood in for the boundary.

- Both accept the certificate, and both refuse to connect without it. Neither falls back to another route, so there is no unread way through.
- Claude Code sends plain requests and receives a stream. Codex holds a persistent socket for the model and sends plain requests for the rest. All of it was read: the model that answered, the token counts, the tool calls.
- Both providers answered normally to a connection the proxy had opened. One short run cannot show what happens over days or under heavy use.
- Neither program hands out its encryption keys. So reading without ending the connection is not possible, and the boundary ends it, as planned.

## Still to find out

Signing in from scratch and the renewal of a token through the boundary; trust through the computer's trust store alone, with no setting in the program; that Silo's own boundary forwards byte for byte; other agent programs.

## What the vendors' terms say (read on 19 November 2026)

- Anthropic documents the set-up: "Enterprise TLS-inspection proxies work without additional configuration when their root certificate is installed in the OS trust store". Its terms do not prevent "an end user from signing in to the unmodified Claude Code binary with their own Claude subscription, including where a platform hosts Claude Code".
- OpenAI's terms, read in full, have no clause on reading one's own traffic. Its documentation gives Codex a setting for "a corporate TLS proxy or private root CA". The nearest clauses forbid sharing an account's credentials, extracting output automatically, and trying to extract the model's protected reasoning.

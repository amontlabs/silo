# Does a subscription sign-in change how Silo reads traffic

polarzero said this, on 18 November 2026.

- What is specific to a subscription: `provider-sees-silos-connection`.

## How Claude Code sends each kind (24 November 2026)

An agent's part, 24 November 2026, from `research/2026-11-24-spend-budgets`. Claude Code 2.1.331 sends an API key in
`x-api-key`. With `CLAUDE_CODE_OAUTH_TOKEN` it sends `Authorization: Bearer sk-ant-oat01-…` and the
beta `oauth-2025-04-20`, to the same host and path. So the kind of credential is known from the
request's headers before it leaves, which a budget on metered calls only needs. This was against a
stand-in, not Anthropic, and for this one version.

## The per-computer choice, specified (25 November 2026)

An agent's part. The last sentence of the answer, "the choice is made per computer and not per
request", is specified as `claude-key-or-plan` and measured in `research/2026-11-25-key-or-plan/`.
In a computer set for a plan, the boundary passes the connection on from the program's own TLS
handshake, so even the host is all it knows. In a computer set for an API key, the kind of sign-in
is read from the request's headers, as the note above found, and a plan's is refused.

## Windows are counted in different units (25 November 2026)

An agent's part, from `research/2026-11-25-plan-windows`. Anthropic and OpenAI give a plan's
windows as a share of a hidden weight, for the whole account. Google counts a plan in model
requests per day, shared by Gemini CLI and Code Assist on every device, so the boundary knows a
computer's share of a Google plan exactly by counting its requests. Codex names each request's
conversation in a `thread-id` header.

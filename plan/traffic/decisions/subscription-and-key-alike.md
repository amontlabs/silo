# Does a subscription sign-in change how Silo reads traffic

polarzero said this, on 18 November 2026.

- What is specific to a subscription: `provider-sees-silos-connection`.

## How Claude Code sends each kind (24 November 2026)

An agent's part, 24 November 2026, from `research/2026-11-24-spend-budgets`. Claude Code 2.1.331 sends an API key in
`x-api-key`. With `CLAUDE_CODE_OAUTH_TOKEN` it sends `Authorization: Bearer sk-ant-oat01-…` and the
beta `oauth-2025-04-20`, to the same host and path. So the kind of credential is known from the
request's headers before it leaves, which a budget on metered calls only needs. This was against a
stand-in, not Anthropic, and for this one version.

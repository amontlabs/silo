# Does a subscription sign-in change how Silo reads traffic

polarzero said this, on 18 November 2026.

- What is specific to a subscription: `provider-sees-silos-connection`.

## Windows are counted in different units (25 November 2026)

An agent's part, from `research/2026-11-25-plan-windows`. Anthropic and OpenAI give a plan's
windows as a share of a hidden weight, for the whole account. Google counts a plan in model
requests per day, shared by Gemini CLI and Code Assist on every device, so the boundary knows a
computer's share of a Google plan exactly by counting its requests. Codex names each request's
conversation in a `thread-id` header.

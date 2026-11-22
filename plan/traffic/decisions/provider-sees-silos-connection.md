# Does a provider treat a subscription's traffic differently when Silo's network stack makes the connection

## The choice

An agent's part, 22 November 2026. You use Claude Code on your Max plan in main all day. Anthropic
sees each connection made by Silo and not by Claude Code itself, as it would behind a company's
inspecting proxy. One short test on 19 November got normal answers from Anthropic and from OpenAI.

The options:

- Your own plan, watched (recommended). Silo records every answer from a provider that is not a
  normal reply: a challenge, a refused sign-in, an unusual error or limit. These are compared with
  the same programs used without Silo. At the first difference, or a warning from a provider, you
  decide before anything more is built on reading a subscription. It costs the risk on the plan you
  work with: a provider that dislikes the connection could limit it or suspend it.
- A second plan first. A separate subscription carries the risk while the comparison runs, and your
  own plan stays out of it. It costs a second subscription paid while it runs. It also shows less,
  since a plan used only for tests never sees your real, heavy use.

Recommended: your own plan, watched. Anthropic documents company inspecting proxies as working with
Claude Code, and OpenAI gives Codex a setting for one, so this is a setup both already expect.

## Notes

To find out. One point is specific to a subscription. The provider then sees a connection made by Silo's network stack and not by the agent program, as it does behind any company's inspecting proxy.

In the first test of 19 November 2026 both providers answered normally to a connection the proxy had opened (`reading-each-agent-program`).

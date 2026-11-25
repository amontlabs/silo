# Which credential carries a model call that Silo or an extension makes itself

## The choice

An agent's part, 24 November 2026. You install an extension with a classifier that asks Claude
whether each `git push` looks safe. You use Claude Code on your Max plan, and Silo sees that plan's
sign-in pass through the boundary with each request.

The options:

- An API key you give for it (recommended). The classifier asks Claude with a key you hold as a
  secret in Silo, billed to you at API prices. Without one it gives no opinion and says why. It
  costs an API key, and its bill, for anyone who wants classifiers that ask a model.
- The plan's sign-in. The classifier reuses the sign-in that passes through the boundary, at no
  extra cost to you. It costs your account: every provider read forbids it. Anthropic does not let
  developers "route requests through Free, Pro, Max, or Ultra plan credentials on behalf of their
  users", and every case Anthropic and Google enforced in 2026 was a tool using a plan's token
  from its own code. Silo would also have to keep the token to use it, which `logins` rules out.

Recommended: an API key. It is the one way every provider documents for a tool of its own.

## Notes

Read on 24 November 2026 (an agent's reading, not a lawyer's):

- Anthropic, Claude Code legal page: developers "may not collect, store, or intermediate Claude.ai
  credentials or session tokens"; Agent SDK products "should use API key authentication through
  Claude Console or a supported cloud provider"; plan limits "assume ordinary, individual usage of
  Claude Code and the Agent SDK".
- OpenAI, Sign in with ChatGPT Terms (4 November 2026): "Use the user's plan only for the
  application they connected."
- Google: using Gemini CLI's or Antigravity's sign-in from third-party software is a breach of
  their terms (`google-sign-in-unread`).

This also covers an extension's computer that runs an agent program in the background: it signs
in with its own key, not with your plan.

## Linked billing, a way with no key in Silo (25 November 2026)

An agent's part. Anthropic's support article 18467148 (beta, updated 24 November 2026) describes
linked billing: a user links their own Console organization to an app and pays Anthropic directly,
and never gives the app a key. For Silo's own model calls it is a second way beside a key you
paste. It costs Silo its own Anthropic organization, since the app runs the model in its own
organization and not yours; your own organization's settings, zero data retention included, do not
apply to those calls; it is a beta with a $50 limit while the app is in testing; and plans cannot be
linked. Not for the first build.

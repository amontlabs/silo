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

An agent's part, 25 November 2026, from `research/2026-11-25-reader-model`. The answer says a
classifier with no key "does not ask" a model. A model that runs in the classifier's own computer
needs no key and sends nothing to a provider, and the summary reader caught as many false claims
with a 4B model there as with a small hosted one (55 of 57), with more false alarms. Suggested
wording for the last sentence: "A classifier, a summary or an extension that asks a provider's
model and has no such key does not ask it, and says so; a model that runs in a computer of Silo's
needs no key." `summary-reader-model` asks which of these the summary reader uses.

## One provider offers a way on a plan (27 November 2026)

An agent's part, from pages another agent read on 27 November 2026. OpenAI's "Sign in with
ChatGPT" lets an open-source program that runs on the user's own machine make its own requests on
the user's ChatGPT plan ([developer documentation](https://developers.openai.com/siwc/llms-full.txt)).
Nothing like it was found at Anthropic or Google. Anthropic's Agent SDK page says third-party
developers may not offer a claude.ai login or its limits in their products "unless previously
approved", so an extension that drives `claude -p` on your plan for its own work is ruled out.

The answer above could gain one sentence: "For OpenAI's models, an extension may also use Sign in
with ChatGPT." It is not added here. It needs a ruling, and a check that Silo's extensions meet
that programme's terms.

Two more limits for an extension that asks a model about the record, such as
`said-compared-with-done`. It reads only what the provider sends in plain text, and never tries
to open sealed reasoning (`hidden-reasoning-stays-closed`). And the model it asks is an existing
one: training a model on recorded replies needs the provider's agreement under both Anthropic's
and OpenAI's terms.

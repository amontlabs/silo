## The choice

An agent's part, 22 November 2026. You install Silo and start Claude Code in main. Its first call to
its model provider is refused, because no rule allows that host. This entry said that Silo works
with no setting touched, and a promise in force says that no rule means deny.

The options:

- Silo proposes, you say yes (recommended). The first run shows the few rules your agent program
  needs, such as its model provider, and switches them on with one yes. It costs you that click, and
  Silo can no longer say it works with no setting touched. It still says no account, no sign-up and
  no key.
- Those rules ship switched on. Silo works with nothing touched. It costs the closed fresh install,
  since something is allowed out before you said anything.
- Nothing until you write rules. A fresh install stays fully closed. It costs you writing the first
  rules yourself before any agent works.

Recommended: Silo proposes, you say yes. One yes keeps a fresh install closed until you say so, and
it costs you a single click.

## Found by an audit, 22 November 2026

An agent's part. This says Silo works with no setting touched, but no rule means deny
(`deny-beats-hold-beats-allow`, in force). On the first run the agent program in main calls its
model provider, no rule allows the host, and no agent works. The audit proposes: "Silo needs no
account, no sign-up and no key. The first run proposes the rules main needs to reach the provider of
the agent program you chose, and they are switched on with your yes."
`proposed-rule-changes-nothing` already covers the yes.

# Rules a member cannot switch off

## The choice

An agent's part, 24 November 2026. A company of 30 must show its auditor that no agent can push to
the `main` branch of its payment service. A rule set each developer may switch off does not show
that.

How it is done today: Claude Code and Codex both take enforced settings from the operating system's
device management. Claude Code reads managed settings from a configuration profile, a file the user
cannot edit, or Anthropic's servers, and "nothing in your own settings files or `--settings`
overrides a managed key". Codex reads `requirements.toml` the same ways, and users "can't override"
it. Anthropic's own page warns that its server-delivered settings are "a client-side control, not a
security boundary": a personal account or an API key from another organisation does not receive
them. The profile installed by device management is the part that holds.

The options:

- Through the Mac's management only (recommended). The company's device management installs a
  configuration profile that names the rule set. Silo marks the set as managed and the member cannot
  switch it off. Silo builds no management of its own. It costs a company without device
  management the ability to enforce, and a team of five rarely has device management.
- Through a team key in Silo. A lead signs a set, and every Silo that trusts the lead's key applies
  it without a member's yes. It costs a second management system beside the Mac's, and a member who
  owns their Mac can remove the key anyway.
- Never. Every set is the member's choice. It costs the companies that must show an auditor a
  control that holds.

Recommended: through the Mac's management only. It is where companies already enforce settings on
a Mac, and on a Mac the member owns, nothing Silo builds would hold either.

What the member sees of a managed set is `managed-and-shown`.

# Does a subscription sign-in work with substitution at all

## For the owner

An agent's part, 22 November 2026. You use Claude Code with your Claude Max subscription. Each fresh
computer and each fork asks you to sign in again, since the login lives on that computer's disk.

The options:

- Not tried (recommended). You sign in the vendor's own way in each computer that needs it. It costs
  a sign-in per fresh computer or fork, and the login sits on that computer's disk, where its agents
  can read it.
- Tried, for providers whose terms allow it. Silo holds your login outside the computer and puts it
  into each request, as it does a secret, so the computer never holds it. It costs going against
  `program-and-sign-in-untouched`, which says Silo holds no subscription login. Anthropic's terms,
  as read on 18 November 2026, forbid it for a Claude subscription. It is also expected to break
  when the login is renewed, since the program inside writes the new one itself.

Recommended: not tried. Anthropic forbids it for the subscription Claude Code runs on, and a login
that breaks at its first renewal is worse than one sign-in per computer.

## Notes

Substitution: Silo holds the login outside the computer and swaps it in like a secret. To find
out. Anthropic's terms, as read on 18 November 2026, are in the notes of `logins`.

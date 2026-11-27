# Can a program send a credential of its own to a host that gets a secret

## The choice

An agent's part, 25 November 2026, from #355 (page 2), #351 and #353, as #364 (section 6) reconciles them.

The case: main has a GitHub secret limited to `polarzero/silo`. A worm in main reads a token of
its own from a `.env` file and pushes what it stole to `attacker/loot`, on the same host. Silo swaps
nothing into that request, since it carries no placeholder, so a limit on the secret does not stop
it.

The options:

- Not refused by Silo itself (recommended). The record notes the request on a host Silo reads, and
  a rule can refuse it on a host where Silo holds a secret for this computer, or on any host it
  allows. `gh` signed in inside the computer with your own login keeps working. It leaves the
  worm's case open until you set such a rule or switch on a set of rules such as
  `uploads-to-your-places`.
- Refused by default on a host where Silo holds a secret for this computer, except the agent
  program's own sign-in to its provider (#355's scope). It closes the worm's case with no rule. It
  costs a program you signed in to inside the computer on such a host, such as `gh` with your own
  login, which then needs its credential given as a secret.
- Refused by default on every host Silo reads (#351). It also refuses Claude Code's own
  subscription login, which `logins` leaves in the computer, and every command-line tool signed in
  inside, which #353 keeps working on purpose.

Recommended: not refused by Silo itself. Silo then acts the same way everywhere: by itself it
records, and a request is refused only by a rule you set or switched on. `secret-placeholder` and
`signing-keys-re-signed` answer the same way for a placeholder sent to another host.

## Notes

A credential is recognised by where it sits (`Authorization`, `Cookie`, a token in the query), not
by its format. #358's proposal, a hash of the subscription login kept so that the login leaving to
any other host is a fact, belongs with `logins`.

## A ChatGPT sign-in renews its token (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-codex-read-in-full/`. The answer lets
the agent program's own sign-in pass. #385's prototype of a related check (a request on a model
host whose credential this computer was not given is refused) compared the token's value, and a
ChatGPT access token changes each time Codex renews it. Once it had, every request of Codex was
refused and it could not start. A sign-in is known by its account, not by one value of its token:
the account id in the token stayed the same and passed, and another account's token was refused.

## Settled in the consolidation (27 November 2026)

An agent's part, 27 November 2026. The drafts had this refused by default, and this entry was
named `foreign-credential-refused`. polarzero asked the agent to choose where drafts disagree, and
to pick the choice that is predictable. The answer is now the first option above, so that the three
entries about a credential sent where it was not meant to go say one thing. The two other options
stay above for polarzero to weigh.

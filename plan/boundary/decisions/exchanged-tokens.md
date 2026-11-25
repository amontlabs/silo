# What does a computer get when a secret is exchanged for a token

## The choice

An agent's part, 24 November 2026. A script in main holds a placeholder for a Google refresh token.
It sends it to `oauth2.googleapis.com/token`, and Silo swaps the real one in. Google answers with
an access token that works for an hour. That answer comes back into the computer, so the agent can
read the access token and use it from anywhere for that hour. The swap protected the secret, and
the token made from it got out.

The options:

- Placeholders for the tokens too, at the endpoints Silo knows (recommended). Silo reads the
  answer of a known token endpoint, keeps the real tokens on your Mac, and gives the computer
  placeholders of the same shape. It refreshes them itself. It costs a description of each
  endpoint: where it is, what its fields are called, and, for a token a program decodes such as
  Azure's, a placeholder that decodes to the same claims. An endpoint Silo does not know passes the
  token through, and the app shows that this secret is weaker.
- Exchange once on your Mac, keep the token as a secret. You, or Silo, get the access token outside,
  and the computer only ever sees its placeholder. It costs every program that wants to run its own
  refresh, and a token that expires during a long task unless Silo renews it.
- Let the token in. It is short-lived. It costs the hour, or whatever its life is, in which the
  agent can use it from anywhere.

Recommended: placeholders at known endpoints. It is the same reading of traffic Silo does for the
swap, done on the answer, and Docker publishes it as a requirement for its own sandboxes.

## Notes

Agents' research of 24 November 2026, in `research/2026-11-24-secrets-not-sent-as-is/` of this
repository.

- Anthropic's Managed Agents documentation states the problem: "Substitution is outbound only…
  the returned token arrives in the sandbox unredacted". It advises making the exchange outside.
- Docker's kit specification for its sandboxes says a runtime "MUST intercept the OAuth
  `tokenEndpoint` and serve sentinel tokens, refreshing host-side".
- The first endpoints to describe: Anthropic's and OpenAI's sign-in, Google's, Microsoft's and
  GitHub's.
- This changes a token's way back, not the request. `what-silo-changes-in-traffic` allows one
  change, the swap; this would be a second, made on a reply. It has to be named there if it is
  taken.

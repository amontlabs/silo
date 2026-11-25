# How does a computer use a secret it cannot read

## The choice

An agent's part, 25 November 2026, from the challenges of #347 and #355, as #364 reconciles them.

The case: an agent in main runs `env` to debug a build. Every placeholder in the computer is now in
the conversation, and every later request to the model provider carries them all in its body. Under
the earlier answer, "a request that carries the placeholder to any other host is denied", so the
conversation ends for good, and nothing is protected, since a placeholder is worth nothing.

The options:

- Swap only in the secret's places, forward a placeholder seen anywhere else and record it
  (recommended). #355 built it and ran git, `gh`, boto3, Claude Code and the AWS SDK for JavaScript
  with only placeholders in the computer: 61 of 61 checks passed. A place is a named header, a
  scheme inside `Authorization`, the password of Basic authentication, a query parameter, a cookie
  or a path segment. It costs a list of places per secret, which the app proposes from the secret's
  format (#355).
- Swap anywhere in a request. It costs real keys written into the model's context, and an agent can
  publish a token through an issue body (#347).
- Keep the deny sentence, and find out whether a secret can also be bound to a method and a path
  (#356). It keeps the `env` case above, and #353 and #355 already answer the path question:
  a secret is limited by operation and by place in `secret-limits`.

Recommended: swap only in the secret's places. Every draft that tested the deny sentence hit the
`env` case, and the slot design is the only one measured against real programs. The keys that sign
requests, SSH keys and database passwords are answered by entries of their own in open drafts:
`signing-keys-re-signed` (#361), `ssh-keys` (#356) and a database secret (#355, page 3); a request
signed with a placeholder towards another host is then forwarded and recorded, as here (#364).

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

An agent's part, 22 November 2026. An agent in main uses your OpenAI key, which is sent as it is in
each request. It also uses your AWS keys, which are never sent: the program signs each request with
them inside the computer.

The options:

- A placeholder, swapped on the way out (recommended). The computer holds a stand-in, and Silo puts
  the real value in only on the way to the hosts you allowed. It costs the kinds of secret that are
  not sent as they are: AWS keys, SSH keys and database passwords stay ordinary files the agent can
  read, until a way is found for each. Silo also opens the encrypted traffic to those hosts.
- The real value inside, as everywhere else. The computer gets the real value in its environment,
  and everything works. It costs the secret: an agent can read it and send it to any host it
  reaches.

Recommended: a placeholder, swapped on the way out. It covers the keys agents use most, and it says
plainly which ones it does not cover yet.

## Notes

Which process holds the real values is the open question `which-process-holds-the-boundary` in
`engine`.

# How does a computer use a secret it cannot read

## The choice

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

## Where a placeholder travels: a real case (24 November 2026)

An agent's part, 24 November 2026. Claude Code 2.1.331 ran with three placeholders in its
environment: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` and `GITHUB_TOKEN`, each a made-up value. The
fake model asked for one command, `env | grep -i -E "key|token"`, as an agent looking for
credentials would.

- Claude Code sent its own key's placeholder in the `x-api-key` header of every request, as
  expected.
- The command printed all three placeholders. Claude Code sent that output to the model as the
  call's result, so from then on every request to `api.anthropic.com` carried all three
  placeholders in its body, gzip-compressed. Claude Code also put a token of its own in the
  command's environment, and that token went into the conversation the same way.

Read against the answer, two things go wrong:

- **"A request that carries the placeholder to any other host is denied"** denies every later
  request of this conversation: the OpenAI and GitHub placeholders are now part of its history, and
  `api.anthropic.com` is not a host allowed for them. One `env`, or one `cat .env`, ends the
  conversation for good. The denial protects nothing, because a placeholder is worth nothing away
  from the boundary.
- **Swapping "on the way to the hosts you allowed"**, if it means anywhere in the request, puts
  real values into the model's context. Anthropic's placeholder is in the body of a request to
  Anthropic, an allowed host, so the real key would be written into the conversation. Worse, an
  allowed host can publish: an agent that posts the GitHub placeholder in the body of an issue
  through `api.github.com`, the host allowed for that token, would have the real token published in
  the issue. The body is also compressed, so a swap there means compressing it again. The request
  then no longer leaves as the program wrote it, against `secret-and-prompt-cache`.

What the agent would put in its place, for polarzero to weigh: a secret names where it goes in a
request, such as the `Authorization` or `x-api-key` header for the hosts allowed. Silo swaps it
there and nowhere else. A placeholder seen anywhere else, in a body or towards another host, is
forwarded as it is, and the record keeps it as a fact. Nothing is denied for it, since what leaves
is only the placeholder. A program that sends its key somewhere else (a query string, a body) is
found when the secret is set up, and the app says that secret cannot be swapped.

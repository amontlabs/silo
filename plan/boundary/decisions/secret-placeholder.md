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

An agent's part, 25 November 2026 (`research/2026-11-25-boundary-end-to-end`). One boundary swapped
placeholders for two computers, one reaching it by a proxy inside (#378's `CONNECT`) and one by
gvproxy's hand-off (#380). The swap worked the same on both, and Claude Code ran a task on each
holding only its placeholder. The answer's sentence "No proxy is configured inside" conflicts with
#378, which configures one, and the swap does not need it. Proposed in its place: "The real value
is swapped in by the boundary, whichever way the computer's requests reach it."

The same run needed one placeholder per secret and per computer. #377 gives every computer the same
one, `placeholder-not-a-key`. With that, Silo cannot tell which computer's placeholder arrived, and
#374's check that a key was given to this computer passes any computer's. The placeholder must also
keep the shape programs read: Claude Code tells a key (`sk-ant-api03-…`) from a sign-in
(`sk-ant-oat01-…`) by it, and Azure's tokens must be JWTs.

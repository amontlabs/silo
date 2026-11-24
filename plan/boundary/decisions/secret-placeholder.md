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

## A secret bound to a path, 24 November 2026

An agent's part. Your Anthropic API key is allowed for `api.anthropic.com`, so that the agent can
call the model. The same key, on the same host, also reaches the endpoints that upload files and
run batches. Bound to the host, the key opens all of them.

Two products for agents read in this research bind a secret more narrowly. One injects a key only
on a request that matches a method and a path, such as `POST /v1/messages`, and answers anything
else on that host with a refusal. Another can also pin the model a request names. `secret-limits` already
limits a secret to requests that only read. Binding to a path goes one step further. It
needs the request to be read, which Silo already does on a host that gets a secret
(`ending-the-encrypted-connection`). What is to find out is the cost: how a person writes such a
binding without breaking the agent program, and which programs call more paths than they need.

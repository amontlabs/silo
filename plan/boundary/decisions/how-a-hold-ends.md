# How is a held request refused on each kind of connection

## The choice

An agent's part, 22 November 2026. A rule holds every request from main to api.stripe.com, and you
do not answer before the hold's limit. Another rule holds an `ssh` connection from main to your
server.

The options:

- In each connection's own terms (recommended). A web request Silo reads gets the status 403, with
  words that say held and not answered, or denied. The usual programs do not retry a 403 by
  themselves. The `ssh` connection, which Silo does not read, is reset. A name lookup is refused.
  Other traffic with no connection to reset is told the port cannot be reached, so the program stops
  waiting at once. It costs the words on the `ssh` side, since a reset looks like a network fault to
  the agent. Which status and which words each agent program reads best is still to find out.
- As a server error. The request gets the status 503, as from a service that is down. It costs
  repeated asks: programs retry a 503 by themselves, and each retry is held and asks you again.
- In silence. Nothing comes back. It costs the agent's time: it waits for its own timeout, often
  minutes, then tries again.

Recommended: in each connection's own terms. A refusal that programs do not retry by themselves
means one ask per request, and the agent learns at once what happened.

## Notes

Left for when it is built, with what the message of a hold says. What is known:

- Silo can put words in a reply only while the caller still listens and where the reply has room
  for words.
- A raw connection can only be refused.

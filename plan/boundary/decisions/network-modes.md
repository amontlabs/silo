# Which network settings does a computer have

## The choice

An agent's part, 22 November 2026. You make a fork to try a tool you do not trust yet, and you want
to say what it can reach. Later an agent in it starts sending files to a host you do not know, and
you want that cut off now.

The options:

- Three settings (recommended). A computer is open, limited to a list of hosts, or closed, and you
  switch in one step. Closing also cuts the connections already open. It costs what "open" always
  costs: an open computer can send anything it reads to any host.
- Rules only. There are no named settings, and you write one rule per host. It costs reading a list
  of rules to know where a computer stands, and cutting it off takes more than one step.

Recommended: three settings. Comparable tools offer the same three (off, a list, everything), and
one step to close is what you want when something goes wrong.

## What "a host" is on a connection Silo does not read (24 November 2026)

An agent's part, 24 November 2026. Reasoned, with the sources named; not tried against a CDN.
A computer limited to `github.com` opens an encrypted connection that Silo passes unread
(`ending-the-encrypted-connection`). The only name Silo sees is the one the program puts in the
first message of the connection, the server name. Four ways an agent with root can make that name
lie:

- **Its own address.** The agent sends the name `github.com` to an address of its own choosing,
  for example with `curl --connect-to github.com:443:<its server>:443`. If Silo forwards to the
  address the computer asked for, the list is worth nothing. Silo has to look the name up itself
  and connect to the address it found, whatever address the computer used.
- **No name at all.** A connection to a bare address carries no server name. In a limited computer
  it is refused.
- **A shared front.** On a CDN, the name in the first message picks the certificate, and the name
  inside, which Silo does not read, picks the site. A 2024 study at the ACM Web Conference found
  22 of 30 providers still letting the two differ, Akamai and Fastly among them. So a host on such
  a CDN, allowed and unread, can carry traffic to any other site there. Hosts Silo reads are safe
  from this, since it sees the inner name.
- **An encrypted first message (ECH, RFC 9849).** The real name is then encrypted and the visible
  one is the provider's, such as Cloudflare's. A program gets the key for it from a DNS record of
  type `HTTPS`. Silo answers the computer's lookups (`how-a-hold-ends`), so it can leave those keys
  out, and refuse a first message that still carries ECH towards a host it does not read.

What I would add to `network-modes`: in a limited computer, a host on the list is reached at the
address Silo itself finds for it, and a connection without a name, or with an encrypted one, is
refused. And the app should say that a host on a shared CDN, allowed and not read, is a way out to
the rest of that CDN, unless Silo reads it.

Sources: "Domain fronting in 2026" (routeharden.com/blog/domain-fronting-2026), which cites the
2024 study; RFC 9849.

## The hosts an agent program reaches besides its model (24 November 2026)

An agent's part, 24 November 2026. In a limited computer, an agent program reaches more than its
model. Claude Code 2.1.331 and Codex 0.172.0 each ran one turn with the model on a local stand-in
and every other connection sent to a stand-in for the boundary that refused it, reset it, or never
answered.

- **Claude Code** opened five other connections in one `claude -p` turn, all to
  `api.anthropic.com`, the model's own host, for its event and metric reports, even with the model
  at another address. **Codex** opened four: `github.com` and `api.github.com` (its check for a new
  version), `chatgpt.com` and `ab.chatgpt.com`.
- **Refused or reset, they cost nothing.** Both programs answered in under a second, as with every
  host open.
- **Left without an answer, they cost 10 to 12 seconds.** The same turn took 12.5 s in Claude Code
  and 10.2 s in Codex, waiting on reports that never left, and this is paid on every run of
  `claude -p` or `codex exec`, which agents start from commands.

What I would add to the answer: a limited computer refuses at once, never in silence, and the
hosts each program reaches besides its model belong in the piece that adds the program
(`recipe-names-its-needs`), so that a new computer does not raise an alert or an ask for
`ab.chatgpt.com`. The scripts are in the project files, `research/2026-11-24-side-hosts`.

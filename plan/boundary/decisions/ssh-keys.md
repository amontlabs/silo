# How does a computer use an SSH key it cannot read

## The choice

An agent's part, 24 November 2026. An agent in main clones `git@github.com:you/site.git` and later
runs `ssh deploy@your-server` to read a log. Both need your SSH key. A placeholder cannot stand in
for it: SSH signs a value that is new for each connection, so only the real key can make the
signature.

The options:

- HTTPS for Git, and an agent held by the boundary for the rest (recommended). Inside the
  computer, Git rewrites `git@github.com:` to `https://github.com/`, so a clone and a push use the
  token swap that already exists, with its limits. For `ssh` itself, the computer gets an SSH agent
  that Silo serves from your Mac. The key never enters the computer. The agent signs a login only
  when Silo saw that connection leave the computer, to a host on the key's list, with a host key
  Silo checked against its own list. It costs an SSH agent in the boundary. Programs that do not
  say which connection a signature is for (OpenSSH before 8.9, Paramiko, Go's SSH library) are not
  refused: Silo finds the connection from the session id in what they ask it to sign.
- Silo logs in for the computer. The boundary ends the computer's SSH connection with a host key
  of its own, and opens a second one to the real host with your key. It sees every Git push over
  SSH, so it can limit them. It costs a whole SSH server and client in the boundary, and forwarded
  ports, jumps through other hosts and file transfer need work one by one.
- Short-lived certificates. The computer holds a key of its own, and Silo signs a certificate for
  it that lasts minutes. It costs every host you reach: each must trust Silo's authority, which
  you can arrange on your own servers but not on GitHub for most accounts.
- The key inside the computer. Everything works. It costs the key: an agent can read it and use it
  from anywhere.

Recommended: HTTPS for Git, and an agent held by the boundary. Git is most of what agents do over
SSH, and the rewrite costs nothing. The agent keeps the key on your Mac for the rest.

## Notes

Agents' research of 24 November 2026, in `research/2026-11-24-secrets-not-sent-as-is/` of this
repository.

- The danger of a forwarded agent is a relay, not a stolen key. Someone outside opens a connection
  to GitHub, passes what must be signed into the computer, and the agent inside signs it. The
  signature is sent back out, and they are logged in as you. Since OpenSSH 8.9, a client tells the
  agent which connection a signature is for, with the host's own signature of that connection. Silo
  can check that signature against the ones it saw cross the boundary, in the clear, at the start of
  each SSH connection the computer opened. A connection opened elsewhere was never seen, so the
  relay fails. No product read in this research makes that check, because none of them holds the
  computer's whole network.
- Docker Sandboxes forwards the agent of the Mac by default, with no list of hosts. 1Password's
  agent asks per key and per program, but its prompt names the program, not the host.
- Claude Code's sandbox has no answer for SSH. Its documentation says to switch the remote to
  HTTPS.
- `secret-placeholder` is reworded to match once this is taken: an SSH key is no longer "to find
  out".

## Measured

An agent's part, 25 November 2026, in `research/2026-11-25-credential-brokering/` of this
repository. A prototype of the recommended agent ran on Linux against OpenSSH 9.6, Paramiko,
asyncssh, Go's `x/crypto/ssh` and Git.

- It works. OpenSSH logged in, cloned, pushed and signed a commit with the key on the Mac.
- It refused each case it should: a host not on the key's list, a host key Silo does not know, a
  server reached without crossing the boundary, the relay above, a signature asked with no
  connection behind it, a file signature outside the `git` namespace, and `ssh -A` onward from the
  server. Plain forwarding allowed every one.
- Paramiko, asyncssh and Go send no bind. The boundary can still tie their request to a connection:
  the data to sign holds the session id, and the host signed that session id in the clear at the
  start of the connection. Silo checks that signature against each open SSH connection it carries.
  With that, all three worked, and every refusal above held. The choice above is reworded to match.
- A tunnel gets through. An outsider who reaches a program in the computer can have it open the
  connection to `github.com` and carry the outsider's own SSH session inside it. Silo sees a real
  connection from the computer, to an allowed host, with the right host key, and the agent signs.
  The outsider holds the session and is logged in as you. So the relay fails only for a connection
  Silo never carried. What the agent promises is narrower: it signs for connections that cross the
  boundary, to hosts on the key's list, with a host key Silo checked. Only a boundary that ends SSH
  itself sees what a session does.
- The rewrite to HTTPS is followed by `git` and by cargo (libgit2). go-git ignores it and uses the
  SSH agent, which then works through the agent above.

## Who drives a session

An agent's part, 25 November 2026, in `research/2026-11-25-ssh-tunnel-tell/` of this repository.
The tunnel above was taken further: can the boundary tell a login that a program in the computer
makes from one it only carries for someone outside? Four ways were built on Linux and measured
with OpenSSH 9.6, Paramiko, asyncssh and Go, idle and with the CPU oversubscribed, against an
outsider at round trips from 0 to 50 ms.

- A check that the computer makes, such as asking the boundary to check the host key before the
  agent signs, gives nothing. The program that carries the outsider's connection passes that
  request on too, as it passes the agent's. This holds for any check made of messages: whatever
  the computer can receive or send, that program can forward.
- The shape of a session after login (how fast the client answers the server's data) does not
  separate the two: a local OpenSSH answered more slowly, at the median, than an outsider 2 ms
  away.
- The time of the key exchange does not either. The first encrypted packet cannot leave before the
  server's reply reached whoever makes the session keys, but OpenSSH 9.6's default exchange
  (`sntrup761x25519`) takes the client about 20 ms itself, and Paramiko waits about 40 ms, so a
  local login looks like an outsider 20 to 40 ms away.
- The time from the signature to the client's next packet does. The signed login request has to
  reach whoever holds the session keys and come back encrypted. A local client sent it within a
  millisecond in almost every login; an outsider never sooner than its round trip.

So the agent can add a deadline: after it signs for a connection, a client packet that arrives
later than a few milliseconds, before the server has said anything since, closes the connection
unforwarded. The signature is for that connection only, so it dies with it. The deadline stopped
every outsider farther than it, using OpenSSH through a tunnel.

It does not stop an outsider who knows of it. Sending the server a packet it must answer, at the
moment it asks for the signature, lets the server's answer clear the deadline before the signed
request arrives. With the server 20 ms from the boundary, that worked for 28 of 30 logins from an outsider 20 ms
away and all 30 from 50 ms; at 5 and 10 ms, every login was still cut.
Nor does it stop a program in the computer that logs in itself and then hands its session keys
out: that program is code in the computer, and only what sees processes can name it.

What it costs: logins cut when the computer's CPU is overloaded, each fixed by a retry. With a
5 ms deadline and twice as many busy processes as CPUs, 7 of 150 local logins were cut; idle, none
of 150. The slowest local answer measured was 10.7 ms, so a 12 ms deadline would have cut none of
these and still stopped every outsider 20 ms away or more. The deadline has to be set from
measurements on a Mac, where the computer is a virtual machine.

The options for this entry:

- Keep the agent as above, and state its limit: it signs for connections that cross the boundary,
  to hosts on the key's list, with a host key Silo checked. It does not know who drives them.
- Add the deadline (recommended). It costs a timer per signature, and closes the case of an
  outsider using a stock SSH client through a tunnel from farther away than the deadline. Each
  login's time from signature to answer becomes part of the signature's fact, so the record shows
  how near the session keys were. The limit above still holds and is still stated.
- End SSH at the boundary (the second option of the choice). It sees what a session does, but not
  who drives it either, and it costs an SSH server and client in the boundary.

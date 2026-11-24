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
  Silo checked against its own list. It costs an SSH agent in the boundary, and a refusal for the
  few SSH programs that do not say which connection a signature is for: OpenSSH before 8.9, and
  some libraries such as Paramiko and JGit.
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

# A person's secrets on a team's host

## The choice

An agent's part, 25 November 2026. Ana's computers on the team's host push to GitHub with her
token, sign in to Google Cloud with her account (`cloud-credentials`) and use her SSH key
(`ssh-keys`). On her Mac, the values stay with Silo on the Mac. On the host, something on the host
has to put them in her requests.

Measured ([the research](../../../research/2026-11-25-shared-linux-host/README.md), `creds.sh`), with a process standing in for Ana's boundary:

| Who tries to read the secret in Ana's boundary | Ana's boundary as is | Marked not dumpable |
|---|---|---|
| Another process of Ana's Unix user | read | refused |
| A process of Ben's Unix user | refused | refused |
| root | read | read |

Ana's SSH agent, its socket in a folder only she can open: Ben's process was refused, root listed
her key. The kernel here has no Yama setting, which on Ubuntu by default also stops a process from
reading another of the same user.

The options:

- On the host, in the person's own boundary process (recommended). Run as their Unix user and
  marked not dumpable, so neither another person nor another of their own processes reads it. The
  host's administrator can, and Silo says so when a secret is first used on a host. It costs
  trusting whoever runs the host, as with any shared build machine.
- On the person's Mac, asked for at each use. It keeps an SSH key off the host, since the host only
  gets signatures. It does not keep a token off the host: the swapped value is in the request the
  host sends. And it costs every overnight run the moment the Mac sleeps.
- Only the team's own machine identities on a host (a GitHub App, a service account), never a
  person's. It costs everything an agent does as the person.

Recommended: on the host, in the person's own process. A person who does not trust the host's
administrator keeps their computers on their Mac.

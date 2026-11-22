# Does main reach every host its projects need

## The choice

An agent's part, 22 November 2026. Main holds your three projects. The first deploys to Vercel, the
second talks to Stripe, and the third only needs GitHub.

The options:

- Main is limited like any computer (recommended). Main reaches the hosts you allowed for it and no
  others. It costs a list that grows with every project, and an agent working on the third project
  can reach Stripe too, because the list is main's and not the project's.
- Main is open. Since main needs so many hosts, its network is left open and only forks are limited.
  It costs the most where it matters most: the computer that holds all your projects can send what
  it reads to any host.

Recommended: main is limited like any computer. Main is where your private code and untrusted
content meet, so it is the last computer to leave open. Limits per project inside main cannot be
enforced from outside, so a narrower reach takes a fork.

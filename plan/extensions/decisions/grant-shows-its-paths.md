# A grant shows its paths

## The choice

An agent's part, 22 November 2026. You are about to install Disk watch on main. Its computer may
reach hooks.slack.com. Before you confirm, the app shows one new path: main, then Disk watch's
computer, then hooks.slack.com.

The options:

- Show every path (recommended). You see where your data can end up, not only what the extension may
  read. It costs Silo working this out from the rules of every computer on the way, and when one of
  them may reach any host, the line says "any host", which warns you but tells you little more.
- Show only what is granted. The app lists "reads /work on main" and leaves the rest to you. It
  costs you the step from there: you can grant an extension main's files without seeing that they
  can reach Slack.

Recommended: show every path. Where your data can end up is the question a person has before
plugging something in, and browser extensions and phone apps that list only permissions leave it to
you.

## Notes

polarzero wants to plug something in and know, completely, what it can affect on the network and
the disk, and what goes in and out of a computer. Three kinds of knowledge answer it. This
decision holds the first; `extension-bound-checked-once` the second; the later feature
`extension-trial-on-the-past` the third.

The text it is put forward from:

> **What it can do, at most.** This is known before it runs, from its access alone: the rules of
> its own computer, and the calls its access allows. Silo can compute it as paths: from which
> computer data can reach which host. Letting a classifier read main adds one path: what is
> recorded about main, to the classifier, to the hosts the classifier may reach. The app shows the
> paths a grant adds before you confirm it.

That every path out of a computer is among the paths Silo shows is a property of the part
`boundary`.

# Ports of a computer reached from your Mac

## The choice

An agent's part, 22 November 2026. An agent in a fork starts the development server of your site on
port 3000. You grant that port to your Mac and open localhost:3000 in your browser.

The options:

- Build it (recommended). You see what the agent built in your own browser. It costs a channel: a
  page served from the computer runs in the browser of your Mac, with what that browser holds for
  localhost. Two forks that both use port 3000 cannot both have 3000 on your Mac, and what Silo does
  then is still to design.
- Leave it out. You look at the site on the computer's own screen, inside Silo. It costs you your
  browser, its extensions and its developer tools.

Recommended: build it. Editors and container tools all forward ports for this reason, and a fork you
cannot open in your browser is of little use for web work.

## polarzero's words (21 November 2026)

polarzero believed this was planned: "giving ports from a computer accessible on the machine, which
I believe is already planned."

## An agent's notes

- The channel list names ports (`channel-list`), and `kinds-of-action` files "open a port" as
  granting. No feature built it, so this entry was added.
- Opening a port is granting. You do it, or a rule does. A precaution never does.
- Bad paths to design: two computers listen on the same port number and both want it on the Mac's
  localhost. The port is already taken on the Mac. The computer is paused or stopped while a program
  on the Mac is connected.
- A computer that reaches a port of your Mac is the other direction. It is not in this entry, and
  `nothing-shared-from-your-device` is in force.
- `sign-in-from-a-computer` needs this entry.

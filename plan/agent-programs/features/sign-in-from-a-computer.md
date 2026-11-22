# A sign-in started in a computer

## For the owner

An agent's part, 22 November 2026. In main, Claude Code connects to Linear's MCP server, which asks
you to sign in. With your yes, the page opens in the browser on your Mac, where you are already
signed in to Linear. You approve, and the answer reaches Claude Code in main.

The options:

- Build it (recommended). A sign-in started in any computer finishes in your own browser. It costs a
  yes each time, and a new way for a computer to reach your Mac: it can open a page in your browser.
  The code the provider sends back passes through Silo and stays out of the record. How Silo learns
  the link, and where the answer has to go, is still to find out.
- Leave it out. You copy the link into your browser yourself. A sign-in that offers a code to paste,
  as Claude Code's own does, still works. It costs every sign-in that waits for its answer on the
  computer itself, as many MCP sign-ins do: it never gets it. On a macOS computer you can sign in
  again in its own browser instead.

Recommended: build it. You called it super important, and without it such a sign-in cannot finish at
all in a computer with no screen.

## The owner's words (21 November 2026)

> That thing about signin callback to do from the user machine is super important. I think it's
> related to giving ports from a computer accessible on the machine, which I believe is already
> planned. This is an important feature indeed.

The owner marks this feature as important.

## The need

An agent program or an MCP server starts a sign-in by printing or opening a link. It then waits on a
port of its own localhost for the provider's answer. The link has to open in the browser where you
are signed in, which is on your Mac. The provider then sends that browser to localhost. That is the
Mac and not the computer, so the program never gets its answer. This breaks whenever the program and
the browser are on different machines.

## An agent's notes

- Two things cross. The link goes out, from the computer to the browser on your Mac. One request
  comes in, from the Mac to a port of the computer.
- The program often picks the port at random for each sign-in, so a grant set in advance does not
  fit. Opening a port is granting, so Silo asks you. How Silo learns the link and the port is to
  find out.
- Opening a link on your Mac is not one of the channels of `channel-list`. Whether it is a channel
  of its own is to settle there.
- The sign-in stays the vendor's own flow, as `logins` proposes, and nothing
  is disguised (`disguising`). The provider's answer carries a code that works as a login for a
  short time. It passes through and stays out of the record, as the value of a secret does. How is
  to design.
- The owner said on 18 November 2026, in `logins`, that how a login gets into a computer is not for
  Silo to solve. This entry holds no login and moves none. It lets the vendor's flow finish across
  two machines.
- A macOS computer has a browser of its own, where a sign-in can finish with nothing crossing. You
  then sign in to the provider again inside the computer. A Linux computer with no desktop has no
  browser.
- Bad paths to design: you say no. No answer comes from the provider. The program gave up waiting
  before the answer arrived. Two sign-ins wait on the same port number in two computers.

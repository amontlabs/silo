# A sign-in started in a computer

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
- The sign-in stays the vendor's own flow, as `program-and-sign-in-untouched` proposes, and nothing
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

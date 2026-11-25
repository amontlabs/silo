# A sign-in started in a computer

## The choice

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

Recommended: build it. polarzero called it super important, and without it such a sign-in cannot finish at
all in a computer with no screen.

## polarzero's words (21 November 2026)

> That thing about signin callback to do from the user machine is super important. I think it's
> related to giving ports from a computer accessible on the machine, which I believe is already
> planned. This is an important feature indeed.

polarzero marks this feature as important.

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
- polarzero said on 18 November 2026, in `logins`, that how a login gets into a computer is not for
  Silo to solve. This entry holds no login and moves none. It lets the vendor's flow finish across
  two machines.
- A macOS computer has a browser of its own, where a sign-in can finish with nothing crossing. You
  then sign in to the provider again inside the computer. A Linux computer with no desktop has no
  browser.
- Bad paths to design: you say no. No answer comes from the provider. The program gave up waiting
  before the answer arrived. Two sign-ins wait on the same port number in two computers.

## How Silo learns the link and the port (24 November 2026)

An agent's part, 24 November 2026. Tried on Linux with a fake MCP server that signs in with OAuth
(dynamic registration, a code with PKCE, then tokens), and `mcp login` in Claude Code 2.1.331 and
Codex 0.172.0. `BROWSER` pointed at a small script standing in for Silo: it logged the link it was
given, followed the server's redirect, and called the address it was sent to, as your browser
would after your yes.

- **Both open the link through `BROWSER`.** Each ran the script with the link as its one argument.
  So a command of Silo's in the computer, named by `BROWSER` in its environment, learns every link
  a program opens, with no change to the program. `xdg-open` reads the same variable.
- **The port is in the link.** Each link carried `redirect_uri=http://localhost:50946/callback`
  (Claude Code) or `http://127.0.0.1:37285/callback/<id>` (Codex), a new port each time. Silo reads
  it there before it opens anything.
- **Both finished.** The script's call to that address got a 200, and each program saved its
  tokens: "Authenticated with "fake"" and "Successfully logged in to MCP server 'fake'".
- **Claude Code also takes the answer pasted.** In a terminal it shows "Or paste the redirect URL
  here", so the answer can reach it as text. Without a terminal it refuses: "this isn't an
  interactive terminal". Codex has `--no-browser`, which prints the link and takes the answer the
  same way.

So the two crossings of the notes are known before they happen: Silo gets the link from its
`BROWSER` command, opens it on your Mac with your yes, and listens on the port named in the link,
on your Mac, only until that one answer comes, then passes it to the same port in the computer.
Two computers that pick the same port at once is the case left: Silo can refuse the second until
the first is done. The scripts are in the project files, `research/2026-11-24-mcp-oauth`.

# How any program on the Mac reaches any computer

polarzero said this, on 18 November 2026: people use the app of their agent program on the Mac
and connect it to a machine over SSH. That can be the way into a computer. It is not yet clear
how it works with many computers. Adding each one to SSH by hand is too heavy, a user switches
between computers, and an agent may start other computers. polarzero expects this to need
several discussions, and wants it researched first: similar tools, and people who have the same
problem.

The question has three parts: `reaching-a-computer`, `following-the-work` (with
`conversation-taken-up-again`, in `going-back`) and `agents-making-computers`.

## The choice

An agent's part, 22 November 2026. A fork was made a moment ago and you gave no yes for it. Your
editor already moves files in and out of it, and it already reads your repositories. The plan
proposes that nothing is open in a new computer until you allow it, and these two are exceptions.

The options:

- Only your own way in (recommended). One yes lets your Mac into every computer, present and future,
  and Silo shows it on each one. Repositories get no special treatment. It costs a standing grant
  that covers computers not made yet. A fork also reads a private repository only once a rule of
  yours gives it a secret that only reads.
- No exception at all. Each new computer needs your yes before your terminal or your editor reaches
  it. It costs a prompt on every fork.
- Both stay as defaults. Your way in and read-only access to repositories exist on every new
  computer. It costs the principle, which is reworded to name both exceptions, and your decision
  that no destination is special, since Silo then treats repository hosts apart.

Recommended: only your own way in. Reaching your own computers is what Silo is for. Treating
repositories apart goes against your decision that no destination is special.

With this answer, "repository access" leaves the list of channels (`channel-list`) and your way in
joins it. What a fork is given changes the same way (`what-a-fork-is-given`).

One thing must change to build it. `secret-limits` proposes that a secret that only reads is sent
only with GET requests, and `git fetch` over HTTPS also sends a POST. That limit needs another
definition, or a token that the host itself limits to reading.

## What is known

- Over SSH, the app on the Mac is only the window. The agent program runs inside the computer, so
  its model traffic still crosses the boundary. The Claude desktop app installs Claude Code on
  the remote machine, and the Codex app starts its server there.

## What the research of 18 November 2026 found

- A window finds a conversation in two ways. The user points the app at a machine over SSH (the
  Claude desktop app, the Codex app, T3 Code, Zed, Cursor). Or the conversation announces itself
  through the vendor's account (Claude Code's Remote Control, the vendors' cloud sessions).
- No app documents a way for another program to add a machine to it. Most read the Mac's SSH
  settings. The Codex app lists only the machines named one by one there.
- What people mention most is not adding machines by hand. It is signing in again on every fresh
  machine, several agents in one machine destroying each other's work, and losing sight of what
  sub-agents do.
- The SSH mode of the desktop apps is young. It has open bugs with several conversations on one
  machine, and for Claude it takes no API key. A terminal over SSH is the path that always works.

## More from the research of 18 November 2026

- From the researchers: Lima and GitHub Codespaces each generate SSH entries for their machines,
  which the user's SSH settings include.
- The protocols that attach a window to a conversation (the Agent Client Protocol, and one VS Code
  is building) leave open how a window finds a conversation. Silo sees every computer from
  outside, so it is placed to answer that. How a conversation is recognised is still open
  (`recognising-a-conversation`).

How sure this research is:

- Each claim of the researchers carries its source. Some rest only on a search summary.
- Checked directly: Anthropic's terms, Claude Code's network documentation and the Codex rule on
  SSH entries.
- Almost no independent user review was found of the tools for following many agents. No Reddit or
  Hacker News thread could be read.
- The findings are kept outside this repository.

## Found by an audit, 22 November 2026

An agent's part. One yes here gives every computer, "including one made a moment ago", a way in from
your Mac, but `channel-question` says every channel is listed, has a rule, and none exists by
default. In a fork with no grant, `scp fork-3:/work/secret.txt .` takes a file out. The audit
proposes to add "the way in from your Mac (a terminal, an editor)" to `channel-list`, and to say
here that the yes grants it to every computer, present and future. A second audit asks to remove
"repository access" from `channel-list`, since `security-framing` says no destination is special.

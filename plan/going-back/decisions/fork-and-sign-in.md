
## A sign-in held outside the computer survives a fork (24 November 2026)

An agent's part, 24 November 2026. The note above reads from the code that a copied sign-in can
sign main out. Tried with Claude Code 2.1.331 and a fake MCP server that rotates its refresh
token on each renewal, as OAuth 2.1 asks of a server whose clients hold no secret. Main and a fork
held the same expired sign-in, and each ran `claude mcp list` in turn, the fork first.

- **The sign-in copied in the file:** the fork renewed and connected. Main's renewal then sent a
  refresh token the server had already replaced, was refused, and Claude Code cleared main's
  tokens from its file. Main showed "Needs authentication" and stayed so. Nothing was done in
  main.
- **The sign-in held by the boundary,** as in the trial of `secret-placeholder` ("An MCP server's
  sign-in, held outside"): both computers held only placeholders, the boundary swapped in the one
  real refresh token and kept each new one. Main and the fork both stayed connected through four
  renewals.

What this changes: when you let a fork carry a sign-in, the answer's own way out, Silo should not
copy the file. It should give the fork the same placeholders and keep one real sign-in outside
for both. One sign-in then renews in one place, so neither computer signs the other out, and
leaving the fork stops its use at once without touching main.

One sign-in shared this way needs one more thing at the boundary. Main and the fork ran
`claude mcp list` at the same moment, against a server that takes 0.3 s to renew. A boundary that
forwards each renewal sent the same refresh token twice: the server took the first and refused
the second, and Claude Code cleared the fork's placeholders, so the fork stayed signed out in every
round after, although the boundary still held a good sign-in. A boundary that lets one renewal
through at a time, and gives a renewal that comes just after another the same reply, kept both
computers connected through three rounds, and the server saw one renewal. So the answer should
say: a sign-in held outside renews once for every computer that uses it. The scripts are in the
project files, `research/2026-11-24-mcp-oauth` (`fork.sh`, `race_slow.sh`, `race2_slow.sh`).


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
leaving the fork stops its use at once without touching main. Two forks that renew at the same
moment must wait on one renewal at the boundary, which this trial did not test. The script is in
the project files, `research/2026-11-24-mcp-oauth/fork.sh`.

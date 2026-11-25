# Copies of the record's head kept off the Mac

## The choice

An agent's part, 25 November 2026, from [the record's head off the Mac](../../../research/2026-11-25-record-head-off-the-mac/README.md).
On Tuesday night an agent in main deletes a staging database. On Wednesday a program running as
you on the Mac (a compromised editor plug-in, or a script you ran) changes line 10 of main's
record, computes every hash after it again and writes a new `head`. `silo verify` says every
chain holds. It cannot say otherwise: whoever can write the record's files can write a whole new
chain. The same goes for the last lines cut away with the head deleted. Agents in a computer cannot
do either, since the record is on the Mac, outside every computer.

Only a copy of the head that such a program cannot change shows it. The test in the research
rewrote and cut a real record of the prototype; `silo verify` passed both, and both anchors below
named the first line that differs from what they hold. The anchor must keep every head it ever
received, not only the last: an attacker may add a head, so what catches them is that they cannot
remove the old ones.

The options:

- **A Git repository with a protected branch (recommended for one person).** The recorder commits
  each head to a repository you own, on a branch a ruleset keeps from force pushes and deletions,
  with a token that may only push to that repository. The prototype's remote refused a force push
  and a deletion, and accepted a new commit that replaced the file's lines; `silo verify` reads the
  heads of every commit, so that changed nothing. It costs a GitHub (or other) repository, a token
  on the Mac, and a remote that sees the times Silo ran. A program with your GitHub owner rights can
  lift the ruleset; one with only the token cannot.
- **The team's host (recommended for a team).** The host a team already runs (`teams`, #362) keeps
  each Mac's heads, refuses a head not signed by that Mac's key and a head older than the last one,
  and the lead runs `silo verify` against it. The developer whose Mac it is cannot rewrite it. It
  costs a host, which a team of one does not have.
- **A public transparency log** (Sigstore's Rekor, Sigsum). Anyone can check the record without
  trusting you or a host. It costs a public entry per head, linked by your key, that tells anyone
  when you ran agents. Rekor's public log removed its search in version 2 and Sigstore keeps version
  1 as its default; Sigsum asks each submitter for a domain to limit its rate. Later, for records
  shown to people outside the team.
- **iCloud Drive.** Rejected: any program running as you writes the folder, and a deletion syncs.
- **A timestamp (OpenTimestamps).** Rejected alone: it proves a head existed at a time, but its
  proof is kept on the Mac, so deleting it hides the head.
- **Nothing.** A head next to the record shows accidents and nothing else. It costs the record as
  evidence for anyone but you, which `changes-to-the-record` promises in part.

Recommended: a Git repository for one person, the team's host for a team, and the same `silo
verify` for both. Either catches the rewrite and the cut in the case above.

What no anchor covers: the lines written since the last head sent. With a head every minute, a
program running as you can cut or rewrite at most the last minute and stop the next head going
out. The team's host can warn when a Mac that is awake stops sending heads.

## The key

An agent's part, 25 November 2026. A signature does not catch a rewrite: heads kept on the Mac,
signed, were deleted with the rewrite in the test, and nothing was left to check. The signature
tells the anchor which Mac sent a head, so a host or a log can refuse a head from anyone else. On a
Mac the key is a P-256 key in the Secure Enclave, usable only by Silo's signed app. Apple lets only
an app bundle with a provisioning profile hold such a key, not a bare command-line tool, so this
depends on how Silo is shipped. Until then the key is a keychain item, which a program running as
you may use; the anchor still catches the rewrite, since the old heads stay.

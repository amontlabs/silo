# Edits to the record show

## The choice

An agent's part, 24 November 2026. A team reviews an incident: an agent deleted a staging database
last Tuesday. The record on the developer's Mac says which run did it, with which secret. The
security lead asks whether the record could have been changed since.

`changes-to-the-record` already says that nobody edits a line, the user included. Nothing yet lets
someone else tell. Auditors say that "logs the dev team can modify or delete" do not hold up
(Sentrely, on SOC 2 and Claude Code), and in the PocketOS deletion of April 2026 "the only audit
trail was the agent's own post-hoc confession" (Zenity). The known practice is a hash chain, as in
signed syslog (RFC 5848) and in Certificate Transparency: each entry carries the hash of the one
before it, and the hash of the newest is kept somewhere else.

The options:

- A hash chain, with its head written out on a schedule (recommended). Each line carries the hash
  of the line before. The hash of the last line can be written every hour to a place the user
  chooses, such as the company's log tool. It costs a few bytes per line, and the bulky content
  stored by hash (`bulky-content-once`) is already covered by its hash.
- Nothing. It costs the record as evidence for anyone but its owner.
- The whole record sent off the Mac as it is written. It costs everything `what-silo-sends-home`
  keeps on the Mac.

Recommended: a hash chain. This is ground work: the record's files carry it from their first
version, or a later change has to rewrite them.

## A head kept next to the record (25 November 2026)

An agent's part, from `research/2026-11-25-record-format/`, added on merge because #386 wrote it before #362 had landed. A head kept next to the record shows accidents, not someone with your user's rights: another program running as you on the Mac can rewrite the record and the head together. Only a copy of the head kept elsewhere shows a rewrite. Agents inside a computer cannot reach the record, which is on the Mac.

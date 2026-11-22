# Hosts reached and denied, in the record

## For the owner

An agent's part, 22 November 2026. At 14:02 a program in main connects to paste.rs for the first
time. You are alerted. No rule allows the host, so the connection is denied. The record keeps the
host and the time, and names `curl`, started by one of the runs, as what the computer reported. For
api.github.com the record shows that 4 KB were sent.

The options:

- Build it (recommended). You see each new host once, each denied attempt, and how much went to each
  allowed host. It costs alerts in the first hours of a computer, when every host is new. They reach
  you grouped, with a count. Keeping what was sent in model traffic also costs disk.
- Leave it out. Rules allow and deny without a trace you can read. It costs you the answer to "what
  did it send, and where?", and the rules Silo could propose from it.

Recommended: build it. An allowed host is still a way out, and the record is what the rest of Silo
is built around.

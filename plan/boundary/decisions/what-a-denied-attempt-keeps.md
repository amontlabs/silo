# What the record keeps of a denied attempt

## For the owner

An agent's part, 22 November 2026. A rule says main never reaches paste.rs. An agent in main uses
`sudo` to stop the probe, the small part of Silo that runs inside main. Then it tries paste.rs. The
connection is denied from outside main, as always.

The promise you took says: "Denied attempts are recorded: the host and the time as facts, the
process as a report." Here no report comes, because the probe is stopped. One command breaks the
promise, and a test that attacks main would show it. A report is what a program inside the computer
tells Silo, and the word "report" in force already says that an agent with full access can interfere
with it.

The options:

- Promise the host and the time (recommended). The record says that main tried paste.rs at 14:05
  and was denied, and that no process was reported. When the probe runs, the line also names the
  process, as a report. It costs the promise about the process, which Silo cannot keep against an
  agent with `sudo`.
- Keep the promise as it is. It costs a promise that one command breaks, at the moment it matters.

Recommended: promise the host and the time. They are seen from outside main, so nothing inside can
take them away. The design document this line came from marks it "(Agreed, Notes)", and your notes
give no reason for the half about the process.

## Notes

`guarantees-outside-the-computer` puts the general question to you with the same case. This answer
holds whichever way that one is decided, since it follows from the word "report".

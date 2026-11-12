---
"silo-ui": minor
---

When an agent in a computer asks for something only you can allow, Silo now shows it. The Pending page (renamed from Pending changes) lists these requests together with the forks waiting for review, newest first; a merge an agent asked for is one row. Open a row to read what is asked in Silo's own words, and the agent's note, which is labelled as the agent's. Allow it, or deny it with a note the agent will read; afterwards the row shows what came of it. A waiting request also shows as a notification, a count on the Pending entry in the sidebar, and a row in the menu bar panel, and clicking the system notification opens the request. A new switch in Settings, Notifications, turns the system notifications for requests from agents off.

Merging a fork's changes now says Merge, and deleting a fork you will not merge says Delete fork. When both a fork and main changed the same thing, Silo lists each conflict with a short note on what each side did and lets you choose per item (the fork's version, main's, or for a repository the fork's commits left as a branch), then merges with your choices. When main had not changed, it says nothing could conflict.

The computers' Activity section has a new tab, Who did what: a table of what you, another device and each computer's agent did, what Silo refused or held for you, with filters by computer and by who, and a link from each held call to its request.

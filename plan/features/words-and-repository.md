## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- Does the public `main` get a cleaned history, with the agents' churn kept on another branch? (`plan/decisions.json`)
  - Recommendation: no. The gate merges straight to `main` and agents may push (agents-may-push), so a cleaned branch would be kept by hand.
- A check fails when the site or the app uses a word the glossary bans, such as fold or promote. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. git-words already bans the words, and only a check keeps agents to them.
- This repository stays public, and is never made private, even before a launch. (`plan/decisions.json`)
  - Recommendation: yes. The Workshop and every link to it depend on it being public.
- App and command-line text uses active sentences with a subject, never clipped labels such as 'held for approval'. (`WRITING.md`)
  - Recommendation: yes. WRITING.md's table has no row for status labels, which the app will be full of.
- A short doc describes the machines Silo's work runs on, such as the Linux test machine, for agents to operate them. (`AGENTS.md`)
  - Recommendation: talk it through. Agents stop rediscovering them, but the repository is public, so it may name only what is safe to publish.
- An agent changes a large structured file through a small script committed with the change. (`AGENTS.md`)
  - Recommendation: no. `plan` already edits the plan's files, and a reviewer reads any other diff directly.

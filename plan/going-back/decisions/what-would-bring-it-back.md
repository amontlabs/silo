# Which of what stays on disk is worth keeping

polarzero wanted to know, when several runs work in a repository, which run did what, what was
pushed, and what remains: a build, a cache, or something to keep such as an instruction added to a
main instruction file. polarzero was wary of sliding back into merging, and reminded that most
people work with Git and GitHub, so most things can be pulled again.

| What brings it back | Examples | How Silo knows | What Silo may do |
|---|---|---|---|
| **A pull** | Commits that are on the remote | The Git state inside the computer. An opinion: the agent controls that content | Nothing is needed. |
| **A rebuild** | Dependencies, build output, package caches | A list of known names per ecosystem, the recipe, and the user's answers | Offer to purge, biggest first. Leaving it out of long history needs a rule that names the paths (`rule-that-names-paths`). |
| **Nothing: it is only here** | Unpushed commits, uncommitted work, an untracked `.env`, an edited instruction file, a new skill, a database, anything unknown | Everything not in the first two | Never purge it. List it before a drop. Offer to save it. |

This is the diff of the whole machine reduced to the part worth reading.

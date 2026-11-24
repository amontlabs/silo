# Merges and deploys held for your yes

## The choice

An agent's part, 22 November 2026. You switch the set on for main and leave. An agent runs `gh pr
merge 42`. The merge waits, and your phone shows an ask: merge pull request 42 into the main branch
of your site. You say yes.

The options:

- Build it (recommended). Agents work through the night and the two steps that reach other people
  wait for you. It costs a claim that is hard to keep whole: a merge can be a push, a call to the
  host's API or a click, and a deploy can be any request or command. The rules hold what they name
  and nothing else, so the set has to say what it does not catch. It also needs the phone app, and a
  hold that passes its limit is refused.
- Leave it out. You write the rules that hold pushes to your default branch yourself. It costs you
  the work of finding every form a merge and a deploy take.

Recommended: build it, in the stage "later" where it sits. These are ordinary rules shipped as a
set, off until you switch it on, and no destination is treated as special.

## Notes

From the list of smaller features polarzero put on the roadmap on 21 November 2026. polarzero's
words on the list are in `what-ships-with-silo`.

## An agent's notes

- It is a named set of rules (`rule-sets`), off until you switch it on. `security-framing`, taken,
  says no destination is special: these are ordinary rules, shipped as a set.
- How Silo recognises a merge into a default branch or a deploy at the boundary is to find out. A
  merge can be a push, a call to the host's API, or a click you make yourself. A deploy can be a
  request to a host or a command, and you would have to name it.
- `ask-reaches-you`, which has no verdict, proposes that only Silo's own apps answer, the phone app
  included, and that a chat app or a forwarded message "does not answer". An answer given in Silo's own `phone-app` is given in Silo. An answer
  given through `forwarding-to-a-phone` would go against it. polarzero has not ruled.
- `hold-limit`, taken, gives every hold a limit. A merge that waits for your phone can pass it, and
  is then refused as held and not answered. The agent program may also give up first
  (`program-gives-up-first`).
- An extension that adds rules could ship this.

## What the boundary can read of a merge (24 November 2026)

An agent's part, 24 November 2026. The notes leave open how Silo recognises a merge into a default
branch. Tried with git 2.43 against a local server over git's HTTP, and read from the shipped
`gh` 2.89; GitHub itself was not reached.

- **A push over HTTPS says which branch it changes, in its first bytes.** Each push is one request
  to `…/git-receive-pack`, and its body starts with one line per branch: old commit, new commit,
  branch name. They came uncompressed for every push tried: a new branch, a fast-forward of main,
  a force push, a deletion, a tag, and a 3 MB push, which git sent as an empty probe request and
  then the real one. So Silo can hold a push to main after reading only those lines.
- **git has its own refusal for a push, and shows Silo's words in it.** Answered with git's
  per-branch report ("ng", with a reason), the push printed `! [remote rejected] HEAD -> main
  (<Silo's words>)` and failed, with nothing written; a push to another branch in the same session
  went through. That reads better than a 403 and names the branch.
- **git waits for a held push with no limit of its own.** A push held for 200 s finished when the
  answer came. The limit is the agent program's (`program-gives-up-first`) and the hold's.
- **The default branch is not in the push.** The list of branches a push starts from does not say
  which one is the default; the list a fetch starts from does (`symref=HEAD:refs/heads/main`). Silo
  can ask for it once, or the set can name the branch.
- **`gh pr merge` names the pull request by an opaque id.** It sends the GraphQL mutation
  `mergePullRequest` with the pull request's node id, not its number, repository or base branch.
  Silo learns which pull request and branch only from gh's earlier request for the pull request,
  in the same traffic, or by asking GitHub.
- **Auto-merge and merge queues are merged by GitHub, later.** `gh pr merge --auto` sends
  `enablePullRequestAutoMerge`; the merge itself happens on GitHub when the checks pass, and never
  crosses the boundary. So the set can hold only the request that turns auto-merge on, and has to
  say so.
- **A push over SSH cannot be read.** The branch is inside the encrypted connection. With the key
  held outside (`secret-placeholder`, SSH), Silo sees that the key signs for github.com, not what is
  pushed. The set can hold every SSH push to that host, or say that it does not catch them.

What I would add to the answer: the set names what it catches (a push to the default branch over
HTTPS, `mergePullRequest`, turning auto-merge on, a click in the host's page is not caught), and a
push it holds is refused in git's own words. The script is in the project files,
`research/2026-11-24-merges`.

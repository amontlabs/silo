# tools/workshop

The Workshop's GitHub side: the GitHub App that signs people in and holds the conversation as
issues of `amontlabs/silo`, its credentials, and the model of the labels, markers and history
comments on those issues. Node 26, no dependency. `npm test` runs the tests, which use a fake
GitHub (`test/fake-github.js`) and a fake Keychain, never the real GitHub or the real Keychain.

The plan tool ([`tools/plan`](../plan/README.md)) reads and writes no GitHub. A verdict lives in
the plan's file only.

## Commands

`bin/workshop`. Every command takes `--json`, before or after the command name. Exit codes: 0
done, 1 refused or a problem found, 2 wrong use, 3 GitHub or the credentials to reach it could not
be used.

| Command | What |
|---|---|
| `workshop` | The commands, one line each. |
| `workshop setup [--org <org>] [--site <url>] [--replace]` | Creates the App (below). |
| `workshop store [--replace]` | Finishes a setup whose Keychain step failed, from `vercel.env`. |
| `workshop doctor` | Where each credential comes from (never a value), whether the App is public and installed, its permissions, and whether the labels something still sets exist. Exit 1 on a problem. |
| `workshop decline <issue> --why "…"` | Closes an open suggestion as not planned, the reason as the bot's comment. Refuses an issue that is not an open suggestion (an entry, an issue without the suggestion label, a closed one). A run stopped after the comment and run again posts no second comment. |

`doctor` checks one label: `workshop:suggestion`, which the site puts on each new suggestion.
Nothing sets `workshop:entry`, `workshop:building` or the three verdict labels any more; they stay
on the issues already on GitHub, where the site still reads them, so their absence is no problem.

## The App

`workshop setup` opens a page on `127.0.0.1` (it answers no other Host) that posts the manifest to
GitHub: "Silo Workshop", public, Issues write and Metadata read, no webhook, sign-in callback
`<site>/api/workshop/callback`. The owner clicks "Create GitHub App", then "Install" on
`amontlabs/silo` only. The command writes the seven variables Vercel needs to
`~/.local/state/silo-workshop/vercel.env` (0600) first, then the App id, client id, client secret
and private key to the Keychain (service `silo-workshop`, through `security -i` on stdin). It never
registers a second App by accident; `--replace` registers a new one.

Credentials are looked up in the environment, then the Keychain (a locked one stops the command),
then `~/.config/silo-workshop/credentials.json` (0600 or refused). `WORKSHOP_GITHUB_API` and
`WORKSHOP_REPO` override the API and the repository. No credential, token or key is ever printed
or logged.

## The issues already on GitHub

Labels: `workshop:entry`, `workshop:suggestion`, `verdict:take`, `verdict:later`, `verdict:drop`,
`workshop:building`. They keep these names, `verdict:later` included, so the closed issues still
read. Only these three are verdict labels (`verdict:refine` is none), and label names read from
GitHub are compared ignoring capitalisation. An entry's issue is one the bot created whose body
ends with `<!-- silo-entry: <id> -->`; its body also holds `<!-- silo-synced: <commit> -->`. A
history comment ends with `<!-- silo-change: {json} -->`, naming the commit, the commit before and
the fields that changed. The fields a history comment may name are fixed in `model/names.js`, so
the comments already written still read whatever the plan's files become.

## Modules

- `model/`: the names and labels, the three markers and their readers, the history comment and
  its links, the wanted issue of an entry, the readers of the bot's claim comment and of verdict
  labels. Plain ESM; it imports nothing outside its folder and no `node:` module (a test enforces
  it). The site's server imports it, and the site's function ships this folder with
  `package.json` and nothing else of `tools/`.
- `github/`: the REST client, the App's JWT and installation token, credentials, the Keychain,
  `setup` and `store`, `doctor`, `decline`.
- `cli/`: the commands.

## What was deleted

With this tool's move out of `tools/plan`, these were deleted, not ported:

- `plan sync`, the job that created and updated one issue per feature, closed the issues of
  features no longer in the plan, and posted history comments.
- `plan pull`, the job that copied a verdict label from an issue into the feature's file, and
  `bin/push-main`, which pushed that commit.
- `.github/workflows/plan.yml`, which ran both on every push to `main` and on every verdict label.
- `plan decide`, `plan start` and `plan stop`, which set verdict and building labels.
- The live state the plan tool read from the site or GitHub.
- The site's `POST /api/workshop/verdict`, which set a verdict label from the Workshop page.

No push acts on GitHub any more, and nothing sets a verdict or building label. The site still
reads issues, comments, votes and suggestions, still writes comments, votes and suggestions as the
person signed in, and still puts `workshop:suggestion` on a new suggestion.

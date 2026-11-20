# tools/workshop

The Workshop's GitHub side: the GitHub App that signs people in and holds the conversation as
issues of `amontlabs/silo`, its credentials, and the model of the labels and markers on those
issues. Node 26, no dependency. `npm test` runs the tests, which use a fake
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
Nothing else sets a label any more, so nothing else is checked.

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

Two labels: `workshop:suggestion` on a person's suggestion, and `workshop:entry` on an older issue
the bot created for one feature. Label names read from GitHub are compared ignoring
capitalisation. An entry's issue ends with `<!-- silo-entry: <id> -->`. The bot once wrote a comment
on an issue for each change of its entry; that comment is read as any other comment now, and the
history of an entry comes from git (`tools/plan/history`).

## Modules

- `model/`: the names and labels, and the entry marker with its reader. Plain ESM; it imports nothing outside its folder and no `node:`
  module (a test enforces it). The site's server imports it, and the site's function ships this
  folder with `package.json` and nothing else of `tools/`.
- `github/`: the REST client, the App's JWT and installation token, credentials, the Keychain,
  `setup` and `store`, `doctor`, `decline`.
- `cli/`: the commands.

## What the site does with GitHub

No push acts on GitHub. The site reads issues, comments, votes and suggestions, writes comments,
votes and suggestions as the person signed in, and puts `workshop:suggestion` on a new suggestion.

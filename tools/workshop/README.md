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
| `workshop doctor` | Where each credential comes from (never a value), whether the App is public and installed, its permissions (a problem without Issues or Contents write), whether the two labels the site sets exist, and the entries that several issues name. Exit 1 on a problem. |
| `workshop decline <issue> --why "…"` | Drops an open suggestion, the Workshop's word for it: closes it with GitHub's own reason for closing, "not planned", and the reason as the bot's comment. Refuses an issue that is not an open suggestion (an entry, an issue without the suggestion label, a closed one). A run stopped after the comment and run again posts no second comment. |

`doctor` checks both labels: `workshop:suggestion`, which the site puts on each new suggestion, and
`workshop:entry`, the label of an entry's thread, which the bot opens. GitHub's documentation of
"Create an issue" says only that labels set by someone without push access are silently dropped
([REST API, issues](https://docs.github.com/en/rest/issues/issues#create-an-issue), read
2026-11-20). Neither it nor "Add labels to an issue"
([REST API, labels](https://docs.github.com/en/rest/issues/labels#add-labels-to-an-issue)) says
that a label that does not exist is created. So the labels must exist, and a missing one is a
problem that names it: create it on GitHub, in the repository's labels. When the site opens a
thread and GitHub drops the label, the App creates it.

`doctor` also reads every issue labelled `workshop:entry` and reports each entry that several of
them name, with their numbers. The lowest number is the entry's thread and the site ignores the
others: move what matters to the thread and remove the label from the others.

## The App

`workshop setup` opens a page on `127.0.0.1` (it answers no other Host) that posts the manifest to
GitHub: "Silo Workshop", public, Issues and Contents write and Metadata read, no webhook, sign-in callback
`<site>/api/workshop/callback`. The owner clicks "Create GitHub App", then "Install" on
`amontlabs/silo` only. The command writes the seven variables Vercel needs to
`~/.local/state/silo-workshop/vercel.env` (0600) first, then the App id, client id, client secret
and private key to the Keychain (service `silo-workshop`, through `security -i` on stdin). It never
registers a second App by accident; `--replace` registers a new one. Contents write is for the
owner's Take and Drop on the Workshop, which commit to `main` with the owner's token; an App
registered before it asked for Contents gets it in its settings, and the installation then
accepts it. This command's own installation token, like the site's, is requested with Issues and
Metadata only. The App's private key could still mint one that writes the repository's contents,
so a leaked key puts the code at risk, not only the issues: rotate it at once.

Credentials are looked up in the environment, then the Keychain (a locked one stops the command),
then `~/.config/silo-workshop/credentials.json` (0600 or refused). `WORKSHOP_GITHUB_API` and
`WORKSHOP_REPO` override the API and the repository. No credential, token or key is ever printed
or logged.

## The issues already on GitHub

Two labels: `workshop:suggestion` on a person's suggestion, and `workshop:entry` on an entry's
thread, the issue the bot opens at the first comment or 👍 on one entry of the plan. A thread is
never made by hand: an older issue someone labels by hand is found by the site only at the next
refresh of its snapshot. Label names read from GitHub are compared ignoring capitalisation. An
entry's issue ends with `<!-- silo-entry: <id> -->`. When several issues name one entry, the
lowest number is its thread (`issuesByEntry` in `model/markers.js`). The bot once wrote a comment
on an issue for each change of its entry; that comment is read as any other comment now, and the
history of an entry comes from git (`tools/plan/history`).

## Modules

- `model/`: the names and labels, the entry marker with its reader, and the issues of each entry. Plain ESM; it imports nothing outside its folder and no `node:`
  module (a test enforces it). The site's server imports it, and the site's function ships this
  folder with `package.json` and nothing else of `tools/`.
- `github/`: the REST client, the App's JWT and installation token, credentials, the Keychain,
  `setup` and `store`, `doctor`, `decline`.
- `cli/`: the commands.

## What the site does with GitHub

No push acts on GitHub. The site reads issues, comments, votes and suggestions. As the person
signed in, it writes comments, votes, votes on comments and suggestions. As the bot, it puts
`workshop:suggestion` on a new suggestion, and it opens an entry's thread, labelled
`workshop:entry`, at the first comment or 👍 on that entry. As the owner, it commits the owner's
Take and Drop to `main`. See `website/README.md`, "Storage", "Writing to an entry" and "The
owner's verdict".

# workshop

The command for the GitHub side of the Workshop. Agents and the owner use it to create the GitHub App, sync the plan to issues, read back verdicts and comments, and post as the bot. Everything it posts comes from the App's installation token, so it shows as `silo-workshop[bot]`. The storage model (labels, markers, verdicts) is described in the Workshop's server documentation.

Node 26, no dependencies. Run it from anywhere in the repository:

```sh
tools/workshop/bin/workshop --help
```

Every command takes `--json` for machine-readable output. Exit codes: 0 done, 1 failed (one plain line on stderr), 2 wrong use.

## Commands

### `setup`: create the App

```sh
tools/workshop/bin/workshop setup                      # --org amontlabs --site https://silo.amontlabs.com
```

The command opens the browser on a local page (`http://127.0.0.1:<random port>/`), which posts the App manifest to GitHub. The owner then has two clicks to do:

1. "Create GitHub App" on GitHub.
2. "Install" on the link the command prints, choosing only `amontlabs/silo`.

The manifest sets the name "Silo Workshop", Issues read and write, Metadata read, no webhook, not public, and the sign-in callback `<site>/api/workshop/callback`. GitHub sends the browser back to the local server with a one-hour code. The command exchanges the code for the App's id, client id, client secret and private key, and stores those four in the Keychain. It then writes all seven variables Vercel needs (the four, plus a new `WORKSHOP_SESSION_SECRET`, `WORKSHOP_REPO` and `WORKSHOP_SITE_ORIGIN`) to `~/.local/state/silo-workshop/vercel.env` (mode 0600, outside the repository). Each line is `NAME="value"`, quoted as in JSON, so the PEM's newlines are written as `\n`, the form dotenv-style parsers expand. The command refuses to run when credentials already exist in the Keychain, the credentials file or `vercel.env`; pass `--replace` to register a new App.

### `doctor`: check the setup

```sh
tools/workshop/bin/workshop doctor
```

Shows where each credential was found (environment, Keychain, file or missing), never a value. It also shows whether the App is installed on the repository, the installation's permissions, and whether the three kind labels and four verdict labels exist. It exits 1 when something is wrong.

### `sync`: make GitHub match the files

```sh
tools/workshop/bin/workshop sync --dry-run   # prints what it would do, writes nothing anywhere
tools/workshop/bin/workshop sync             # created 3, updated 1, unchanged 115
```

- Creates the seven labels if they are missing.
- `docs/product/features.jsonl`: for each line, `sync` finds the `workshop:entry` issue by the line's `issue` number, or by the `<!-- silo-entry: <id> -->` marker in its body, and creates the issue when there is none. The title is the entry's `name`; the body is its `what` followed by the marker. A changed title or body is updated. `sync` writes `"issue": N` into the line, either replacing the value in place or appending the field before the closing brace, so the file's field order and every other byte stay as they were. It never adds or removes a verdict label and never closes or reopens an entry. A line whose `issue` names an issue that is not that entry's stops the run.
- `docs/product/DECISIONS.md`: for each list item that does not end with ` (#N)`, `sync` creates a `workshop:decision` issue. The title is the item cut at 100 characters on a word; the body is the whole item. The issue is closed as completed, and ` (#N)` is appended to the item. Items are processed oldest first. Nothing else in the file changes.
- Each number is written to disk as soon as its issue exists, and a rerun finds issues by marker (entries) or by body (decisions). A run stopped by GitHub's rate limit (exit 1, "run again after …") therefore resumes without creating duplicates, and a second run changes nothing. Writes are paced one per second, inside GitHub's content-creation limits.

### `pull`: what GitHub has that the files do not

```sh
tools/workshop/bin/workshop pull            # report only
tools/workshop/bin/workshop pull --write    # record verdicts in features.jsonl
```

The report lists:

- Entries whose verdict on GitHub differs from the line's `verdict`. A `verdict:*` label gives the verdict; an issue closed as "not planned" without a label counts as `drop`. Each verdict shows who set it, read from the issue's events.
- Decision issues closed as completed that no line of DECISIONS.md references, with the closing comment.
- Open suggestions with their 👍 counts.
- Comments by people (not bots) on Workshop issues since the last `pull --write`. That time is kept in `~/.local/state/silo-workshop/last-pull`.

`--write` records a verdict in the line's `verdict` field only when the person who set it has write, maintain or admin permission on the repository. A verdict set by anyone else (or by a bot), or an issue that carries two verdict labels, is reported as not recorded. A verdict removed on GitHub is never cleared from the file, because a verdict in the file without a label is "carried over". `pull` never edits DECISIONS.md: the orchestrator writes a decided line itself, ending it with ` (#N)`.

### `ask`, `comment`, `built`: post as the bot

```sh
tools/workshop/bin/workshop ask --title "Which sandbox first?" --body @question.md --entry vm-runtime
echo "Built in abc1234." | tools/workshop/bin/workshop comment 42 --text -
tools/workshop/bin/workshop built 42
```

- `ask` opens an open `workshop:decision` issue. Its title can be at most 120 characters. `--entry ID` adds a line naming that entry's issue.
- `comment` comments only on an issue that carries a `workshop:` kind label; it refuses any other issue and any pull request.
- `built` closes a `workshop:entry` issue as completed and leaves its labels alone. It refuses an entry closed as not planned (dropped).

`--body` and `--text` take the text itself, `@file`, or `-` for stdin.

## Credentials

The App id and private key (and for `doctor`, the client id and secret) are looked up per name in this order:

1. Environment variables: `WORKSHOP_APP_ID`, `WORKSHOP_PRIVATE_KEY` (PEM), `WORKSHOP_CLIENT_ID`, `WORKSHOP_CLIENT_SECRET`.
2. The macOS Keychain: generic passwords with service `silo-workshop`, one item per name as the account. `setup` writes them through `security -i` on stdin, so no value appears on a command line. Values are stored base64-encoded with a `base64:` prefix, so the multi-line PEM survives. A value added by hand without the prefix is read as it is.
3. `~/.config/silo-workshop/credentials.json`: a JSON object of the same names. The file must be readable only by you (`chmod 600`); otherwise it is refused.

The tool signs the App JWT itself (RS256, issued 60 seconds in the past, valid 9 minutes). With it, the tool finds the installation on `WORKSHOP_REPO` (default `amontlabs/silo`) and requests a one-hour installation token limited to that repository, with Issues write and Metadata read. No credential, token or code is ever printed. `WORKSHOP_GITHUB_API` overrides the API address (default `https://api.github.com`).

## Tests

```sh
npm --prefix tools/workshop test
```

The tests run against a fake GitHub (`test/fake-github.js`), with a temporary HOME, a temporary copy of `docs/product` and a replaced Keychain module. They never touch the real files, the real Keychain or GitHub, and every run asserts that no secret appears in the output. `app/SiloUI/scripts/ci/scripts.sh` runs them in CI.

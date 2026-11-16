# The Workshop

The Workshop is the public page of the website at
[silo.amontlabs.com/workshop](https://silo.amontlabs.com/workshop). It shows
Silo's plan: every feature in build order. Anyone signed in with GitHub can
vote, comment and suggest a feature, and the owner decides each entry there.

This document covers the server side: the API under `/api/workshop/`, the
GitHub App it signs in with, how it is deployed and run locally, and its
security properties. The page itself is built separately against the same
contract.

## Storage

GitHub issues of `amontlabs/silo` are the only storage. There is no database,
and nothing is written at run time apart from GitHub issues.

| Label | What the issue is |
| --- | --- |
| `workshop:entry` | One line of `docs/product/features.jsonl`. The body ends with `<!-- silo-entry: <id> -->`. |
| `workshop:decision` | A question for the owner. Open: to decide. Closed as completed: decided, and the owner's closing comment is the outcome. |
| `workshop:suggestion` | A feature suggested by a visitor. |

The owner's verdict on an entry is one label: `verdict:take`, `verdict:refine`
or `verdict:later`. A dropped entry has `verdict:drop` and is closed as "not
planned". A built entry is closed as "completed". Votes are 👍 reactions,
comments are issue comments, and history is GitHub's.

## The API

All endpoints are under `/api/workshop/` on the site's own origin and answer
JSON. The shared contract (SHARED.md with its review changes) is restated
briefly here:

| Endpoint | Who | What |
| --- | --- | --- |
| `GET state` | anyone | Every issue with a Workshop label, open or closed: kind, entry id, title, state, verdict, 👍 count, comment count, author, dates. |
| `GET me` | signed in | `{ login, avatar, canDecide }`, or 401. |
| `GET issue?n=` | anyone | Body text, 👍 count, and the latest 100 comments, oldest first, as plain text; `more: true` when there are earlier ones. The same public answer for everyone. |
| `GET vote?n=` | signed in | `{ on }`: whether the person has given a 👍, read with their own token. |
| `GET login?return=` | anyone | Redirects to GitHub. `return` must be a path under `/workshop`. |
| `GET callback` | GitHub | Finishes sign-in and redirects to the return path. |
| `POST logout` | anyone | Revokes the person's whole grant for the App (all their sessions), then clears the cookies. |
| `POST comment {n, text}` | signed in | Comments as that person. |
| `POST vote {n, on}` | signed in | Adds or removes that person's 👍. |
| `POST suggest {title, text}` | signed in | Opens an issue as that person and labels it `workshop:suggestion`. Returns `{ number }`. |
| `POST verdict {n, verdict, note?}` | can decide | Sets (`take`, `refine`, `later`, `drop`) or clears (`null`) the verdict of an entry; the `verdict` key is required. `refine` needs a note; a note is posted first as the person's comment. |
| `POST decide {n, text}` | can decide | Comments on an open decision, then closes it as completed. |

Errors are `{ "error": code, "message": plain words }` with 400 (bad input),
401 (not signed in, or the session is gone), 403 (not allowed, or a request
from another site), 404 (not a Workshop issue), 405, 413 (body over 64 KB),
429 (GitHub's rate limit or one of ours), 502 (GitHub failed or took over 8
seconds) and 503 `not_configured` (missing or invalid App settings). A
suggestion whose issue was created but could not be labelled answers 502 with
`"error": "github"` and the issue's `number`. `login` and `callback` never
answer JSON: any failure redirects to `/workshop?signin=failed`; cancelling on
GitHub returns to the page.

Comments, notes and decisions are limited to 5,000 characters, titles to 120
characters on one line (no control characters or line breaks). Unicode
direction controls (U+202A–U+202E, U+2066–U+2069) are removed from titles and
text.

### Caching and GitHub's limits

- `state` is cached in memory for 60 seconds and sent with
  `Cache-Control: public, s-maxage=60, stale-while-revalidate=300`.
- `issue` is the same for everyone: cached in memory for 30 seconds and sent
  with `public, s-maxage=30, stale-while-revalidate=300`. It answers only for
  numbers in the cached `state`, so other numbers cost no GitHub call.
- Everything else, and every response that sets a cookie, is
  `private, no-store`.
- Calls with the installation token outside the `state` refresh are limited to
  60 a minute per function instance. Reading an issue takes three. When the
  budget is spent, or while GitHub has asked us to wait, `issue` answers from
  its cached copy (marked `"stale": true`) if it is under 10 minutes old,
  otherwise 429.
- When GitHub rate-limits the App (including a secondary limit: a 403 or 429
  with `retry-after`, or whose message mentions a rate limit), the server stops
  calling GitHub with the installation token until the time GitHub gave (at
  most an hour). `state` serves its last good copy, marked `"stale": true`, for
  up to 10 minutes after it was fetched, then answers 429. The same stale rule
  covers GitHub server errors and timeouts.
- Each signed-in person may make 30 writes a minute per instance; more answer
  429.
- Every GitHub call has an 8-second limit.

## Code

| Path | What |
| --- | --- |
| `website/server/config.mjs` | Settings from the environment. |
| `website/server/github.mjs` | GitHub REST client with pagination and an 8-second limit per call; errors carry a status and the identity used, never a body. |
| `website/server/app-auth.mjs` | App JWT (RS256, Node's `crypto`) and the cached installation token. |
| `website/server/session.mjs` | Sealed cookies (AES-256-GCM). |
| `website/server/workshop.mjs` | Every endpoint, independent of the HTTP server. |
| `website/server/node.mjs` | Adapter for Node's `req`/`res`, used by Vercel and the local server. |
| `website/api/workshop.cjs` | Vercel's entry point. |
| `website/scripts/prepare-vercel.mjs` | Writes the Build Output (static site, function, routes). |
| `website/scripts/workshop-dev.mjs` | Local server for the site and the API. |
| `website/scripts/fake-github.mjs` | Fake GitHub used by the tests and by `--fake`. |
| `website/scripts/workshop-server.test.mjs` | The server's tests, run by the website's `npm test`. |

The server uses only Node's standard library (`crypto`, `fetch`, `http`).

## The GitHub App

Create it under the `amontlabs` organization (Settings → Developer settings →
GitHub Apps → New GitHub App):

| Setting | Value |
| --- | --- |
| Name | Silo Workshop (posts show as `silo-workshop[bot]`) |
| Homepage URL | `https://silo.amontlabs.com/workshop` |
| Callback URL | `https://silo.amontlabs.com/api/workshop/callback` (the only one) |
| Expire user authorization tokens | On |
| Request user authorization (OAuth) during installation | Off |
| Enable Device Flow | Off |
| Webhook | Inactive |
| Repository permissions | Issues: Read and write. Metadata: Read-only. Nothing else. |
| Organization and account permissions | None |
| Where can this GitHub App be installed | Only on this account |

Then generate a private key and a client secret, and install the App on
`amontlabs/silo` only ("Only select repositories"). Create the seven labels in
the repository: `workshop:entry`, `workshop:decision`, `workshop:suggestion`,
`verdict:take`, `verdict:refine`, `verdict:later`, `verdict:drop`.

For local work against the real GitHub, create a second App the same way with
the callback `http://127.0.0.1:4174/api/workshop/callback`, installed on a test
repository. Do not add a local callback to the production App.

## Environment variables

Set them in the Vercel project (Settings → Environment Variables) for the
**Production** environment only, marked Sensitive. Preview deployments are
built from any branch and must not receive the App's secrets; without them
the API answers 503 and the page shows the plan without live data.

| Variable | Value |
| --- | --- |
| `WORKSHOP_APP_ID` | The App's numeric ID. |
| `WORKSHOP_CLIENT_ID` | The App's client ID. |
| `WORKSHOP_CLIENT_SECRET` | A client secret of the App. |
| `WORKSHOP_PRIVATE_KEY` | The App's private key, PEM. Newlines may be written as `\n`. |
| `WORKSHOP_SESSION_SECRET` | Exactly 32 random bytes in standard base64: `openssl rand -base64 32`. Changing it signs everyone out. |
| `WORKSHOP_REPO` | Optional, default `amontlabs/silo`. |
| `WORKSHOP_SITE_ORIGIN` | Optional, default `https://silo.amontlabs.com`: a bare origin, no path. Writes are accepted only from this origin, and cookies are `__Host-` and `Secure` only when it is `https`. |

A missing or invalid value (a secret that is not 32 bytes of base64, a key that is not an RSA private key, a malformed repository or origin) leaves the API answering 503 `not_configured`. The response never says which value is wrong; the server log names the variable, never its value.


## Deployment

`npm --prefix website run build:vercel` builds the site and runs
`prepare-vercel.mjs`, which writes `.vercel/output/` at the repository root
following Vercel's
[Build Output API v3](https://vercel.com/docs/build-output-api/configuration):

- `static/`: the built site.
- `functions/api/workshop/state.func/`: one Node.js function
  (`.vc-config.json`: runtime `nodejs24.x`, handler `api/workshop.cjs`,
  launcher `Nodejs`), holding the entry point and `server/`. The other
  endpoints (`me.func`, `issue.func`, `vote.func`, …) are symlinks to it, so
  Vercel creates one function served at every `/api/workshop/<endpoint>` path,
  as the [primitives reference](https://vercel.com/docs/build-output-api/primitives#functions)
  allows. An unknown endpoint is Vercel's 404.
- `config.json`: `version: 3` and two routes. The first,
  `{ "src": "^/workshop/?$", "dest": "/workshop/index.html" }`, serves the
  Workshop page at both `/workshop` and `/workshop/`; the second hands every
  other path to the filesystem.

The script fails when the build has no Workshop page (`workshop/index.html`, or
`workshop.html`). `--out <dir>` writes the output elsewhere. Both
`/.vercel` and `website/.vercel` are ignored by Git.

The Vercel project's root directory is the repository root, where Vercel reads
`.vercel/output` after the build. The one project setting this needs is the
build command: `npm --prefix website run build:vercel` (it was
`npm --prefix website run build`, which produces only the static site in
`website/dist`).

## Running locally

```sh
npm --prefix website run build
npm --prefix website run workshop:dev -- --fake
```

This serves `website/dist` and the API at `http://127.0.0.1:4174`
(`--port` to change it), with the Workshop page at `/workshop` and
`/workshop/`. `--fake` starts a fake GitHub on another local port, seeded with
an entry for every line of `docs/product/features.jsonl`, two decisions (one
open), a suggestion, comments, 👍 votes, an ordinary issue and a pull request.
Sign-in goes through the fake's page, which offers two people: `owner-fake`
(admin, can decide) and `visitor-fake` (read only). Everything lives in memory
and resets on restart.

Without `--fake`, the server uses GitHub with the `WORKSHOP_*` variables of the
environment (a development App, see above); its origin is always
`http://127.0.0.1:<port>`, so cookies are not `Secure` and have no `__Host-`
prefix. Without the variables, the API answers 503.

The tests run with the website's tests: `npm --prefix website test`. The fake
GitHub answers like GitHub where it matters to the server: expiring user tokens
with PKCE, a single-use refresh token, grant revocation, the permission
endpoint answering 200 with `none` for a non-collaborator, primary and
secondary rate limits, locked issues, and a hook that delays or fails any call.

## Security properties and their limits

**Session.** One cookie, `__Host-workshop-session` (`Path=/`, no `Domain`,
`HttpOnly`, `Secure`, `SameSite=Lax`, 30 days). It holds the person's GitHub
id, login, avatar, user token, refresh token and their expiries, sealed with
AES-256-GCM under a key derived (HKDF-SHA256) from `WORKSHOP_SESSION_SECRET`
for this purpose only. A changed byte, another key or a value sealed for the
sign-in cookie does not open. The session ends 30 days after sign-in. The user
token is refreshed when under five minutes remain. The session is cleared only
when GitHub says the refresh token is invalid, or when a call with the person's
token answers 401; any other failure answers 502 and keeps the cookie. A
readable marker cookie, `workshop-hint=1` (no secret, same lifetime, not
`HttpOnly`), tells the page to ask `me`; it is set at sign-in and cleared with
the session.

*Limits:* there is no server-side session list. `me` reads only the cookie, so
after a sign-out elsewhere it keeps answering until the next call made with
the person's token is refused (401, and the session is cleared). A stolen
cookie works until it expires, the person signs out, or the secret is changed.

**Sign-out.** Sign-out revokes the person's whole grant for the App (all their
sessions), then clears the cookies.

**Sign-in.** `login` sets a 10-minute sealed `__Host-workshop-signin` cookie
holding a random `state`, a PKCE verifier (S256) and the return path.
`callback` clears it, compares `state` in constant time, and exchanges the code
with the verifier and `repository_id`, so the user token works only on the
Workshop repository. The redirect URI is fixed to the configured origin. A
token without expiry (the App setting off) is refused. `return` is accepted
only as a path under `/workshop` (no scheme, no `//`, no backslash, no control
characters; dot segments are resolved and checked).

**Cross-site requests.** Every `POST` needs `Origin` equal to
`WORKSHOP_SITE_ORIGIN` and the header `X-Workshop: 1`, which a cross-site page
cannot send without a CORS preflight that the server never approves (it sends
no CORS headers). The cookies are `SameSite=Lax`, so cross-site subrequests do
not carry them. Writes from preview deployments are refused because their
origin differs.

**Identities and scope.** The App can only read and write issues of the one
repository, and the installation token is requested for that repository with
those two permissions. The installation token serves only the anonymous cached
reads (`state`, `issue`), the permission check and labelling a suggestion
(which a visitor cannot do). Everything else done for a signed-in person,
including reading the issue before a write, uses that person's token, so
GitHub's own permissions apply (a locked issue refuses a visitor's comment)
and GitHub records the person as the author. Every write first reads the issue
and refuses with 404 unless it carries a Workshop label and is not a pull
request; `verdict` works only on entries and `decide` only on open decisions.
A verdict adds and removes only `verdict:*` labels, one at a time, so labels
changed by someone else are kept. A 401 on the installation token is a 502 and
drops the cached token; it never signs anyone out. *Limit:* if labelling a
suggestion fails twice, the new issue stays on GitHub outside the Workshop, and
the answer carries its number.

**Deciding.** `canDecide` is GitHub's answer: the person's permission on the
repository (`write`, `maintain` or `admin`), read with the installation token
and cached five minutes. `verdict` and `decide` confirm with the person's token
that the GitHub account id matches the session, then read the permission again
without the cache, so a renamed login cannot inherit another account's
permission. A cached "cannot decide" refuses at once without calling GitHub.

**Secrets in output.** Logs are one JSON line per request with the endpoint,
the status and GitHub's status only. Errors sent to the browser are fixed
sentences; nothing from a GitHub response body is passed on. No token, code,
cookie, client secret or private key is logged or returned; a test asserts
this on every response and log line of a session that includes failures.

**Rate limits.** The budgets and caches are per function instance, in memory;
several instances together can make more calls than one.

## Not verified until deployed

Everything above was run against the fake GitHub and in Node, not against
GitHub or Vercel. These depend on the real services:

- the token exchange body format: the server sends JSON with
  `Accept: application/json`, including `code_verifier` and `repository_id`;
- whether a refreshed user token stays limited to the repository;
- a visitor removing their own 👍 with their user token;
- the collaborator permission endpoint with only the Metadata read permission;
- that Vercel follows the symlinked functions, and loads the `.cjs` handler
  with `req.url` as the original path;
- that Vercel's Git build uses `.vercel/output` at the repository root.

## Why built this way

- **giscus** stores comments in GitHub Discussions and has no place for a
  verdict, a vote that the owner's page counts, or a decision's state.
- **utterances** signs people in with an OAuth App that needs the broad
  `public_repo` scope, write access to every public repository of the person.
  A GitHub App's user token is limited to the App's permissions and, here, to
  one repository.
- **A database of our own** is something to run, secure and back up. Issues
  keep the history, the authorship and the moderation tools on GitHub.
- **Verdicts as commits** to `docs/product/features.jsonl` would put a commit
  on `main` for every click, which interrupts a merge in progress. The file
  keeps carried-over verdicts; new ones are labels.

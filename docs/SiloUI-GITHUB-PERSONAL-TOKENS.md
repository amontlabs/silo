# Personal GitHub tokens

2026-09-14. Implemented as an alternate connection alongside the existing GitHub
App OAuth flow. Changed on 2026-11-10: a personal token is now a secret (see
[Secrets](SiloUI-SECRETS.md#github-tokens)), you can keep several, and each computer chooses
one by name.

## Behavior

- A token is a secret of kind `github-token`, added on the Secrets page with a name and the
  token. Silo validates the token with GitHub's read-only `/user` endpoint before saving it
  and shows the account it belongs to. Validation does not require any existing repositories.
  Classic and fine-grained tokens are accepted.
- A computer's GitHub access is signed-in access (OAuth) or one named token. On the GitHub
  page each computer offers Use GitHub OAuth and Use token; Use token is enabled while at least
  one token is connected and shows a list of the tokens by name and account. A computer
  stores the chosen secret's id as `tokenSecretId` beside `authenticationMethod: "token"`.
  Different computers can use different tokens, and a computer never receives a token it
  was not given.
- Existing computer policies default to OAuth. Losing a connection preserves the selected
  method and token; it never falls back to the other method or to another token.
- Token access uses the token's full permissions. OAuth repository and read/write controls
  are hidden for a computer using a token, and their selections are preserved when switching
  back. The page says the computer can do everything the token allows.
- Replacing or removing a token is independent of OAuth login. Removing a token detaches it
  locally; it does not revoke the user's token on GitHub or remove the OAuth connection.
- The worker validates every saved token at startup and every five minutes. A check has
  three outcomes: GitHub accepts the token; GitHub rejects it (a 401, or a 403 whose body
  says the credentials are bad, never a rate limit); or GitHub cannot say (a timeout, a
  server error, a rate limit, no network). Only a rejection marks the token disconnected and
  detaches the computers using it; the Secrets page offers replacement without a manual test
  button. When GitHub cannot say, the token keeps its previous status and is checked again
  after 30 seconds. A token Silo has not been able to check since it started shows as waiting
  and its computers stay detached until GitHub answers. Saving a token that GitHub cannot be
  asked about fails with "Silo could not reach GitHub to check this token. Try again." and
  saves nothing. A check records its result only if the token it checked is still the saved
  one: a token replaced or removed while GitHub was being asked keeps what the save recorded.
  Other tokens are not affected. Individual GitHub API permission errors remain GitHub's
  responsibility.
- A computer whose chosen token secret was removed keeps naming it and has no GitHub access
  (it never falls back). The GitHub state marks that computer's policy `tokenMissing`, and
  the GitHub page says the token was removed and asks for another.
- A computer's agent that asks for token access must name the token secret by id. The owner's
  approval says which token it is and that a personal token gives its full permissions.

## Earlier versions

Earlier versions kept one token for the whole app in the credential store entry
`personal-token`, with each computer either using it or not. At engine start Silo moves it
once: the saved token becomes a `github-token` secret named `github-token` (`github-token-2`
and so on if that name is taken), every computer in token mode is pointed at it, and the old
entry is deleted only after both are written. Each step can be repeated, so an interruption
anywhere is finished at the next start. A removal that was pending at that time is honoured:
the token is deleted rather than moved.

`github.personalToken.save` and `github.personalToken.remove` are superseded but still work as
a thin layer over the secret named `github-token`. After a client has used either in the
current session, a configuration save from that client that selects token mode without naming
a token chooses that secret when it exists. Any other save that selects token mode without
naming a token fails with "Choose a connected GitHub token before selecting it." `personalToken` in the
GitHub state describes the same secret.

### Going back to an older Silo

Going back to an older Silo after a token secret exists is not supported. The older version
cannot read the secrets file once it holds a `github-token` secret, and the old single token
entry in the credential store is deleted when the token moves, so the older version would have
neither.

## Credential boundaries

The password input is cleared at submission. Its value crosses the secret save command
once, never a general configuration save. No returned snapshot, durable JSON document,
activity, or error includes the token.

The native backend stores the value in the secrets vault of the OS credential store, like
any secret. Public snapshots read only a process-local status observation and cannot
trigger credential-store dialogs. The removal tombstone of a secret is persisted before
detachment, so a failed credential deletion cannot silently reattach the token after
relaunch.

The runtime uses an explicit version-2 personal-token profile, independent of
OAuth owner/repository grants. The proxy replaces the exact placeholder only in
GitHub Authorization headers, including Git Basic authentication, under verified
TLS. Bodies, queries and unrelated headers are not substituted. Unsupported
hosts and malformed/mixed profiles fail closed. Tokens do not enter OAuth child
issuance or revocation ledgers. Host Push uses the token the computer chose.

The updated CLI checks the running computer's token-profile capability before sending
a personal-token update. A computer running an older proxy must be restarted once;
Silo does not report a successful attachment to an incompatible proxy. Changing
connections in a compatible running computer uses the existing live secret update and
connection invalidation mechanism.

## Verification

Synthetic tests cover independent selector availability, default OAuth behavior,
preserving an unavailable selection, token input clearing, redacted failures,
public-state parsing, empty-account validation and native command permissions.
Patched proxy tests cover REST creation/deletion, GraphQL creation, Git writes,
Basic authentication, and header-only substitution. Existing OAuth regression
checks remain applicable.

Commands used:

```sh
npm --prefix app/SiloUI run typecheck
npm --prefix app/SiloUI run lint
npm --prefix app/SiloUI test -- src/features/github src/features/application/application-app.test.tsx src/desktop/production-source.test.ts
npm --prefix app/SiloUI run test:release
# Native GitHub tests use explicit synthetic build configuration, per the release guide.
cargo test --offline --manifest-path app/SiloUI/src-tauri/Cargo.toml github
```

The pinned runtime patch is also applied to a fresh cached source archive under
`app/SiloUI/src-tauri/target/verification/github-token-runtime/` and checked with
Rust 1.94.0. Its network tests run against the actual patched source; the CLI,
SDK and runtime compile with the shipped net/ssh feature combination.
Browser layout/interaction checks use synthetic OAuth and token accounts.
No real token, live computer or packaged Silo bundle was exercised. Live repository
creation and secure-store integration require a subsequent authorized live test.

## Primary references

GitHub's [personal-token guide](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)
documents classic and fine-grained tokens, one resource owner per fine-grained
token, permission differences and organization restrictions. Full token access
means all authority granted to that token, not permissions beyond the user or
GitHub's authorization.

The [repository creation endpoint](https://docs.github.com/en/rest/repos/repos#create-a-repository-for-the-authenticated-user)
documents the permissions needed to create the first repository. Silo forwards
that request using the selected token without deriving grants from existing
repositories.

## Verification on 2026-11-10 (tokens as secrets)

Synthetic tests, with the test credential stores and no real token, cover: the move of the
saved token (fresh start, repeated, interrupted at each step, name taken, removal pending);
several tokens on different computers each producing their own version-2 profile; one token
rejected by GitHub disconnecting without touching the others; host push choosing the right
token and failing rather than using another; a token secret never appearing as an
environment placeholder; the API types; and the Secrets and GitHub pages. Not exercised
here: a real computer, the real Keychain, a real GitHub account, and the macOS proxy path
(`macos_proxy_policy`) with a live gateway. A live check should add two tokens for two
accounts, give each to a different running computer, confirm each computer's `git ls-remote`
and `gh api user` answer as its own account, revoke one token on GitHub and confirm that
computer loses access within five minutes while the other keeps working, then restart the
engine with the old single token saved and confirm it appears as `github-token`.

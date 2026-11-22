# Is a subscription login supported as fully as an API key

What is known about subscriptions and substitution is in the notes of `logins`, and whether
substitution works with a subscription sign-in is `subscription-substitution`.

## For the owner

An agent's part, 22 November 2026. You make a fresh computer. An API key works in it at once,
because Silo holds it as a secret. With a subscription you sign in again inside it, because Silo
holds no subscription sign-in.

The options:

- Say it as it is (recommended). Silo says that a subscription works in every computer through the
  vendor's own sign-in, and that Silo does not hold it. It costs the claim that a subscription is
  supported as fully as an API key, and you sign in again in each fresh computer.
- Promise nothing yet. Silo says nothing about subscriptions until logins are designed as a whole.
  It costs anyone with a subscription knowing whether theirs works.

Recommended: say it as it is. It matches what the plan proposes, it fits the vendor's terms as they
were read, and it designs nothing you asked to leave for later.

## Found by an audit, 22 November 2026

An agent's part. This says a subscription login is supported as fully as an API key, but
`program-and-sign-in-untouched` says Silo holds and injects no subscription sign-in. In a fresh
computer an API key works at once, while a subscription login is a file in main, so the user signs
in again. The audit proposes: "A subscription login works in every computer, through the vendor's
own sign-in inside it. Silo reads its traffic as it reads an API key's. It is not held outside the
computer as a secret is." `subscription-substitution` is then dropped.

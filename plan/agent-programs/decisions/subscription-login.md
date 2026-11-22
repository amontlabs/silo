# Is a subscription login supported as fully as an API key

What is known about subscriptions and substitution is in the notes of `logins`, and whether
substitution works with a subscription sign-in is `subscription-substitution`.

## Found by an audit, 22 November 2026

An agent's part. This says a subscription login is supported as fully as an API key, but
`program-and-sign-in-untouched` says Silo holds and injects no subscription sign-in. In a fresh
computer an API key works at once, while a subscription login is a file in main, so the user signs
in again. The audit proposes: "A subscription login works in every computer, through the vendor's
own sign-in inside it. Silo reads its traffic as it reads an API key's. It is not held outside the
computer as a secret is." `subscription-substitution` is then dropped.

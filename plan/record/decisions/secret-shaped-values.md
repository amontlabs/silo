# A value shaped like a secret that Silo was not given

## The choice

An agent's part, 25 November 2026. An agent runs `cat .env` in main. The file holds an AWS access
key that you put there yourself, not through Silo's prompt, so by `secret-entry` it is not a
secret. The agent sends the file's text to the model, and the record keeps that request. The key
now sits in the record for as long as the record keeps the message.

The options:

- Replaced by its shape (recommended). The recorder looks for the formats of common keys and puts
  `[secret-like:<format>]` in their place before the line is hashed. The line still shows that a
  key of that kind went to the model, which is what a morning review should catch. It costs a
  record that is no longer byte for byte what was sent, and a few misses: a key with no prefix (an
  AWS secret key, a database password) is caught only next to a name such as `aws_secret` or
  `password`.
- Kept as it is. The record holds what the agent saw. It costs a copy of every key an agent ever
  read, in a store that is kept longer than the file and that a review or a shared receipt may
  show.
- Kept, and the line marked. The value stays, and the line says it holds a key, so a view can hide
  it. It costs the same copy on disk as keeping it.

Recommended: replaced by its shape. It costs little, and the line keeps the one fact a person
needs: which kind of key left, and to whom.

## What was measured

From [the redaction research](../../../research/2026-11-25-secret-redaction/README.md).

- On 816 samples (17 key formats, 16 places where they appear), the shape rules took the whole
  value in 536 and missed 244. The misses are values with no prefix and no name beside them, and
  values in hex.
- On 40 MB of this repository's text and code, the rules fired 54 times. The 25 hits of the
  format rules were all test keys in test or vendored code. The 29 others came from the rules that
  read a name: test values, lines of `/etc/passwd` and package versions under a key named
  `passwd`. Without the check that the value looks random, those rules fired 789 times.
- One pass of Hyperscan over all rules ran at 1,030 MB/s on 8.5 MB of code.

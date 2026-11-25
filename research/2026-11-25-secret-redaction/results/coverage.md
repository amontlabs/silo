# Redaction coverage, seed 25112026

816 samples: 17 formats, 3 values each, 16 places. A cell is the samples held (no character of the value left); partial ones left some.

## By place

| | #386 exact | forms | shapes | shapes+decode | forms+shapes |
|---|---|---|---|---|---|
| prompt | 51/51 | 51/51 | 36/51 (6 partial) | 36/51 (6 partial) | 51/51 |
| tool output | 51/51 | 51/51 | 42/51 (8 partial) | 42/51 (8 partial) | 51/51 |
| file in a request body | 45/51 | 51/51 | 42/51 | 42/51 | 51/51 |
| JSON escaped the Go way | 45/51 | 51/51 | 42/51 | 42/51 | 51/51 |
| tool arguments (JSON in JSON) | 45/51 | 51/51 | 48/51 | 48/51 | 51/51 |
| URL query, percent-encoded | 36/51 | 51/51 | 51/51 | 51/51 | 51/51 |
| URL password, percent-encoded | 36/51 | 51/51 | 45/51 | 51/51 | 51/51 |
| Basic authorization | 0/51 | 51/51 | 51/51 | 51/51 | 51/51 |
| base64 inside a file | 0/51 | 51/51 | 0/51 | 44/51 | 51/51 |
| hex | 0/51 | 51/51 | 0/51 | 0/51 | 51/51 |
| shell trace | 39/51 | 51/51 | 42/51 | 42/51 | 51/51 |
| split across two lines | 0/51 | 19/51 (30 partial) | 0/51 (16 partial) | 0/51 (16 partial) | 19/51 (30 partial) |
| CRLF line ends | 45/51 | 51/51 | 42/51 | 42/51 | 51/51 |
| rewrapped at 76 | 27/51 | 51/51 | 21/51 (3 partial) | 21/51 (3 partial) | 51/51 |
| screen of 80 columns | 24/51 | 51/51 | 24/51 (3 partial) | 24/51 (3 partial) | 51/51 |
| raw model stream | 0/51 | 0/51 | 0/51 | 0/51 | 0/51 |

## By format

| | #386 exact | forms | shapes | shapes+decode | forms+shapes |
|---|---|---|---|---|---|
| aws-access-key-id | 33/48 | 42/48 (3 partial) | 36/48 | 39/48 | 42/48 (3 partial) |
| aws-secret-access-key | 27/48 | 42/48 (3 partial) | 15/48 | 16/48 | 42/48 (3 partial) |
| github-classic | 33/48 | 42/48 (3 partial) | 36/48 | 39/48 | 42/48 (3 partial) |
| github-fine-grained | 27/48 | 43/48 (2 partial) | 30/48 | 33/48 | 43/48 (2 partial) |
| gitlab | 33/48 | 42/48 (1 partial) | 36/48 | 39/48 | 42/48 (1 partial) |
| stripe-live | 27/48 | 45/48 | 30/48 (9 partial) | 33/48 (9 partial) | 45/48 |
| openai-project | 27/48 | 45/48 | 30/48 (2 partial) | 33/48 (2 partial) | 45/48 |
| anthropic | 27/48 | 43/48 (2 partial) | 30/48 (2 partial) | 33/48 (2 partial) | 43/48 (2 partial) |
| slack-bot | 33/48 | 45/48 | 36/48 (3 partial) | 39/48 (3 partial) | 45/48 |
| google-api | 33/48 | 43/48 (2 partial) | 36/48 | 39/48 | 43/48 (2 partial) |
| npm | 33/48 | 43/48 (2 partial) | 36/48 | 39/48 | 43/48 (2 partial) |
| sendgrid | 30/48 | 43/48 (2 partial) | 33/48 | 36/48 | 43/48 (2 partial) |
| jwt | 27/48 | 44/48 (1 partial) | 30/48 (1 partial) | 33/48 (1 partial) | 44/48 (1 partial) |
| azure-storage-key | 18/48 | 42/48 (3 partial) | 15/48 | 16/48 | 42/48 (3 partial) |
| database-password | 24/48 | 42/48 (3 partial) | 9/48 (2 partial) | 9/48 (2 partial) | 42/48 (3 partial) |
| ssh-ed25519-private | 6/48 | 44/48 (1 partial) | 24/48 (8 partial) | 30/48 (8 partial) | 44/48 (1 partial) |
| rsa-private-pkcs8 | 6/48 | 43/48 (2 partial) | 24/48 (9 partial) | 30/48 (9 partial) | 43/48 (2 partial) |

## All

- #386 exact: 444 held, 0 partial, 372 missed of 816
- forms: 733 held, 30 partial, 53 missed of 816
- shapes: 486 held, 36 partial, 294 missed of 816
- shapes+decode: 536 held, 36 partial, 244 missed of 816
- forms+shapes: 733 held, 30 partial, 53 missed of 816

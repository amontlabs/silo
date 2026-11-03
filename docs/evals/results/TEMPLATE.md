# Isolation eval results, YYYY-MM-DD

This is the layout `silo_eval.py` writes to `results/<date>-<commit>.md`. Copy it
only to record a run by hand; otherwise publish the generated file with its `.json`.

- Eval commit: `<full commit of the checkout that ran the evals>`
- Silo app: `<version (build)>` from `/Applications/Silo.app`
- Engine protocol: 7
- Host: macOS `<version>` on arm64 (`<Mac model>`)
- Computer under test: `silo-eval-a`; peer: `silo-eval-b`
- Totals: `<n> pass, <n> known-gap, ...`

| Scenario | Expected | Verdict | Summary |
| --- | --- | --- | --- |
| `host-files`: Guest reads the Mac's files | pass | | |
| `host-network`: Guest reaches the Mac's loopback, LAN address or metadata address | pass | | |
| `computer-to-computer`: Guest reaches a service in another computer | pass | | |
| `secret-in-guest`: Secret value visible inside the guest | pass | | |
| `secret-other-hosts`: Secret sent to a host outside its allowed list, or over plain HTTP | pass | | |
| `secret-reflected`: Allowed host reflects the secret back to the guest | known-gap | | |
| `open-egress`: Guest posts workspace data to an arbitrary public host | known-gap | | |
| `github-scope`: Guest pushes to a read-only or ungranted repository | pass | | |
| `engine-socket`: Another macOS user drives the engine | pass | | |
| `same-user-client`: Any same-user process drives the engine | known-gap | | |

⚠️ marks a verdict that differs from the expected one.

## Details

One section per scenario: the boundary, the verdict and duration, notes, and the
evidence as JSON (guest mounts, probe targets and replies, HTTP status codes, push
exit codes). Every `fail`, `inconclusive` or `error` gets a sentence below the
table explaining what happened and, if known, the fix or follow-up issue.

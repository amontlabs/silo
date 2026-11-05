# Record which secrets a sandbox used (draft upstream PR)

## Summary

The secret handler in `microsandbox-network` replaces placeholders with real values for allowed
hosts and blocks them for other hosts, but it leaves no per-sandbox record of either. A host
application cannot answer "which secrets did this sandbox use, for which hosts?" without parsing
`runtime.log` warnings, and the `block` action logs nothing at all.

## What changed

- New `secrets::usage` module: `set_usage_log(path)` and an append-only JSON-lines file,
  `secret-usage.jsonl`, capped at 1 MiB with one `.1` generation.
- The sandbox runner calls `set_usage_log(<log_dir>/secret-usage.jsonl)` next to `runtime.log`.
- `SecretsHandler` records one `used` line for each secret substituted in a request's headers
  or request line (HTTP/1 and HTTP/2), and one `blocked` line for each violation, including
  the `block` action that logs nothing today.
- Without a call to `set_usage_log` (embedders that do not opt in) nothing is written.

Line shape: `{"v":1,"at":<epoch ms>,"kind":"used|blocked","secret":"NAME","host":"…","method":"…","reason":"host_not_allowed"}`.
A line never holds a secret value or a placeholder; names, hosts and methods are JSON-escaped.

## Why

Hosts that assign credentials to sandboxes need to show their users what was used. A file in the
sandbox's own log directory needs no new channel and survives the sandbox process.

## Known gaps

- Body substitution is not recorded yet; only header, basic-auth and query substitution are.
- One line per request, not per connection; very chatty clients grow the file until it rotates.
- The reason is a fixed string today; the violation location could be added.

## Test plan

- `usage` unit tests: line format, escaping, request-line method, rotation at the cap.
- A handler test sends one allowed and one disallowed request and reads the file: one `used` and
  one `blocked` line with host and method, and neither the value nor the placeholder anywhere.
- `cargo test -p microsandbox-network --lib` (624 tests on the unpatched tree plus these).

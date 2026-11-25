# How is the code that reads what a computer sends checked

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- The probe's messages, model traffic, extension manifests and recipes all come from places an
  agent controls. Which fuzzer works well on the Mac is still to find out.
- Guillotine fuzzes its bytecode analysis for crashes and hangs.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Rejection paths are tested.** Test that verification rejects tampered, truncated and missing
  input, not only that it accepts good input.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/trie/REVIEW_SUMMARY.md?plain=1#L151))

## Challenged, 25 November 2026

An agent's part. #352 challenge this answer on one case: a filesystem reader that reads a different tree than macOS does not crash, so fuzzing for crashes does not find it. The replacement is `parsers-fuzzed-and-compared`, which names this decision in `replaces`, for polarzero to take or drop. Until then this answer stays in force. The reasons are in `research/2026-11-25-challenged-decisions/README.md`.

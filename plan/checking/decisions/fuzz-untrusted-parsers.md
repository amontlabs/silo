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

## The drafts' parsers, fuzzed and compared

Agent, 25 November 2026, from `research/2026-11-25-untrusted-parsers/` (#401's folder). Fourteen
parsers of eight drafts were fuzzed, coverage-guided, and each reader of a format another program
also reads was compared with that program.

- The worst findings were different readings with no crash. A path spelled `//v1/messages` or
  `/v1/./messages`, or a header after a bare LF, is read one way by the spend boundary and another
  by Anthropic's host, and the model call leaves uncounted. A directory entry of type 0xDE is
  listed by Linux and skipped by #381's ext4 reader. This is the case `parsers-fuzzed-and-compared`
  (#376) gives, measured.
- Bounded memory per input has to count what the input unpacks to. 64 KiB of zstd in a request
  took 5.4 GB of the spend boundary's memory, with no crash for a fuzzer to report.
- A reader that refuses must refuse, never raise: six inputs crash #381's ext4 reader, and each
  stops the whole comparison instead of writing a gap.
- Zig 0.15 code can be fuzzed by libFuzzer on Linux: `zig build-obj -ffuzz` and one call that
  registers the coverage counters (`zig/sancov_init.c` there). Which fuzzer works on the Mac is
  still open.

## The disk reader, fuzzed

Agent, 25 November 2026, from `research/2026-11-25-main-disk-from-outside/`. This answer's fuzzing
found 10 faults in #390's APFS content reader in 87,847 inputs, and a read past the end of the
input in Apple's open-source lzfse decoder. The reader must also read what macOS reads: a
corrupted LZVN block decodes to wrong bytes of the right size, which no crash shows. That is the
case for the sentence `parsers-fuzzed-and-compared` adds (#376).

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

## The disk reader, fuzzed

Agent, 25 November 2026, from `research/2026-11-25-main-disk-from-outside/`. This answer's fuzzing
found 10 faults in #390's APFS content reader in 87,847 inputs, and a read past the end of the
input in Apple's open-source lzfse decoder. The reader must also read what macOS reads: a
corrupted LZVN block decodes to wrong bytes of the right size, which no crash shows. That is the
case for the sentence `parsers-fuzzed-and-compared` adds (#376).

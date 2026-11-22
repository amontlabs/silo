# Where does a check's expected value come from

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- guillotine-mini's bug hunt found tests whose expected values had been copied from the code's own
  output, which locked the bug in.

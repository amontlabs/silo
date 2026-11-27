# What does the check refuse in the engine's code

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- Zig has no private fields, so "a type only validation builds" holds only because this check
  refuses a literal of that type anywhere else.
- What `engine-states-and-failures` refuses (an `else` over states or failures, `catch
  unreachable`, a `catch` that turns every failure into one value) stays as it is. This entry adds
  what Guillotine's code shows is needed besides.
- `unreachable` and `std.debug.assert` do not stop in Zig's ReleaseFast and ReleaseSmall builds:
  the optimizer assumes they never happen. Guillotine's call path builds its failure result with
  `catch unreachable`
  ([evm.zig](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/evm.zig#L419)).
- Guillotine bans `catch {}`, placeholders and commented-out tests in the text of its agent file.
  Its code still holds 26 `catch {}`, about 116 `catch unreachable` outside tests, and 40 of 81 test
  imports commented out. That is the case for `how-the-engine-is-built`: a rule in a prompt does not
  hold, a check does.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Pin the exact Zig version.** Pin the exact compiler version, upgrade it in a change of its own,
  and write the removed and the new form of each changed API side by side. Silo's trial ran on Zig
  0.17.1. Guillotine pins 0.15.1 and keeps a note of the API changes agents get wrong.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/CONTRIBUTING.md?plain=1#L179))

## The alternatives, and the check (27 November 2026)

An agent's part. A second agent, as judge, read this entry on its own and found it is about how
Silo is built, so agents may take it once the entries it needs are taken (`owners-part-on-engine`).

- Review only. It costs every file after the first, once the reviewer tires.
- The compiler only. Zig refuses none of these.
- Check: one planted violation per rule, which the check must refuse (`check-must-catch-a-fault`).
- Found by the judge: "wrapping arithmetic on a value that came from an input" cannot be checked by reading the source. A rule that can be: any wrapping operator in the core outside one named file.

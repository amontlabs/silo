# Does a concept keep one name in the plan, the code and the app

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Each part has one front door.** Each part has one public file that re-exports its interface, and
  imports go through declared modules, never up to a parent directory. Keep one implementation per
  contract. The build declares each engine part as a module, so an import that bypasses it does not
  compile.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/db/root.zig#L51))
- **Docs checked against the code.** Code shown in docs is generated from the source or anchored to
  a file and line, and a check fails when a name it cites no longer exists. Extend the `links`
  check: names and file anchors in engine docs must exist. Guillotine's notes beside files describe
  changes already made.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/performance/TRACER.md?plain=1#L54))

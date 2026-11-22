# What speaks to the engine from the app and the command line

- The owner set this on 16 November 2026.
- That the Mac app is Swift and only renders is `mac-app-language`, in the part `views`.

## Sources

- [Integrating Zig and SwiftUI](https://mitchellh.com/writing/zig-and-swiftui) (Mitchell
  Hashimoto): a Zig core linked into a Swift app through a C interface.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

The answer's last sentence was added on 22 November 2026, from these lessons.

- **Encode, never concatenate.** Serialize structured output with an encoder that escapes every
  special character, never by joining strings. The client protocol, record exports and anything Silo
  writes as JSON go through one encoder.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/test_runner.zig.md?plain=1#L17))
- **One definition for both sides of a protocol.** Derive request and response types, and the
  foreign header, from one definition. The writer and the reader share one predicate for every
  optional part. Each call of the engine is defined once in Zig, and the client, the C header and
  the reference for the command line come from that definition.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/engine/api.zig#L14))
- **Unknown fields are refused, missing ones get a default.** A newer reader gives a field that
  older data lacks its default. A reader refuses a field it does not know. An older Silo must never
  ignore a field it does not know, such as one in a recipe that closes the network: ignoring it
  would grant more than was written.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/block/README.md?plain=1#L212))
- **One owner per buffer, freed where it was made.** Say who allocates, borrows and frees every
  buffer that crosses a boundary, pair each creation with its release on the same side, and state
  how long a borrowed value stays valid. For the C interface of the client and the Swift driver.
  Guillotine found leaks there late.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/storage/journal.md?plain=1#L84))
- **Owned and borrowed are different types.** Make owned and borrowed storage distinct types, and
  give only the owned one a free operation. Never infer ownership from a length. Freeing a borrowed
  value then does not compile.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/memory/memory.zig#L58))
- **Copy what outlives its scope.** A result that must outlive the memory that produced it is copied
  into owned memory. A copy of a struct with pointers shares what they point to. Results handed to
  the app are owned copies.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/src/call_result.zig#L198))
- **Opaque handles, stable layouts, numbered errors.** Foreign callers get opaque handles, explicit
  structure layouts and error codes whose numbers are tested. Each result carries its own error
  text. Guillotine's C API keeps the error text in a buffer that the next call overwrites. In Silo,
  handles are ids with a generation, not pointers.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/evm_c_api.zig#L36))
- **Check pointers, lengths and types at the boundary.** Validate a foreign pointer, length and
  alignment before building a typed view. Where they cannot be checked, write the caller's duties in
  the header.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/memory/REVIEW_SUMMARY.md?plain=1#L96))
- **Test through the bindings, with leak checks.** Run tests through the foreign wrappers as well as
  the native calls, with an allocator that fails a test that leaks. The Zig client is tested through
  its C interface under `std.testing.allocator`.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/memory/REVIEW_SUMMARY.md?plain=1#L15))
- **Every call gets a reply.** Always answer a request, even one whose id cannot be read, so a
  caller is never left waiting for an answer that will not come. A client call that cannot be parsed
  still gets an error back. At the boundary, `refusal-says-so` proposes the same: denied, or not
  answered.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/rpc/eth.zig#L59))
- **Every handle is checked on every call.** Look every foreign handle up before using it, not only
  when it is released. Releasing a handle you do not own, releasing after shutdown, and shutting
  down with handles still live are reported, never ignored. The client and the Swift driver refuse a
  handle the engine never issued, and say when one is used after it was released.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/evm_c_api.zig#L611))
- **Start and stop exist from the first version.** Ship a library's start and stop entry points from
  the first version, even when they do nothing yet, so callers adopt them once. The C interface of
  the client has its init and deinit from day one.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/root_c.zig#L49))
- **The version comes from the build.** Report a program's version from the build's own metadata,
  never from a literal typed in the code, and keep test-only entry points out of what ships by the
  build itself.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/root_c.zig#L40))

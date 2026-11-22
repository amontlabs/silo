# How the engine is built

The owner said these. Two other lines of the same list have their home elsewhere: "No time
estimates" binds every agent and is in the repository's `AGENTS.md`; "Properties are stated and
checked as we build" is `properties-are-checked`, in the part `checking`.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Clean up right after each acquisition, and test it.** Place `errdefer` right after each fallible
  acquisition, and test the failure paths after partial acquisition. In a function that swallows an
  error, `errdefer` does not run. For drivers, the recorder and the client, which allocate. Zig's
  `std.testing.checkAllAllocationFailures` fails each allocation in turn.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/frame/frame.zig#L187))
- **Caches follow writes, or drop the entry.** A write updates or invalidates the cache in front of
  a store. A failed fetch drops the cached entry. Every cache derived from a structure is cleared
  when it changes. Name which copy is the authority. For the helper's and the client's caches.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/STATE-MANAGEMENT.md?plain=1#L46))
- **Locks in one fixed order.** Where code holds several locks, take them in one global order,
  release each with `defer`, copy what a display reads while holding the lock, and state per object
  which calls may run together. The core runs on one thread. This is for drivers and the helper if
  they use threads.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/TESTING.md?plain=1#L657))
- **A readable dump of every prepared form.** Every prepared or compiled form, and the state, can be
  printed in a form a person reads. Compiled rules and the core's tables each have a text dump, used
  in failure reports.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/preprocessor/dispatch.zig#L1308))
- **Poison freed memory.** After tearing an object down, set it to `undefined` before freeing it,
  and run tests under an allocator that reports misuse. In Zig's safe builds, memory set to
  `undefined` is filled with a marker byte, which makes a later use visible; the testing allocator
  reports leaks and double frees.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/frame/CLAUDE.md?plain=1#L40))
- **Rules in prose drift; checks hold.** Every banned pattern is listed by its exact name and
  enforced by a mechanical check before merge, not by a reviewer reading a prompt. Guillotine bans
  `catch {}`, commented-out tests and placeholders in its agent file; the code has 26 `catch {}`, 40
  commented-out test imports and hundreds of `catch unreachable`. That is the strongest case for
  taking `how-the-engine-is-built`.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/CODEBASE_REVIEW.md?plain=1#L932))

# What the core may depend on

## Found

A trial on 21 November 2026 built a small core in Zig 0.17.1. It is described in the notes of
`core-simulator`. The first sentence of the proposed answer holds up: the core imports only Zig's
standard library, and uses one thing from it, `std.debug.assert`. A test that reads the core's
source holds it to that.

Its tests need nothing from outside Zig either. The simulator is 201 lines of our own, and the
fuzzer comes with Zig.

Two libraries were looked at and not taken:

- Marionette 0.9.0, alpha. It simulates `std.Io`, with faults on the disk, the network and time.
  The core has no `std.Io`, so it has no use for it. It could serve the drivers and their fakes.
- ZlowCheck, pre-alpha, written for Zig before 0.15.

Not tested: the rest of the answer, about drivers. The trial had no driver and no fake.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Commands as argument lists.** Run external commands with an explicit list of arguments, never a
  string handed to a shell. Drivers that run tools (Git in a computer, SSH, libkrun helpers) pass
  paths that an agent may have named. A shell would reinterpret them.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/build.zig.md?plain=1#L481))
- **A narrow interface to the outside.** Reach the outside through a narrow interface. Use a table
  of functions where the backend changes at run time and a comptime parameter where there is one,
  and generate the type-erasing wrapper at compile time so implementers never write the cast.
  Drivers sit behind such interfaces. Silo owns its few drivers, so a driver missing an operation
  fails to compile.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/prd/GUILLOTINE_CLIENT_PLAN.md?plain=1#L187))
- **Ask what a driver can do.** Expose whether a backend supports a capability natively instead of
  assuming it. A driver states what it supports, for example whether its libkrun build takes memory
  snapshots, and the core decides from that.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/db/adapter.zig#L480))

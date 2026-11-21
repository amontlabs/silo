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

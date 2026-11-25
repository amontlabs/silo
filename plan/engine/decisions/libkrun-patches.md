# How Silo carries its patches to libkrun

- Why upstream: a large patch carried against a rewrite in progress drifts the way MicroSandbox's
  copy did. That copy is 653 commits behind upstream, as of 19 November 2026.
- The first sentence of the answer is marked Notes in the record, the second Proposed.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **A dependency's internals in one adapter.** Reach into a dependency's internals from one adapter
  file only, record beside it the upstream accessor that will replace it, and use that accessor as
  soon as it exists. Where Silo reaches into libkrun or MicroSandbox beyond their public interface,
  it does so in one file that names the upstream change that will remove it.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/blockchain/local_access.zig#L6))

## The write hook, written

An agent's part, 25 November 2026, from [the write log's research](../../../research/2026-11-25-write-log/README.md).
The patch option B needs is written against libkrun's `main` of 23 November 2026, as two commits
shaped for upstream. The first is a fix on its own: a write, discard or write-zeroes to a sector of
2^55 or more wrapped around, so a release build wrote it to sector 0 and told the guest it worked,
and a debug build panicked. It helps every user of libkrun and needs nothing of Silo. The second
adds the hook: `krun_block_device_set_write_hook(handle, hook, user)`, called before each change to
the disk with a range checked to lie inside it. Both build, and 19 unit tests pass; no virtual
machine ran them, since the container has no KVM. Sending them upstream is outward-facing, so it
waits for polarzero, and the `Signed-off-by` line libkrun requires has to be polarzero's.

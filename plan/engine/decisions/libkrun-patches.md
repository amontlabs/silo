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

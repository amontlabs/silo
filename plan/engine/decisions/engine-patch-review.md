# How a patch to the VM engine is reviewed

- The record says "a patch to the engine"; the engine meant is the VM engine, libkrun.
- Snapshot, device and display code runs in the helper process, which a hostile guest attacks, and
  restore reads back state the guest controlled. Most of MicroSandbox's security advisories of
  2026 were in such layers: snapshot capture and import, the disk journal, file copy. Facts as of
  19 November 2026, from [research/linux-vm-engine.md](../../../research/linux-vm-engine.md).

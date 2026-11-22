# How a patch to libkrun is reviewed

- The record says "a patch to the engine"; what it means is libkrun, the virtual machine monitor
  that runs Linux computers, and not Silo's engine.
- Snapshot, device and display code runs in the helper process, which a hostile guest attacks, and
  restore reads back state the guest controlled. Most of MicroSandbox's security advisories of
  2026 were in such layers: snapshot capture and import, the disk journal, file copy. Facts as of
  19 November 2026, from [the research in the notes of `linux-vm-engine`](linux-vm-engine.md).

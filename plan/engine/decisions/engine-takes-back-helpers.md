# Can a restarted engine take back its helpers

To find out, for Apple's framework and for the VM engine of Linux computers (`linux-vm-engine`).

- A virtual machine lives in the process that created it.
- libkrun runs a virtual machine in the process that calls it. Fact as of 19 November 2026, from
  [the research in the notes of `linux-vm-engine`](linux-vm-engine.md).

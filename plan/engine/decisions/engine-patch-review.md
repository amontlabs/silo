# How a patch to libkrun is reviewed

- The record says "a patch to the engine"; what it means is libkrun, the virtual machine monitor
  that runs Linux computers, and not Silo's engine.
- Snapshot, device and display code runs in the helper process, which a hostile guest attacks, and
  restore reads back state the guest controlled. Most of MicroSandbox's security advisories of
  2026 were in such layers: snapshot capture and import, the disk journal, file copy. Facts as of
  19 November 2026, from [the research in the notes of `linux-vm-engine`](linux-vm-engine.md).

## The first patch reviewed this way

An agent's part, 25 November 2026, from [the write log's research](../../../research/2026-11-25-write-log/README.md).
The write hook reads three values the computer controls: the sector, the number of sectors and the
length of the data. Its hostile test sends 2,000 random requests whose ranges sit near the end of
the disk and near the largest numbers, and checks that the hook never sees a range outside the disk
and that every byte that changed lies in a range the hook was told of. Writing that test found a
bug already in libkrun: a sector of 2^55 or more wrapped to sector 0. libkrun has no fuzz targets
yet, so the test is a seeded loop inside its unit tests, not a fuzzer; a `cargo fuzz` target on the
same function is the next step.

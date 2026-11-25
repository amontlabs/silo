# Where a computer's system image comes from

## The choice

An agent's part, 22 November 2026. You make your first Linux computer beside main to run your tests
on Linux. Its system has to come from somewhere. For main's macOS there is no choice: Apple forbids
shipping it installed, so each Mac downloads and installs its own, over 10 GB.

The options:

- Ubuntu, downloaded the first time (recommended). Silo fetches Ubuntu's official image, under 1 GB,
  once, and every later Linux computer starts from that copy. Agents get the system most guides use
  and GitHub's own build machines run. It costs that download the first time you make a Linux
  computer.
- Ubuntu, shipped inside Silo. Nothing to wait for later. It costs everyone a larger download of
  Silo, including people who never make a Linux computer.

Recommended: Ubuntu, downloaded the first time. Many people start with main alone, and the wait
comes once, when a Linux computer is first needed.

## Notes

| | Linux computer | macOS computer |
|---|---|---|
| Image | One Ubuntu image, downloaded once. | Downloaded and installed on each Mac. |

The answer above is these two cells, written as one text.

Apple forbids shipping a pre-installed macOS image. Each Mac downloads and installs its own, so
the first run takes longer than with a Linux image (`first-run-time-macos`).

## Ubuntu's image as it is

An agent's part, 25 November 2026, from [option B's specs](../../../research/2026-11-25-linux-main/README.md). Read on that day: Ubuntu 26.04's cloud image for arm64
is a qcow2 file of 943 MB, checked against Ubuntu's `SHA256SUMS`, with 95 % of its clusters
compressed; root in partition 1, `/boot` in 13, EFI in 15. A computer boots a raw disk, so the copy
Silo keeps is the converted raw disk, and its hash can be pinned too: proposed words after "checks
it against Ubuntu's published checksum and keeps it": "converted to a raw disk whose checksum Silo
also pins,".

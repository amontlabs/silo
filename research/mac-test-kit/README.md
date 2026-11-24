# Silo's Mac test kit

The research of 24 November 2026 left about 30 questions that only a Mac can answer: how long an APFS
clone stalls a disk, whether Apple's guest provisioning accepts your own user name, whether
gvproxy works under a macOS computer, and more. This folder runs them. Each test ends on one
outcome, and [RESULTS.md](RESULTS.md) says what each outcome means for which decision of the plan.

Nothing here has run on a Mac yet. Every part ran on Linux against fakes and Linux analogues
(`./kit selftest`), so expect a first-run fix or two, most likely in the VM helper's Swift.

## Before you start

- **The VM tests need macOS 27 on your Mac.** Apple's guest provisioning, which sets up a new
  macOS computer with nobody clicking, needs macOS 27 on both sides, and a Mac can only install a
  guest no newer than itself. Your Mac ran macOS 26.5 on 21 November. On 26.5, only the `quick`
  tier runs; the rest skip themselves and say why.
- Install the Command Line Tools: `xcode-select --install`. They bring `cc`, `swiftc` and
  `python3`.
- Tests 23 to 30, for Mac and iOS developers, need Xcode installed in `/Applications` (23 copies it
  into a test computer). Test 30 needs an iPhone on USB and stops after finding it, because the VM
  helper cannot pass a USB device through yet.
- About 100 GB free for the VM tests, 30 GB for the quick ones, on the internal SSD.
- Run it in Terminal on mains power, with the Mac set not to sleep while the long tests run.

## Running it

```
cd research/mac-test-kit
./kit build              # compiles the VM helper and the C probes into ~/SiloKit/bin
./kit list               # every test, its tier, time and needs
./kit run quick          # 03 05 06 20: about 40 minutes, no VM, no sudo, no download
./kit run 00             # downloads macOS 27 (about 18 GB) and installs the base computer: about an hour
./kit run vm             # 01 04 07 10 11 14 21 23 24 26 27 32 33: about 7 hours, unattended after the first questions
./kit run guided         # 02 08 09 19 22 28 29 30: you at the keyboard for sign-ins and app checks
./kit run long           # 12 13 15 16 17 18 25 31: hours; 16 is meant for a night; 31 downloads Zig (51 MB)
./kit pack               # one .tar.gz of the last run, to hand back to an agent
```

A test asks before anything that downloads, starts a computer or takes more than 10 minutes.
`--yes` answers those questions in advance, so `./kit run --yes vm` runs on its own.

If your time is short, the order that moves the most decisions is: `quick` and `31` (no VM),
then `00`, `01`, `10`, `04`, `14`, `11`, then `02` (the first build's promise that Claude Code
stays signed in overnight), then `07`.

## What it changes on your Mac

Everything goes under `~/SiloKit`, which `./kit clean --all` deletes: the macOS restore image, the
installed computer, the clones each test makes and deletes, the results, the kit's own SSH key.
The computers are reached with that key and never through `~/.ssh`.

Eight tests touch something else, and each asks first and undoes it when it ends (or at
`./kit clean`):

- **03** attaches an ASIF disk image for the length of the test. If the device is not yours, it
  asks for sudo to use it.
- **07** runs a stand-in server on all your Mac's addresses for the length of the test, as a
  control. macOS's firewall may ask whether to allow it.
- **08** adds one `Include` line to the top of `~/.ssh/config`, so Claude desktop and the Codex
  app can see a test computer. It keeps a copy and puts the file back byte for byte.
- **09** loads a launchd job in your user domain, and with `--allow-sudo` one in the system
  domain, to see whether a computer can start from there. Both are removed at the end.
- **20**, with `--allow-sudo`, runs one client as the user `nobody` to see it refused.
- **23** reads your Xcode (`/Applications/Xcode.app`, or `KIT_XCODE`) to copy it into a computer.
  It changes nothing on the Mac; 24 to 30 then start clones of that computer.
- **28** reads your App Store Connect API key where it is, for a server on `127.0.0.1` that signs
  the computer's App Store Connect calls with it: read-only ones, and with `KIT_ASC_PROJECT` a
  build that may create a development certificate and profile. The key is never copied into a
  computer, and the server stops with the test.
- **30** hands your iPhone's USB connection to a computer while it runs; it comes back to the Mac
  when the computer stops.

`sudo` is used on the Mac only with `--allow-sudo`, and each use says why first. Tests that need
root inside a computer use `sudo` there, never on the Mac. Nothing restarts your Mac or puts it to
sleep; when a test needs that (09's FileVault restart, 07's sleep), it asks you to do it and you
can skip.

## Results

Each run writes `~/SiloKit/results/<date>/`:

- `results.jsonl`: one JSON object per line, each a measure, a fact, a note or the outcome of a
  test;
- `summary.md`: per test, its outcome and what that outcome means;
- `host.json`: the Mac's model, chip, macOS version and FileVault state;
- `logs/NN.log` and `attachments/NN/`: everything each test printed, and files it kept.

`./kit pack` makes one archive of it. Give that archive to an agent with this folder, and it can
read every result against RESULTS.md.

## How the kit is made

| Where | Holds |
|---|---|
| `kit` | The entry command |
| `tests/NN-*.sh` | One test each. Its header gives its question, tier, time, needs, what it decides, and its outcomes with their meaning; `./kit doc` writes RESULTS.md from those headers |
| `lib/` | Helpers the tests share: results (`common.sh`), computers (`vm.sh`, `first-start.sh`, `guest.sh`), gvproxy (`net.sh`), Xcode and simulators (`ios.sh`), the checkpoint engine prototype and its NBD server (`ckpt.sh`, `nbd.sh`), the build (`build.sh`) |
| `swift/` | `silovm`, the VM helper on Apple's Virtualization framework. Its commands are in [swift/README.md](swift/README.md) |
| `c/` | Probes built by `./kit build`: clone and fsync benchmarks, block maps, socket peers, an SSH proxy |
| `py/` | Analysis, the stand-in servers, and a read-only APFS reader in `py/apfs/` |
| `guest/` | Scripts copied into a test computer (perl and sh: macOS has no python3 until the Command Line Tools are installed there) |
| `linux/` | `./kit selftest` and the fakes and Linux analogues it runs |

To add a test, copy the header of an existing one, end it with one `kit_outcome`, and run
`./kit selftest`.

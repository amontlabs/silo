# Silo's Mac test kit

The research of 24 November 2026 left about 40 questions that only a Mac can answer: how long an APFS
clone stalls a disk, whether Apple's guest provisioning accepts your own user name, whether
gvproxy works under a macOS computer, and more. This folder runs them. Each test ends on one
outcome, and [RESULTS.md](RESULTS.md) says what each outcome means for which decision of the plan.
[DECISIONS.md](DECISIONS.md) turns them around: for each open decision, the tests that bear on it
and what each outcome leaves possible, so a run ends on one verdict per decision.

The tests follow the first build's steps (#370): each says which step it serves, and the ones that
block a step (#373) run first.

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
./kit list               # every test in the order it runs, with its tier, step, time and needs
./kit run blocking       # 00, then 07 21 35: what blocks option A of the first build (#373)
./kit run step2          # every test of one step of the first build (#370): step1 to step6, steplater
./kit run all            # everything, in the kit's order
./kit run quick          # 03 05 06 20: about 40 minutes, no VM, no sudo, no download
./kit run 00             # downloads macOS 27 (about 18 GB) and installs the base computer: about an hour
./kit run vm             # 01 04 07 10 11 14 21 23 24 26 27 32 33 35 36 38 39: about 10 hours, unattended after the first questions
./kit run guided         # 02 08 09 19 22 28 29 30 37: you at the keyboard for sign-ins, app checks and a sleep
./kit run long           # 12 13 15 16 17 18 25 31 34: hours; 16 and 34 are meant for a night; 31 downloads Zig (51 MB)
./kit verdicts           # one verdict per open decision, from every run so far (the newest outcome of each test)
./kit pack               # one .tar.gz of the last run, to hand back to an agent
```

Every group runs in one order: the base computer (00), then the tests that block a step and take
no night (07, 21, 35), then each step's tests from step 1 to step 6 (#370's five, and #373's
boundary) and those for later, the cheapest tier first within a step. `./kit list` shows that
order.

| Step of the first build | Tests |
|---|---|
| 1. The engine (#370) | 06 20 |
| 2. Main runs (#370) | 00 01 02 04 07 08 09 10 11 12 13 14 21 35 36 |
| 3. A night (#370) | 16 37 |
| 4. The way back (#370) | 03 15 17 31 32 33 38 |
| 5. The record, and the morning (#370) | 05 18 19 39 |
| 6. The boundary for model traffic (#373) | 34 |
| Later | 22 to 30 |

A test asks before anything that downloads, starts a computer or takes more than 10 minutes.
`--yes` answers those questions in advance, so `./kit run --yes vm` runs on its own.

If your time is short, run `blocking`, then `step2`: they decide what main is and what carries
its network. `34` runs a night of Claude Code and Codex on your plans through a stand-in reading
boundary; it blocks #373's last step but spends both plans for the night, so it runs at its step.

## What it changes on your Mac

Everything goes under `~/SiloKit`, which `./kit clean --all` deletes: the macOS restore image, the
installed computer, the clones each test makes and deletes, the results, the kit's own SSH key.
The computers are reached with that key and never through `~/.ssh`.

Eleven tests touch something else, and each asks first and undoes it when it ends (or at
`./kit clean`):

- **03** attaches an ASIF disk image for the length of the test. If the device is not yours, it
  asks for sudo to use it.
- **07** runs a stand-in server on all your Mac's addresses for the length of the test, as a
  control. macOS's firewall may ask whether to allow it.
- **35** builds gvproxy with #363's patch (`net/gvproxy-sealed.patch`) from its source at
  v0.9.0, with Go 1.26.0 downloaded into `~/SiloKit` unless that Go is installed, and runs test
  07's stand-in server as a control.
- **36** runs a small server on all your Mac's addresses for the length of the test, since that is
  what the computer's connections reach. macOS's firewall may ask whether to allow it.
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
- **37** puts your Mac to sleep once: you do it, or with `--allow-sudo` the kit schedules a wake
  3 minutes later (`pmset relative wake`) and sleeps it with `pmset sleepnow`.
- **38** makes a 2 GB disk image with a volume of its own, attaches it for the length of the test
  and fills it. Your own volume is never filled.
- **30** hands your iPhone's USB connection to a computer while it runs; it comes back to the Mac
  when the computer stops.

`sudo` is used on the Mac only with `--allow-sudo`, and each use says why first. Tests that need
root inside a computer use `sudo` there, never on the Mac. Nothing restarts your Mac; when a test needs
a restart or a sleep (09's FileVault restart, 07's and 37's sleep), it asks you to do it and you
can skip, except 37 with `--allow-sudo`, which sleeps the Mac itself and wakes it 3 minutes later.

## Results

Each run writes `~/SiloKit/results/<date>/`:

- `results.jsonl`: one JSON object per line, each a measure, a fact, a note or the outcome of a
  test;
- `summary.md`: per test, its outcome and what that outcome means, then one verdict per open
  decision from that run alone;
- `host.json`: the Mac's model, chip, macOS version and FileVault state;
- `logs/NN.log` and `attachments/NN/`: everything each test printed, and files it kept.

`./kit verdicts` reads every run at once and keeps the newest outcome of each test, so a test run
again replaces its earlier answer. A verdict is `settled` when every test that bears on the decision
answered and one option is left, `answered` when every test answered and the recommended option is
still among those left, `leaning` while some tests have not answered, and `conflict` when two
tests leave no option in common (then the decision goes back to the owner with both results).

`./kit pack` makes one archive of it. Give that archive to an agent with this folder, and it can
read every result against RESULTS.md.

## How the kit is made

| Where | Holds |
|---|---|
| `kit` | The entry command |
| `decisions.txt` | Each open decision, its options in the order #370 and #373 recommend them, and what each outcome of each test leaves possible. `./kit doc` writes DECISIONS.md from it and selftest checks it against the tests and the plan |
| `tests/NN-*.sh` | One test each. Its header gives its question, tier, time, needs, what it decides, and its outcomes with their meaning; `./kit doc` writes RESULTS.md from those headers |
| `lib/` | Helpers the tests share: results (`common.sh`), computers (`vm.sh`, `first-start.sh`, `guest.sh`), gvproxy (`net.sh`), Xcode and simulators (`ios.sh`), the checkpoint engine prototype and its NBD server (`ckpt.sh`, `nbd.sh`), the build (`build.sh`) |
| `swift/` | `silovm`, the VM helper on Apple's Virtualization framework. Its commands are in [swift/README.md](swift/README.md) |
| `c/` | Probes built by `./kit build`: clone and fsync benchmarks, block maps, socket peers, an SSH proxy, a sleep meter (`powerwatch`) |
| `py/` | Analysis, the stand-in servers, and a read-only APFS reader in `py/apfs/` (`apfscontent.py` reads files' contents, compressed ones included) |
| `net/` | #363's patch to gvproxy, which test 35 builds |
| `guest/` | Scripts copied into a test computer (perl and sh: macOS has no python3 until the Command Line Tools are installed there) |
| `linux/` | `./kit selftest` and the fakes and Linux analogues it runs |

To add a test, copy the header of an existing one, give it a step, end it with one
`kit_outcome`, add what its outcomes leave possible to decisions.txt, and run `./kit selftest`.

## What the Linux runs already show

The analogues cannot answer a Mac question, but three of them found something the Mac runs should
confirm:

- **A restart of gvproxy kills a computer's network for good when its link goes straight to
  gvproxy's socket**, as silovm and vfkit hand it (the connected datagram socket points at a
  socket that no longer exists). Through a relay that connects again and says who it is, the
  network came back 3 s after gvproxy did. Test 36 measures this on the Mac; if it holds, the
  helper owns each computer's network link and gvproxy never receives it directly.
- **#363's sealed gvproxy holds in the analogue**: root in the namespace reached nothing of the
  host, and still reached the internet and Silo's SSH forward. Gvproxy as released in its place is
  caught. Test 35 asks the same of a macOS computer.
- **A corrupted LZVN or LZFSE block decodes to wrong bytes without an error**: neither format
  carries a checksum, and only zlib's catches it. A reader of main's disk from outside therefore
  checks what it reads against something (the size, or a hash Silo keeps), never only that it
  decoded. Test 39 compares every compressed file it reads against the computer's own hash.


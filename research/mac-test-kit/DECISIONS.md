# By decision

Every open entry a test of the kit answers, with the options in the order recommended and
what each outcome leaves possible. The verdict of a run is the first option every answered
test leaves possible (`./kit verdicts`). Written by `./kit doc` from
[decisions.txt](decisions.txt), the only place these mappings are kept.

## `main-on-macos-first`

What is main in the first build: a macOS computer behind Silo's stack from its first start, a macOS computer on Apple's NAT first, or a Linux computer?

- Source: plan, open; #373 weighs options A, B and C of the first build and recommends A, with B as the fallback if the Mac kit fails on provisioning or gvproxy
- Tests: 00 (Download macOS and install the base computer), 01 (Provisioning with the Mac's own login name), 13 (Ten fresh computers, and one with a new identity), 07 (gvproxy with a macOS computer, against Apple's NAT), 35 (Silo's gvproxy (#363's seal, grown in #380) with a macOS computer: what root inside reaches, sealed and closed), 40 (A network card that leads nowhere, with Silo's relay and proxy over vsock (#378)), 47 (Option B's Linux main through libkrun on this Mac: boot, stop, memory given back, disk speed (#381 L1 to L3))

| Option | Means |
|---|---|
| `a-macos-boundary` | A: macOS main, on Silo's network stack and behind the boundary from its first start (#373) |
| `c-macos-nat` | C: macOS main on Apple's NAT first, as #348 and #370 specify |
| `b-linux` | B: a Linux main on the Mac first, and macOS main later (#373) |
| `a-macos-deadend` | A with #378's dead-end card: macOS main whose only card answers ARP for the router and drops the rest; its way out is a relay over vsock to Silo's proxy (#378, #380; listed last until the owner rules on the order) |

| Test and outcome | Leaves possible |
|---|---|
| 00 `install-failed` | `b-linux` |
| 01 `both-fail` | `b-linux` |
| 13 `all-failed` | `b-linux` |
| 07 `gvproxy-fails` | `a-macos-deadend`, `c-macos-nat`, `b-linux` |
| 35 `sealed-breaks` | `a-macos-deadend`, `c-macos-nat`, `b-linux` |
| 40 `leaks` | `a-macos-boundary`, `c-macos-nat`, `b-linux` |
| 40 `relay-fails` | `a-macos-boundary`, `c-macos-nat`, `b-linux` |
| 47 `no-boot` | `a-macos-deadend`, `a-macos-boundary`, `c-macos-nat` |

## `network-stack`

What carries main's network in the first build?

- Source: plan, open; #370 assumes Apple's NAT until a patched gvproxy passes test 07; #373 needs a stack from main's first start; #363 seals gvproxy with a 64-line patch, which #380 grows into Silo's gvproxy (-sealed, -closed)
- Tests: 07 (gvproxy with a macOS computer, against Apple's NAT), 35 (Silo's gvproxy (#363's seal, grown in #380) with a macOS computer: what root inside reaches, sealed and closed), 36 (What a computer's connections meet on gvproxy and on Apple's NAT, and a restart of the stack), 40 (A network card that leads nowhere, with Silo's relay and proxy over vsock (#378)), 41 (Ping and name lookups past gvproxy: as released, #363's patch and #380's), 42 (Throughput over the datagram socket a Mac gives gvproxy, at MTU 1500 and the largest the framework takes)

| Option | Means |
|---|---|
| `sealed-gvproxy` | Silo's gvproxy (#363's seal, grown in #380) with -sealed: nothing of the Mac, its loopback or its network is reached from inside, and -closed shuts every way out |
| `configured-gvproxy` | gvproxy as released, configured as research 04 says (API token, no map to the Mac's 127.0.0.1); the Mac's network stays reachable |
| `nat-first` | Apple's NAT until a stack passes; main's network has no boundary and reaches your network (#370) |
| `own-stack` | Silo's own stack, written before main runs |
| `silo-gvproxy` | gvproxy with #380's patch (-sealed, -closed): no ping, no name looked up by the stack itself |
| `dead-end-card` | no stack: #378's card answers ARP and drops everything; main's only way out is a relay over vsock to Silo's proxy, its way in SSH over vsock (#378 and #380 recommend it for the first build; listed last until the owner rules on the order) |

| Test and outcome | Leaves possible |
|---|---|
| 07 `gvproxy-stalls` | `dead-end-card`, `nat-first`, `own-stack` |
| 07 `gvproxy-fails` | `dead-end-card`, `nat-first`, `own-stack` |
| 07 `hardening-fails` | `dead-end-card`, `silo-gvproxy`, `sealed-gvproxy`, `nat-first`, `own-stack` |
| 35 `sealed-leaks` | `dead-end-card`, `silo-gvproxy`, `configured-gvproxy`, `nat-first`, `own-stack` |
| 35 `sealed-breaks` | `dead-end-card`, `configured-gvproxy`, `nat-first`, `own-stack` |
| 36 `alters` | `dead-end-card`, `nat-first`, `own-stack` |
| 40 `leaks` | `silo-gvproxy`, `sealed-gvproxy`, `configured-gvproxy`, `nat-first`, `own-stack` |
| 40 `relay-fails` | `silo-gvproxy`, `sealed-gvproxy`, `configured-gvproxy`, `nat-first`, `own-stack` |
| 41 `silo-leaks` | `dead-end-card`, `sealed-gvproxy`, `configured-gvproxy`, `nat-first`, `own-stack` |
| 41 `silo-breaks` | `dead-end-card`, `sealed-gvproxy`, `configured-gvproxy`, `nat-first`, `own-stack` |
| 41 `fact sealed363_ping_mac=reached` | `dead-end-card`, `silo-gvproxy`, `configured-gvproxy`, `nat-first`, `own-stack` |
| 42 `datagram-limits` | `dead-end-card`, `nat-first`, `own-stack` |

## `network-place`

Where does each computer's network end, and what of the Mac can it reach?

- Source: plan, open: what test 40 shows of the dead-end card fills it in
- Tests: 40 (A network card that leads nowhere, with Silo's relay and proxy over vsock (#378))

Measured, not chosen: the verdict is the measures of the tests above.

## `network-before-rules`

Is main's network closed before Silo's rules are in force?

- Source: #348, draft; #380 closes ping and the stack's own lookups in gvproxy
- Tests: 41 (Ping and name lookups past gvproxy: as released, #363's patch and #380's)

Measured, not chosen: the verdict is the measures of the tests above.

## `ports-reached-from-your-mac`

How do you open a computer's port from your Mac, and what can that page reach?

- Source: plan, open; #384 measured a browser of Silo's own per computer on Linux
- Tests: 44 (The browser Silo starts for a port preview, on a Mac: names, WebRTC, other computers, its own traffic)

Measured, not chosen: the verdict is the measures of the tests above.

## `mac-silo-needs`

Which macOS must the Mac run for Silo?

- Source: #348, open; #370 assumes macOS 27 or later for Apple's guest provisioning
- Tests: 00 (Download macOS and install the base computer), 01 (Provisioning with the Mac's own login name), 10 (A first start cut off after 20 seconds), 13 (Ten fresh computers, and one with a new identity), 47 (Option B's Linux main through libkrun on this Mac: boot, stop, memory given back, disk speed (#381 L1 to L3))

| Option | Means |
|---|---|
| `macos-27` | macOS 27 or later: Apple's guest provisioning sets main up with nobody at its screen |
| `macos-26-too` | macOS 26 too: main's first start is set up by a keystroke script, or by writing Apple's private formats onto its disk |

| Test and outcome | Leaves possible |
|---|---|
| 01 `both-fail` | `macos-26-too` |
| 13 `all-failed` | `macos-26-too` |
| 10 `not-provisioned` | `macos-27`, `macos-26-too` |

Tests 00, 47 give evidence (their numbers and facts) and rule out no option.

## `computer-image`

What does Silo keep of macOS to make each new computer from?

- Source: plan, open; #352's macOS half, which #370 follows
- Tests: 00 (Download macOS and install the base computer), 10 (A first start cut off after 20 seconds), 13 (Ten fresh computers, and one with a new identity), 23 (Xcode copied in from the Mac, and the iOS runtime downloaded in the computer)

| Option | Means |
|---|---|
| `installed-never-started` | the installed system, kept before its first start; each computer is provisioned at its own first start |
| `set-up-system` | a system already set up; every new computer is a fork of it and needs fork-identity first |

| Test and outcome | Leaves possible |
|---|---|
| 10 `not-provisioned` | `installed-never-started` |
| 10 `refused-second-start` | `installed-never-started` |
| 13 `reliable-identity-fails` | `installed-never-started` |

Tests 00, 23 give evidence (their numbers and facts) and rule out no option.

## `moving-in-on-macos`

Does main's user have your own login name and home path?

- Source: plan, open; #370 builds both and lets test 01 pick the default
- Tests: 01 (Provisioning with the Mac's own login name)

| Option | Means |
|---|---|
| `own-name` | main's user has your login name, so paths written in settings stay right |
| `another-name` | main's user has another name; settings that name your home path are listed or rewritten |

| Test and outcome | Leaves possible |
|---|---|
| 01 `own-name-refused` | `another-name` |
| 01 `own-name-silent` | `another-name` |

## `first-run-time-macos`

How long does the first run take with a macOS main?

- Source: plan, open: measured, not chosen
- Tests: 00 (Download macOS and install the base computer), 13 (Ten fresh computers, and one with a new identity), 14 (What provisioning leaves in a new computer), 23 (Xcode copied in from the Mac, and the iOS runtime downloaded in the computer), 43 (How the first run reaches main before the relay is in it (#382))

Measured, not chosen: the verdict is the measures of the tests above.

## `way-in-before-the-relay`

How does Silo reach main on its first start, before main's relay to the proxy is installed?

- Source: #382, draft: four ways, the first that test 43 leaves
- Tests: 43 (How the first run reaches main before the relay is in it (#382))

| Option | Means |
|---|---|
| `nat-first-start` | main's first start on Apple's NAT, its card swapped for the dead-end card once provisioned |
| `gvproxy-first-start` | main's first start behind gvproxy, which is then replaced by the dead-end card |
| `own-tcp-one-connection` | a TCP stack of Silo's own that carries one connection, to the installer |
| `relay-by-provisioning` | Apple's guest provisioning installs the relay itself, so main never needs a network |

| Test and outcome | Leaves possible |
|---|---|
| 43 `nat-first-start-fails` | `gvproxy-first-start`, `own-tcp-one-connection`, `relay-by-provisioning` |
| 43 `fact gvproxy_first_start=fails` | `nat-first-start`, `own-tcp-one-connection`, `relay-by-provisioning` |
| 43 `fact provisioning_can_run_program=no` | `nat-first-start`, `gvproxy-first-start`, `own-tcp-one-connection` |

## `linux-vm-engine`

How does Silo run a Linux computer on the Mac: libkrun on Apple's Hypervisor framework, and what it costs there?

- Source: plan, open: measured; #381 measured it on Linux with KVM and left L1 to L8 to a Mac
- Tests: 47 (Option B's Linux main through libkrun on this Mac: boot, stop, memory given back, disk speed (#381 L1 to L3)), 48 (Option B's disk.raw on APFS: a clonefile while main writes, and discards (#381 L5, L8)), 49 (Option B's main run by a launch daemon while you are logged out (#381 L7))

Measured, not chosen: the verdict is the measures of the tests above.

## `engine-saved-state`

How does the engine save its own state on a Mac?

- Source: plan, open; #370 follows #352 file 02, with #352 file 03 as the other option
- Tests: 06 (Cost of a forced write, and overwrites after a clone), 31 (The checkpoint engine's prototype on APFS: full, cbw, nbd, delta, and the catalog under kill), 45 (What a disk keeps of the record when its power is cut: F_FULLFSYNC against fsync)

| Option | Means |
|---|---|
| `log-and-two-places` | a log and two state places, one F_FULLFSYNC per group of writes, a stop on any failed write (#352 file 02) |
| `whole-save` | a save of the whole state after each input, no log (#352 file 03) |

| Test and outcome | Leaves possible |
|---|---|
| 06 `full-slow-reserve-needed` | `log-and-two-places` |
| 06 `full-slow-prealloc-protects` | `log-and-two-places` |

Tests 31, 45 give evidence (their numbers and facts) and rule out no option.

## `space-the-engine-keeps`

How does the engine keep room to write on a full volume?

- Source: #348, open; #370 assumes a reserve file of 4 GiB and a floor of 20 GB until tests 06 and 33 and a full volume measure them
- Tests: 06 (Cost of a forced write, and overwrites after a clone), 33 (Does the guest's TRIM free the blocks of the disk image on the Mac), 38 (The Mac's volume fills under a running computer), 46 (What deleting a checkpoint frees while a local Time Machine snapshot exists), 48 (Option B's disk.raw on APFS: a clonefile while main writes, and discards (#381 L5, L8))

| Option | Means |
|---|---|
| `reserve-file` | a reserve file the engine makes and frees when the volume fills, and a floor under which checkpoints stop |
| `preallocate` | the engine's own files preallocated, no reserve |

| Test and outcome | Leaves possible |
|---|---|
| 06 `full-fast-reserve-needed` | `reserve-file` |
| 06 `full-slow-reserve-needed` | `reserve-file` |
| 38 `framework-stops` | `reserve-file` |
| 38 `guest-damaged` | `reserve-file` |
| 46 `snapshot-holds-space` | `reserve-file` |
| 48 `trim-ignored` | `reserve-file` |

Tests 33 give evidence (their numbers and facts) and rule out no option.

## `low-disk`

What does Silo do when the disk runs low, given what a full volume does to a running computer?

- Source: plan, open: its levels are measured
- Tests: 06 (Cost of a forced write, and overwrites after a clone), 33 (Does the guest's TRIM free the blocks of the disk image on the Mac), 38 (The Mac's volume fills under a running computer), 46 (What deleting a checkpoint frees while a local Time Machine snapshot exists)

Measured, not chosen: the verdict is the measures of the tests above.

## `events-until-the-recorder`

How large is the engine's table of events waiting for the recorder?

- Source: plan, open: sized from the cost of a flush
- Tests: 06 (Cost of a forced write, and overwrites after a clone)

Measured, not chosen: the verdict is the measures of the tests above.

## `who-can-call-the-engine`

How does the engine know who called it?

- Source: plan, open; #370 assumes the same user, by getpeereid on a Unix socket
- Tests: 20 (The engine's socket: who connected, and what exec changes), 07 (gvproxy with a macOS computer, against Apple's NAT)

| Option | Means |
|---|---|
| `same-user-socket` | the same user, by getpeereid on a Unix socket in a folder only that user reaches |
| `xpc-signature` | also Silo's signature, on an XPC path beside the socket |

| Test and outcome | Leaves possible |
|---|---|
| 20 `other-user-not-refused` | `xpc-signature` |

Tests 07 give evidence (their numbers and facts) and rule out no option.

## `when-the-engine-stops`

Do computers outlive the engine?

- Source: plan, open; #370 assumes a helper per computer, a launchd job of its own (finding 4)
- Tests: 09 (Start a computer from launchd: user agent, system daemon, nobody logged in), 36 (What a computer's connections meet on gvproxy and on Apple's NAT, and a restart of the stack)

| Option | Means |
|---|---|
| `helper-outlives` | a helper per computer, a launchd user agent of its own, which outlives the engine |
| `engine-holds` | no helper of its own: the engine holds main, and a crash of the engine ends it |

| Test and outcome | Leaves possible |
|---|---|
| 09 `agent-refused` | `engine-holds` |

Tests 36 give evidence (their numbers and facts) and rule out no option.

## `what-stop-ends`

How long does a stop of a busy macOS main take, and does it leak a place?

- Source: plan, open; #370 follows #352 file 05 (30 s after requestStop, then stop)
- Tests: 12 (Twenty stops of a busy macOS computer)

Measured, not chosen: the verdict is the measures of the tests above.

## `neutral-core`

Is a refused start of a computer a fact the core must take?

- Source: plan, open
- Tests: 12 (Twenty stops of a busy macOS computer)

Measured, not chosen: the verdict is the measures of the tests above.

## `own-cost-targets`

Against which idle reference and which flush are Silo's own costs set?

- Source: plan, open: measured on polarzero's Mac
- Tests: 06 (Cost of a forced write, and overwrites after a clone), 16 (An idle computer for a night), 36 (What a computer's connections meet on gvproxy and on Apple's NAT, and a restart of the stack), 42 (Throughput over the datagram socket a Mac gives gvproxy, at MTU 1500 and the largest the framework takes)

Measured, not chosen: the verdict is the measures of the tests above.

## `silo-is-light`

What does an idle macOS computer cost the Mac?

- Source: plan, open: measured
- Tests: 16 (An idle computer for a night)

Measured, not chosen: the verdict is the measures of the tests above.

## `record-speed-cost`

What does keeping the record cost in speed?

- Source: plan, open: measured
- Tests: 06 (Cost of a forced write, and overwrites after a clone), 16 (An idle computer for a night), 45 (What a disk keeps of the record when its power is cut: F_FULLFSYNC against fsync)

Measured, not chosen: the verdict is the measures of the tests above.

## `record-damaged-at-start`

When the recorder starts on a record whose end is damaged, does it keep recording or stop until you look?

- Source: #386, draft: how often damage other than an unfinished last line happens is what test 45 measures
- Tests: 45 (What a disk keeps of the record when its power is cut: F_FULLFSYNC against fsync)

| Option | Means |
|---|---|
| `keep-recording` | the recorder cuts the unfinished last line and goes on, noting the cut in the record |
| `stop-until-you-look` | the recorder stops on any damage and waits for you |

Tests 45 give evidence (their numbers and facts) and rule out no option.

## `reaching-a-computer`

How do the programs on your Mac reach main?

- Source: plan, open
- Tests: 08 (Claude desktop and the Codex app through an included SSH entry with fd passing), 20 (The engine's socket: who connected, and what exec changes)

| Option | Means |
|---|---|
| `include-file` | one Include line in ~/.ssh/config for Silo's own file of entries |
| `host-lines` | Silo writes each computer's Host lines into ~/.ssh/config itself |

| Test and outcome | Leaves possible |
|---|---|
| 08 `codex-ignores-include` | `host-lines` |

Tests 20 give evidence (their numbers and facts) and rule out no option.

## `computer-reached-over-ssh`

What works in main over SSH, for you and for agent programs?

- Source: plan, open
- Tests: 02 (Claude Code and Codex signed in over SSH: reconnect, restart, checkpoint, fork), 08 (Claude desktop and the Codex app through an included SSH entry with fd passing), 11 (Documents, Desktop and Downloads over SSH), 14 (What provisioning leaves in a new computer)

Measured, not chosen: the verdict is the measures of the tests above.

## `unmodified-agent-programs`

Where in main can agents work when they are reached over SSH?

- Source: plan, open
- Tests: 11 (Documents, Desktop and Downloads over SSH)

Measured, not chosen: the verdict is the measures of the tests above.

## `fork-and-sign-in`

Can a fork leave out an agent program's sign-in on macOS?

- Source: plan, open
- Tests: 02 (Claude Code and Codex signed in over SSH: reconnect, restart, checkpoint, fork)

| Option | Means |
|---|---|
| `leave-out-files` | a fork leaves out the sign-ins agent programs keep in files, as written |
| `says-copied` | a login kept in main's keychain is copied into a fork, and Silo says so |

| Test and outcome | Leaves possible |
|---|---|
| 02 `lasts-in-keychain` | `says-copied` |

## `engine-matches-wanted`

Does the engine keep the computers you want running, with nobody logged in?

- Source: plan, open; test 49 asks it of option B's Linux main
- Tests: 49 (Option B's main run by a launch daemon while you are logged out (#381 L7))

Measured, not chosen: the verdict is the measures of the tests above.

## `runs-outlive-the-terminal`

Does a run started in a session that outlives its terminal keep working, signed in, through the night?

- Source: #348, open; #370 assumes `ssh main.keep` opens tmux in main
- Tests: 02 (Claude Code and Codex signed in over SSH: reconnect, restart, checkpoint, fork), 37 (The Mac sleeps and wakes with main running)

Measured, not chosen: the verdict is the measures of the tests above.

## `mac-kept-awake`

Does Silo hold the Mac awake while main runs, and when?

- Source: #348, open; #370 assumes on power only, and asks whether Apple's framework already holds it awake
- Tests: 37 (The Mac sleeps and wakes with main running), 16 (An idle computer for a night)

| Option | Means |
|---|---|
| `on-power-only` | Silo holds the Mac awake while main runs, on power only, and says what a closed lid does |
| `on-battery-too` | Silo holds the Mac awake on battery too |
| `framework-holds` | Apple's framework already holds the Mac awake while a computer runs; Silo adds no assertion, and says what battery and a closed lid do |

| Test and outcome | Leaves possible |
|---|---|
| 37 `fact vm_holds_awake=yes` | `framework-holds` |
| 37 `fact vm_holds_awake=no` | `on-power-only`, `on-battery-too` |
| 37 `run-lost` | `on-battery-too` |
| 16 `fact vz_holds_assertion=yes` | `framework-holds` |
| 16 `fact vz_holds_assertion=no` | `on-power-only`, `on-battery-too` |

## `who-made-each-change`

What does the record say about a sleep of the Mac, and can its length be measured from main's helper?

- Source: plan, open; #370 writes a sleep with author `device` and measures it with mach_continuous_time
- Tests: 37 (The Mac sleeps and wakes with main running)

Measured, not chosen: the verdict is the measures of the tests above.

## `checkpoint-per-system`

How is a checkpoint of a running macOS main taken?

- Source: plan, open; #370 assumes an APFS clone while main runs if test 04 holds, #365's delta behind NBD otherwise
- Tests: 03 (APFS clone time against fragmentation, with a writer), 04 (Is a clone of a running computer's disk a moment in time, per disk mode), 15 (A real macOS computer under agent-like work, a checkpoint per step), 31 (The checkpoint engine's prototype on APFS: full, cbw, nbd, delta, and the catalog under kill), 32 (Does a macOS computer boot and run from a disk Silo serves over NBD), 33 (Does the guest's TRIM free the blocks of the disk image on the Mac), 48 (Option B's disk.raw on APFS: a clonefile while main writes, and discards (#381 L5, L8))

| Option | Means |
|---|---|
| `clone-running` | an APFS clone of the image and the auxiliary storage, taken while main runs |
| `clone-paused` | an APFS clone, with main paused for it |
| `delta-nbd` | #365's delta: main's disk served by Silo over NBD, each checkpoint the blocks it copied |
| `stopped-only` | no checkpoint of a running macOS main, only of a stopped one, until another way is found |

| Test and outcome | Leaves possible |
|---|---|
| 03 `stall-grows` | `delta-nbd`, `stopped-only` |
| 04 `none-hold` | `delta-nbd`, `stopped-only` |
| 15 `waits-grow` | `delta-nbd`, `stopped-only` |
| 15 `restore-broken` | `clone-paused`, `delta-nbd`, `stopped-only` |
| 31 `shared-clone-free` | `delta-nbd`, `stopped-only` |
| 31 `shared-clone-waits` | `delta-nbd`, `stopped-only` |
| 31 `shared-clone-stalls` | `delta-nbd`, `stopped-only` |
| 31 `not-a-moment` | `clone-running`, `clone-paused`, `stopped-only` |
| 32 `checkpoint-broke` | `clone-running`, `clone-paused`, `stopped-only` |
| 32 `no-boot` | `clone-running`, `clone-paused`, `stopped-only` |
| 48 `torn` | `clone-paused`, `delta-nbd`, `stopped-only` |

Tests 33 give evidence (their numbers and facts) and rule out no option.

## `checkpoint-at-each-turn`

What does a checkpoint at each turn cost a macOS main?

- Source: plan, open: measured
- Tests: 03 (APFS clone time against fragmentation, with a writer), 31 (The checkpoint engine's prototype on APFS: full, cbw, nbd, delta, and the catalog under kill), 32 (Does a macOS computer boot and run from a disk Silo serves over NBD)

Measured, not chosen: the verdict is the measures of the tests above.

## `thinning-checkpoints`

What does a macOS main's checkpoint keep, and what does dropping one cost?

- Source: plan, open; #370 and #365 propose new ages
- Tests: 31 (The checkpoint engine's prototype on APFS: full, cbw, nbd, delta, and the catalog under kill), 33 (Does the guest's TRIM free the blocks of the disk image on the Mac)

Measured, not chosen: the verdict is the measures of the tests above.

## `checkpoint-storage`

How much disk do a macOS main's checkpoints keep, and what happens when the volume fills?

- Source: plan, open
- Tests: 33 (Does the guest's TRIM free the blocks of the disk image on the Mac), 38 (The Mac's volume fills under a running computer), 46 (What deleting a checkpoint frees while a local Time Machine snapshot exists)

Measured, not chosen: the verdict is the measures of the tests above.

## `restore-to-a-checkpoint`

Does a macOS main restored from a checkpoint of a running disk boot clean?

- Source: plan, open (a feature)
- Tests: 15 (A real macOS computer under agent-like work, a checkpoint per step), 04 (Is a clone of a running computer's disk a moment in time, per disk mode)

Measured, not chosen: the verdict is the measures of the tests above.

## `file-changes-from-outside`

How does Silo read main's file changes from outside?

- Source: plan, open; #370 follows #352 file 09: Silo's own reader, never a mount, a gap for what it cannot read
- Tests: 05 (Which blocks two clones differ in, from the Mac's block map), 14 (What provisioning leaves in a new computer), 17 (How often a clone of a busy computer's disk has a broken newest APFS checkpoint), 18 (Leaf-address diff of two clones of a computer with many files (probe only)), 19 (What the APFS reader meets inside a real computer: raw writes, snapshots, a new volume, FileVault), 39 (The contents of compressed files, read from outside the computer)

| Option | Means |
|---|---|
| `own-reader` | Silo's own APFS reader, from outside, a gap for what it cannot read |
| `mounted-clone` | the macOS kernel's APFS, on a mounted clone of the checkpoint |

| Test and outcome | Leaves possible |
|---|---|
| 18 `wrong` | `mounted-clone` |

Tests 05, 14, 17, 19, 39 give evidence (their numbers and facts) and rule out no option.

## `putting-back-a-path`

Can `look` and `put-back` read the contents of a path in a macOS main's checkpoint from outside?

- Source: #348, open; #370 finding 6 gives the reader a second mode for contents, with APFS compression
- Tests: 39 (The contents of compressed files, read from outside the computer)

| Option | Means |
|---|---|
| `reader-reads-contents` | Silo's reader reads the contents of the paths named, decmpfs compression included |
| `mounted-clone` | look and put-back read a mounted clone of the checkpoint |

| Test and outcome | Leaves possible |
|---|---|
| 39 `reads-wrong` | `mounted-clone` |

## `macos-ships-facts-first`

What can be read of a macOS main, from outside and from inside?

- Source: plan, open
- Tests: 19 (What the APFS reader meets inside a real computer: raw writes, snapshots, a new volume, FileVault), 22 (Endpoint Security events inside a computer, from an SSH session)

Measured, not chosen: the verdict is the measures of the tests above.

## `reading-each-agent-program`

How does main's first run make Claude Code and Codex trust Silo's authority?

- Source: plan, open; #373 step 2 adds the authority to main's System keychain if test 21 allows it
- Tests: 21 (Do Claude Code and Codex trust the computer's keychain authorities), 34 (A night of Claude Code and Codex through a boundary that reads them, against an hour without it)

| Option | Means |
|---|---|
| `keychain-authority` | Silo's authority in main's System keychain, added over SSH by the first run |
| `per-program-settings` | each program's own setting (NODE_EXTRA_CA_CERTS, CODEX_CA_CERTIFICATE) in its environment |

| Test and outcome | Leaves possible |
|---|---|
| 21 `claude-only-keychain` | `per-program-settings` |
| 21 `codex-only-keychain` | `per-program-settings` |
| 21 `neither-keychain` | `per-program-settings` |
| 21 `keychain-needs-a-person` | `per-program-settings` |

Tests 34 give evidence (their numbers and facts) and rule out no option.

## `recognising-a-conversation`

How is a conversation recognised in Claude Code's and Codex's traffic?

- Source: plan, open; #373 proposes the session id each program sends
- Tests: 34 (A night of Claude Code and Codex through a boundary that reads them, against an hour without it)

| Option | Means |
|---|---|
| `by-session` | by the conversation id each program sends with every call (#373) |
| `by-content` | by content, as the entry is written |

| Test and outcome | Leaves possible |
|---|---|
| 34 `conversations-split` | `by-content` |

## `provider-sees-silos-connection`

Do the providers treat a subscription's night differently through Silo's boundary?

- Source: plan, open: found out over real use
- Tests: 34 (A night of Claude Code and Codex through a boundary that reads them, against an hour without it)

Measured, not chosen: the verdict is the measures of the tests above.

## `model-traffic-read`

Can a night of Claude Code and Codex on their plans be read whole through the boundary, in a macOS main?

- Source: plan, open (a feature); #373 makes it part of the first build
- Tests: 34 (A night of Claude Code and Codex through a boundary that reads them, against an hour without it), 21 (Do Claude Code and Codex trust the computer's keychain authorities), 35 (Silo's gvproxy (#363's seal, grown in #380) with a macOS computer: what root inside reaches, sealed and closed)

Measured, not chosen: the verdict is the measures of the tests above.

## `denied-tool-call-mechanism`

Can a probe inside a macOS computer use Endpoint Security?

- Source: plan, open (second build)
- Tests: 22 (Endpoint Security events inside a computer, from an SSH session)

Measured, not chosen: the verdict is the measures of the tests above.

## `xcode-in-a-computer`

Can Xcode be copied into a computer and set up over SSH?

- Source: #361, open
- Tests: 23 (Xcode copied in from the Mac, and the iOS runtime downloaded in the computer), 29 (Xcode's agent bridge used over SSH, and who answers its approval)

Measured, not chosen: the verdict is the measures of the tests above.

## `computers-macos-screen`

Does main need its user logged in on its own screen, for simulators and for Xcode's agent bridge?

- Source: plan, open (a feature)
- Tests: 24 (A simulator booted and used over SSH, with and without a user on the screen), 29 (Xcode's agent bridge used over SSH, and who answers its approval)

Measured, not chosen: the verdict is the measures of the tests above.

## `simulators-for-parallel-agents`

How many simulators does one computer run, and does each need Silo's certificate?

- Source: #361, open
- Tests: 24 (A simulator booted and used over SSH, with and without a user on the screen), 25 (How many booted simulators fit in one computer), 27 (Which certificates an app in a simulator trusts)

Measured, not chosen: the verdict is the measures of the tests above.

## `apple-keys-in-a-computer`

Can a development identity and App Store Connect key serve agents with nobody at the screen?

- Source: #361, open
- Tests: 26 (codesign in a computer, over SSH, with no dialog), 28 (App Store Connect tools through a boundary that re-signs their tokens)

Measured, not chosen: the verdict is the measures of the tests above.

## `registry-traffic-read`

Do simulators trust the authority the computer trusts?

- Source: plan, open
- Tests: 27 (Which certificates an app in a simulator trusts)

Measured, not chosen: the verdict is the measures of the tests above.

## `builds-on-your-iphone`

Can an agent's build reach your iPhone from inside a computer?

- Source: #361, open
- Tests: 30 (An iPhone passed through USB to a computer)

Measured, not chosen: the verdict is the measures of the tests above.

## `sizing-target`

How much memory does main need for an iOS developer?

- Source: plan, open
- Tests: 25 (How many booted simulators fit in one computer)

Measured, not chosen: the verdict is the measures of the tests above.

## `record-file-format`

How does Silo flush what it keeps on a Mac (the record, the spend journal, the team ledger) before it says a thing is kept, and what does that cost?

- Source: plan, taken by agents; #394 found Zig's fsync is not F_FULLFSYNC on macOS, and research 2026-11-25-durability-on-a-mac measures the flushes and a power cut through a disk that loses its cache
- Tests: 50 (What Silo's own writers wait for on APFS: an acknowledgement with F_FULLFSYNC, F_BARRIERFSYNC or fsync), 51 (A power cut under the record's writer: a macOS computer on a disk that loses its cache)

Measured, not chosen: the verdict is the measures of the tests above.

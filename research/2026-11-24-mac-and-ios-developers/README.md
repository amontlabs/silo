# Mac and iOS developers in a macOS computer

An agent's research, 24 November 2026, for polarzero to review. It is about one group of users:
developers who build apps for Apple's systems and want their agents to do it in main. It covers
what those agents need that a Linux computer cannot give, what breaks for them today, where the
plan already answers it, and where it does not.

Nothing here ran on a Mac. What could be tried on Linux was tried, and is said so where it
appears. Everything else is marked as found in a source, or as a test for the Mac test kit.

The plan changes in the same pull request are notes and new entries with no verdict. They are
listed under "Where this meets the plan".

## In short

- **The daily loop needs no Apple secret at all.** Building, running in a simulator and running
  tests sign the app with an ad hoc signature, which needs no certificate and no Apple account.
  A macOS main with Xcode and one simulator runtime is enough for most of what an agent does in
  an iOS project.
- **The hard part is getting Xcode in.** Xcode is downloaded with an Apple Account, behind
  two-factor sign-in, and the Mac App Store cannot be signed into inside a virtual machine.
  Simulator runtimes, on the other hand, come from Apple's public CDN with no account. Measured
  from Apple's own index: the iOS 27.3 runtime for Apple silicon is 9.0 GB. The way that works
  with nobody clicking is to bring in the Xcode you already have on your Mac, checked against
  Apple's signature.
- **Parallel agents fight over simulators, not over the computer.** The two macOS computers Apple
  allows per Mac matter less than it seems, because the parallel work of an iOS developer happens
  inside one macOS system: several agents, each with its own simulator and its own build folder.
  When they share, they overwrite each other's app and lock each other's build database. Each
  booted simulator holds about 3.4 GB. This is the most common complaint found, and it is new to
  the plan.
- **App Store Connect keys can be kept out of the computer.** The tools that talk to App Store
  Connect (notarization, TestFlight uploads, automatic signing in `xcodebuild`) sign a short token
  with a key inside the computer, so the placeholder of `secret-placeholder` cannot work for them.
  The boundary can sign the token again with the real key instead. The prototype in
  [`prototype/`](prototype/) does this on Linux, and refuses the cases it should. Codex's network
  proxy already does the same for AWS request signatures.
- **The key that signs a release should never enter a computer.** A Developer ID key in a
  computer lets an agent sign anything as you. Apple's cloud signing and a signing request that
  waits for you on your Mac both keep it out.
- **Agents cannot reach your iPhone from a computer.** macOS 27 adds USB passthrough to
  virtual machines, but iPhones are reported not to connect. Running a build on your phone means
  the app comes out to your Mac and is installed from there.
- **Xcode's own agent bridge prompts on every connection.** In a computer of its own, that prompt
  is on the computer's screen, where an agent or Silo can answer it. On your Mac it is yours to
  click each time.

## Who they are, and why they come first

The business research of 24 November (kept with polarzero's private research) found that
developers who build for Apple's systems are where Silo has no free substitute. Linux sandboxes,
local or in the cloud, have no Xcode and no simulator. What these developers use instead is a
second Mac, a hosted Mac, or their own Mac with the agent's checks turned off.

What they say, in their own words:

- "Agents pointed at the same simulator interfere with each other." In one test, two agents
  overwrote each other's builds. A developer's issue for an agent orchestrator, opened on
  4 October 2026, lists: "two sessions booting/installing on the same simulator race, and the same
  bundle id overwrites the other session's app and state"; "every booted simulator costs several
  GB; 4–5 parallel sessions each booting one bring the Mac to its knees"; "a queued prompt from
  one session can block others" (the keychain asking for codesign); shared DerivedData giving
  "build.db database is locked". ([shepherd#2751](https://github.com/erwins-enkel/shepherd/issues/2751))
- Three tools were made in the last two months for this one problem: xcfs, a Claude Code plugin
  that gives each session its own simulator and DerivedData, and leases in shepherd. xcfs measured
  on an M1 Max with 64 GB: three agents on new worktrees wait 284 to 387 s for their first builds;
  each booted simulator holds about 3.4 GB; a batch of three worktrees leaves 4.6 to 8.9 GB of
  DerivedData behind; in earlier runs, 3 of 18 test runs were killed by another agent's.
  ([xcfs](https://github.com/PatrickSCLin/xcfs), the author's own benchmark)
- From a developer closing the loop with Claude Code (December 2025): the agent deletes all of
  DerivedData when stuck, which "usually doesn't fix the underlying problem" and breaks Swift
  packages for a while; a launch without `--terminate-running-process` silently does not relaunch,
  and the agent "will start thrashing"; device logs need `sudo log collect`, which the agent
  cannot run. ([twocentstudios](https://twocentstudios.com/2025/12/27/closing-the-loop-on-ios-with-claude-code/))
  In a computer of its own, the agent has `sudo` (`full-access-inside`).
- Claude Code's and Codex's own sandboxes are reported to block CoreSimulator, so simulator tests
  fail inside them (anthropics/claude-code#36611, openai/codex#4987, cited in shepherd#2751, not
  checked here). That is one more reason these developers run with every check off.
- Agents that edit `project.pbxproj` by hand corrupt it, and a corrupted project does not open.
  The advice that spread is to let only Xcode or a generator (XcodeGen, Tuist) write it.
- Xcode 26.3 (February 2026) added Claude Agent and Codex inside Xcode, and `xcrun mcpbridge`
  for agents outside it. The bridge needs Xcode running, and shows an approval dialog for each new
  agent process, with no way to keep the approval. Tools now exist only to click "Allow" with
  AppleScript. ([Cursor forum](https://forum.cursor.com/t/repeated-macos-tcc-prompt-allow-cursor-vscode-to-access-xcode-when-using-xcode-tools-mcp/158553),
  [Swift forums](https://forums.swift.org/t/reducing-the-friction-of-using-xcode-s-mcp-bridge-from-external-coding-agents/85353))

So the case for Silo here is concrete: agents with `sudo` and nothing asking, on a Mac of their
own whose screen and prompts are theirs, with simulators that parallel agents do not share, and a
way back when one of them breaks the project.

## What an iOS project needs, one piece at a time

Each piece says what is true today, what Silo can do, and which entries of the plan it touches.
"Source" means read in the linked page; "measured" means done in this research; "Mac test" means
the kit should answer it, and the test is listed further down.

### Xcode in a computer

- Xcode 27.2 is the current release (29 October 2026). It needs macOS 26.6 or later.
  (Source: xcodereleases.com's data, read on 24 November.)
- Apple's download of Xcode needs an Apple Account with two-factor sign-in
  (`download.developer.apple.com`). The Mac App Store cannot be signed into in a virtual machine,
  even on macOS 15 and later, where iCloud can. (Source: Parallels' and Apple's pages on Apple
  Account in virtual machines.) So a computer cannot fetch Xcode by itself without your Apple
  Account inside it.
- Simulator runtimes do not need an account: `xcodebuild -downloadPlatform iOS` fetches them from
  Apple's public CDN. Their sizes, read from Apple's index
  (`devimages-cdn.apple.com/downloads/xcode/simulators/index2.dvtdownloadableindex`) on 24
  November:

  | Runtime, Apple silicon only | Download |
  |---|---|
  | iOS 27.0 / 27.2 / 27.3 | 8.4 / 8.3 / 9.0 GB |
  | watchOS 27.3 | 4.0 GB |
  | tvOS 27.3 | 3.8 GB |
  | visionOS 27.3 | 7.7 GB |

  The universal runtimes are about 2 GB larger (iOS 26.4: 8.7 GB against 10.9 GB). Silo should take
  the Apple silicon ones.
- Pinning a runtime version on the command line has been unreliable since iOS 26 ("iOS 26.0 is not
  available for download"), and downloads stall in Xcode's own window in many forum threads.
- Xcode's own size is not published for 27. Xcode 26's Apple silicon build cut the download by
  about 22% and the install by about 25%. Mac test 23 measures it.

What Silo can do: bring Xcode in from your Mac. You already have it, or you download it once with
your own account on your Mac. Silo copies `/Applications/Xcode.app` (or a `.xip` you choose) into
the computer, checks Apple's signature on it, accepts the license and runs the first-launch step
(`xcodebuild -license accept`, `xcodebuild -runFirstLaunch`), all over SSH. Then it fetches the
runtime the project needs from Apple's CDN. A copy of Xcode is an ordinary import
(`files-imported-from-your-mac`), and Apple's signature is a fact that it was not changed.

Silo can then keep that installed state the way it keeps the installed system
(`computer-image`): later macOS computers start from a clone that already has Xcode and the
runtime. On APFS, a clone shares those blocks, so a fork or a task does not pay the 20 or more GB
again. This also bears on `shared-cache`: a runtime is signed by Apple, so a store that only Silo
fills, checked against Apple's signature, lets no computer place a file another one runs.

New entry: `xcode-in-a-computer`. Touches `computer-image`, `first-run-time-macos`,
`files-imported-from-your-mac`, `shared-cache`.

### Simulators

- The simulator is not a virtual machine; it is a set of processes on the Mac. So the lack of
  nested virtualization in Apple's framework does not stop it. Tart's images and hosted Mac
  services run simulators inside macOS virtual machines every day.
- Graphics in a macOS virtual machine go through a paravirtualized Metal driver to the host's GPU.
  Tests from 2023 found good performance; a 2026 report says the guest announces a reduced set of
  Metal features. No benchmark of the simulator itself in a virtual machine was found. Mac test 24
  measures boot time and a screenshot that is not blank.
- A simulator needs a logged-in graphical session (Aqua). Apple's DTS: "Running GUI code in an SSH
  session is always a bad idea", and "To ensure that an Aqua session is created for an ssh login,
  you must have a user logged in". So main must log its user in on its own screen at start, and
  agents reach that session over SSH. Mac test 24 checks a simulator booted over SSH with and
  without that session. This also matters for UI tests (XCUITest), which drive the foreground.
- Xcode 27 replaces Simulator.app with Device Hub, and `devicectl` now drives simulators as well as
  devices. `simctl` keeps working. (Source: Bitrise, June 2026.)

**Parallel agents.** This is where the two-macOS-computer limit and the plan meet real use. The
plan sizes main for "about 16 agents working at once in main, on a Mac with 32 GB"
(`sizing-target`). An iOS developer's agent wants a booted simulator to check its work. At about
3.4 GB each, four booted simulators already hold 13.6 GB, before Xcode builds, which peak at
several GB each. Sixteen agents with a simulator each do not fit on any Mac.

So, inside main, simulators have to be leased: each agent gets its own simulator (a clone of a
base device made with `simctl clone`) and its own DerivedData, for as long as it uses them, and
the number booted at once has a maximum set from memory. When none is free, an agent waits for
one, and is told so. This is what xcfs, the Claude Code plugin and shepherd each build. Silo can
do it better than they can for one reason: it can size main's memory for the maximum and show it
(`running-computers-maximum`), and it can see from outside which simulators are booted.

Who does it is a real choice. Silo's core knows no agents (`core-knows-no-agents`). The lease is
a program inside the computer. It fits as a piece of a recipe for Apple development, which Silo
ships (`silo-ships-recipes`, `recipe-pieces`), and which sets the agent programs up through their
documented surfaces (hooks, environment). New entry: `simulators-for-parallel-agents`. Touches
`sizing-target`, `running-computers-maximum`, `several-agents-in-main`.

### Running on an iPhone

- Apple's framework had no USB passthrough before macOS 27. macOS 27 adds it
  (`VZUSBPassthroughDevice` with the Accessory Access framework). On the macOS 27 beta, a
  developer reported "unable to connect iphone, hd cameras and microphones", while USB drives
  worked. Apple's DTS moved the question to another thread; no fix was found.
  ([Apple forums 825379](https://developer.apple.com/forums/thread/825379))
- Xcode can also reach a device over the network once paired, through mDNS and an IPv6 tunnel.
  Main's network goes through Silo's boundary, not your local network, so that path is closed by
  design.
- A paired iPhone takes one install or debug session at a time.

What Silo can do: when an agent wants its build on your phone, the built app leaves the computer
through a channel (`files-through-a-channel`), and Silo installs it on your Mac with
`xcrun devicectl device install app`, after you say yes. The agent gets back what happened. A
debugger attached to the phone stays on your Mac. New entry: `builds-on-your-iphone`. Touches
`files-through-a-channel`, `asks-are-native`.

### Signing, and which keys may enter a computer

There are five kinds of Apple key an iOS or Mac developer has. They are not equal:

| Key | What it does | What an agent could do with it |
|---|---|---|
| None (ad hoc signature) | Builds that run in the simulator, and Mac builds that run locally | Nothing |
| Apple Development certificate and key | Signs builds for your registered devices | Install its own builds on your registered devices. Revocable, and Xcode makes one per Mac |
| Apple Distribution (App Store) | Signs a build for TestFlight and the App Store | Little alone: an upload also needs App Store Connect access |
| Developer ID Application / Installer | Signs Mac software outside the App Store, which Gatekeeper then trusts | Sign and notarize malware as you. The worst case of all |
| App Store Connect API key (`.p8`) | Uploads, TestFlight, certificates, profiles, users, depending on its role | With an Admin key: revoke your certificates, create users, push a build to testers |

What is true of each, from the sources:

- `codesign` over SSH fails with `errSecInternalComponent` when the keychain is locked; logging in
  over SSH does not unlock it. The usual fix is a separate keychain, `security unlock-keychain`
  in the same session and `set-key-partition-list`. When it goes wrong, macOS shows a dialog that
  nobody answers, and the agent hangs. Mac test 26 checks a signing in a computer, over SSH, with
  a self-signed identity, so no Apple account is needed.
- `xcodebuild -allowProvisioningUpdates -authenticationKeyPath … -authenticationKeyID …
  -authenticationKeyIssuerID …` lets `xcodebuild` create and update profiles and certificates
  with an App Store Connect key instead of your Apple Account. With an Admin key, it can also use
  Apple's cloud-managed distribution certificates, whose private key the computer never holds.
  Developers report that cloud signing with Developer ID and an API key does not work, and that
  the cloud-signing permission cannot be given to a key below Admin.
- `codesign` works with a key on a smart card through CryptoTokenKit (YubiKey's PIV, DigiCert's
  token). So a key can sign without being in the keychain as a file.

What Silo can do, per key:

- **Simulator and local builds:** nothing to do. This is most of an agent's work.
- **Development certificate:** let Xcode make one inside the computer, with the App Store Connect
  key re-signed at the boundary (below). It belongs to that computer, and you can revoke it.
- **Distribution for the App Store:** Apple's cloud signing, through the same re-signed key. No
  distribution key enters the computer.
- **Developer ID:** never in a computer. The computer builds without signing. Signing happens on
  your Mac, or through a token in the computer whose key operations go out to your Mac, and each
  signing waits for your yes, with what is signed shown. A signing is then a fact in the record.
- **App Store Connect key:** a placeholder key in the computer, re-signed at the boundary.

New entry: `apple-keys-in-a-computer`. Touches `secret-placeholder`, `secret-values`,
`secrets-held-outside`, `asks-are-native`.

### App Store Connect, TestFlight and notarization: re-signing at the boundary

The App Store Connect API wants a JWT on each request: header `alg ES256`, `kid` the key's id,
`typ JWT`; claims `iss` (the team's issuer id, or `sub: "user"` for an individual key), `iat`,
`exp` at most 20 minutes later, `aud: "appstoreconnect-v1"`, and an optional `scope`. Apple
documents `scope` as a list of `GET` requests (a path and an optional query), and refuses a token
whose scope matches no entry. (Source: Apple's "Generating tokens for API requests", read on 24
November.) `notarytool`, `altool`, fastlane and `xcodebuild` with `-authenticationKeyPath` build
this token themselves from the `.p8` file.

`secret-placeholder` answers that "a key that signs requests … cannot be swapped in; how those are
handled is to find out". For this key, here is a way:

1. The computer holds a placeholder: a real P-256 key, made by Silo, with a placeholder key id and
   issuer. It is worthless anywhere else.
2. The tools sign their tokens with it, as usual. Nothing in the computer is configured
   differently.
3. The boundary already ends the encrypted connection to the hosts that get a secret
   (`ending-the-encrypted-connection`). On a request to `api.appstoreconnect.apple.com` that
   carries a token, it checks that the placeholder signed it, that the audience is App Store
   Connect, and that it lives no more than 20 minutes.
4. It signs the same claims again with the real key, under the real key id and issuer. On a `GET`,
   it adds a `scope` that names only that one request, so the token Apple gets is good for nothing
   else.
5. A request to any other host with the placeholder is refused, like any placeholder.

**Tried on Linux.** [`prototype/asc_resign.py`](prototype/asc_resign.py) runs a fake App Store
Connect that trusts only the real key, a boundary that re-signs, and the token a tool would make.
Run on 24 November with Python 3.13 and `cryptography` 50:

```
ok  200 (want 200)  program in the computer, through the boundary
ok  401 (want 401)  placeholder token sent straight to Apple
ok  403 (want 403)  placeholder token to another host
ok  403 (want 403)  token the agent made with a key of its own
ok  403 (want 403)  token asking for a day
ok  403 (want 403)  token for another audience
```

What it does not protect, said plainly: while the computer runs, an agent can make any call the
key's role allows, through the boundary, as with any swapped secret. Two things narrow that. The
key's role: an App Manager or Developer key cannot revoke certificates or add users. And the
boundary sees each call's method and path, since it opened the connection, so a rule can deny
`DELETE` on `/v1/certificates`, and each call is a fact in the record.

Codex's network proxy does the same for AWS: it pairs a placeholder access key id with a real one
and "re-signs for the configured HTTPS" host, keeping the client's timestamp and signed headers,
and denies presigned URLs and signed streams. (Source: Codex 0.172.0's network proxy, read in the
project's files.) So re-signing is a known practice, and Silo would take it as the model for every
key that signs a request: App Store Connect first, AWS next.

What a Mac must still show (test 28): which hosts each tool really calls (notarization uses
`appstoreconnect.apple.com/notary/v2`; `xcodebuild`'s provisioning may use another host and
audience), and whether each tool accepts Silo's certificate from the system store.

New entry: `signing-keys-re-signed`. Touches `secret-placeholder`, `secret-values`,
`ending-the-encrypted-connection`, `what-was-sent-to-a-host`.

### Packages: Swift Package Manager and CocoaPods

- Swift packages come from Git repositories, mostly on GitHub, over HTTPS or SSH. A private package
  over HTTPS uses a token, which the placeholder covers. Over SSH, it uses a key, which it does not.
  The advice for a project in Silo: HTTPS for private packages.
- `package-age-rule` takes a package's age "from what the registry itself says". Swift packages
  have no registry for most projects: a version is a Git tag, and a tag's date is written by
  whoever made the commit, so it is not a fact. The age Silo could trust is the date GitHub first
  saw the tag, from GitHub's API, which is a report of GitHub's. Swift's package registry
  (`swift package-registry`, SE-0292) exists but few use it.
- CocoaPods' trunk becomes read-only on 2 December 2026. Existing pods keep working; no new
  version is published there. So a rule on package age has nothing new to judge on CocoaPods after
  that date, and new projects use Swift packages.
- Xcode resolves packages into DerivedData, per project. Several agents on the same repository in
  worktrees each resolve again unless they share a cache, which is the same question as
  `shared-cache`, inside one computer this time, where it does not apply.

Touches `registry-traffic-read`, `package-age-rule`, `minimum-package-age`.

### The certificate inside simulators

The boundary ends the encrypted connection to the hosts that get a secret, with a certificate the
computer trusts. A simulator has its own trust store, separate from macOS's. An app in a simulator
that calls a host where Silo swaps a secret (a debug build that calls Stripe's test API with a
placeholder key) fails, unless Silo's certificate is added to each simulator
(`xcrun simctl keychain <device> add-root-cert`). A simulator cloned for an agent must get it too.
Mac test 27 checks it. Touches `registry-traffic-read` and the trust-store test 21 of the kit.

### Xcode's own agents, and its prompts

- Xcode 26.3 and later run Claude Agent and Codex inside Xcode, and offer `xcrun mcpbridge` to
  agents outside it. The bridge needs Xcode open, and Xcode asks to allow each new agent process.
- In main, Xcode runs on main's own screen. The approval dialog appears there, not on your Mac, and
  can be answered by an agent driving that screen (`agents-drive-the-screen`) or by Silo's setup
  for the computer. That is a real difference for these developers: on their own Mac, every new
  agent process makes them click.
- The same holds for the other prompts of a fresh Mac that block an agent: the keychain asking for
  `codesign`, privacy prompts for UI tests, Developer Mode (`DevToolsSecurity -enable`).

Mac test 29 checks that an agent over SSH can use `mcpbridge` with Xcode open on the computer's
screen, and how the prompt is answered. Touches `computers-macos-screen`, `windows-stay-inside`.

### The two macOS computers per Mac

- The license allows "up to two (2) additional copies or instances" of macOS in virtual machines
  on each Mac you own, "except as otherwise provided in writing" by Apple. Apple's DTS, September
  2026: the framework enforces this for macOS guests, and "has no fixed cap for Linux VMs". DTS
  declined to say how to get a written exception.
  ([Apple forums 845455](https://developer.apple.com/forums/thread/845455))
- With main on macOS, one more macOS computer can run: a fork, or a task's macOS computer
  (`task-macos-limit`). Parallel iOS work therefore happens inside main, which is why the simulator
  lease above matters more than the limit.
- `tasks-test-your-changes` makes "a fresh macOS computer" for each test. For an iOS project, a
  fresh computer has to get Xcode and a runtime and build from cold: over 20 GB that a clone could
  share, and minutes of build that DerivedData would save. A task's macOS computer made as a fork of
  a checkpoint of main, or from the kept image with Xcode, starts ready. That is a note for that
  entry, not a change made here.

### Disk and memory, for one iOS developer

From the sources above, before the Mac tests replace them with measurements:

| | Disk | Memory |
|---|---|---|
| macOS installed in main | the kit's test 00 measures it | |
| Xcode 27, Apple silicon | not published; test 23 | |
| iOS 27.3 simulator runtime | 9.0 GB download (measured from Apple's index) | |
| DerivedData per worktree | 1.5 to 3 GB (xcfs: 4.6 to 8.9 GB per batch of three) | |
| A booted simulator | | about 3.4 GB (xcfs) |
| A cold build of a medium app | | several GB at peak (xcfs: 21 to 26 GB to warm three slots at once) |

So main for an iOS developer wants more memory than the 16 GB load of `sizing-target`: one agent,
one booted simulator and a build already take most of 8 GB given to main.

## Where this meets the plan

| Entry | What this research adds |
|---|---|
| `secret-placeholder` | A way for keys that sign requests: re-signing at the boundary. Tried on Linux for App Store Connect; Codex ships it for AWS |
| `sizing-target` | Challenge: 16 agents in main cannot each have a simulator. The heavy load needs simulators leased and a maximum booted |
| `tasks-test-your-changes`, `task-macos-limit` | A fresh macOS computer for an iOS project pays Xcode, a runtime and a cold build every time |
| `computer-image` | Keep the installed state with Xcode and a runtime, as the system is kept |
| `shared-cache` | Simulator runtimes and Xcode are signed by Apple, which fits the store only Silo fills |
| `registry-traffic-read`, `package-age-rule` | Swift packages have no registry date; CocoaPods trunk is read-only from 2 December 2026 |
| `computers-macos-screen` | Xcode's agent bridge, UI tests and simulators need main's user logged in on its screen |
| `main-on-macos-first` | Apple's DTS confirms the cap is on macOS guests only |

New entries, each with "The choice" in its notes and no verdict:

| Entry | Part | Asks |
|---|---|---|
| `xcode-in-a-computer` | computers | How does Xcode get into a macOS computer? |
| `simulators-for-parallel-agents` | recipes | How do several agents in main use simulators at once? |
| `builds-on-your-iphone` | computers | How does an agent run its build on your iPhone? |
| `apple-keys-in-a-computer` | boundary | Which of your Apple keys may enter a computer? |
| `signing-keys-re-signed` | boundary | How does a computer use a key that signs each request? |

## Mac tests for the kit

These follow the format of `research/mac-test-kit/` (pull request #359): one question, a few
outcomes, and the decision each outcome moves. Five are written as scripts in
[`kit-tests/`](kit-tests/), for `research/mac-test-kit/tests/` once the kit is merged. Three need a
person or an account and are described only.

| Id | Question | Tier | Moves |
|---|---|---|---|
| 23 | Does Xcode copied from the Mac into a computer pass Apple's signature check and first launch over SSH, and how large is it? How long does the iOS runtime take to download there? | vm | `xcode-in-a-computer`, `first-run-time-macos` |
| 24 | Does a simulator boot, run an app and give a screenshot that is not blank, over SSH, with and without the user logged in on the screen? | vm | `computers-macos-screen`, `computer-image` |
| 25 | How much memory does each booted simulator take in a computer, and how many fit before the computer pages? | long | `simulators-for-parallel-agents`, `sizing-target` |
| 26 | Does `codesign` with an identity in a keychain made over SSH sign without a dialog? | vm | `apple-keys-in-a-computer` |
| 27 | Does an app in a simulator trust a certificate the computer trusts, and after `simctl keychain add-root-cert`? | vm | `registry-traffic-read`, `secrets-held-outside` |
| 28 | With a real App Store Connect key on the Mac and the placeholder in the computer, do `notarytool history`, `altool --list-apps` and `xcodebuild -allowProvisioningUpdates` work through the re-signing boundary? Which hosts and audiences do they use? | guided | `signing-keys-re-signed` |
| 29 | Can an agent over SSH use `xcrun mcpbridge` with Xcode open on the computer's screen, and how is its approval answered? | guided | `computers-macos-screen` |
| 30 | On macOS 27, does an iPhone attach to a computer with `VZUSBPassthroughDevice`, and can Xcode install on it? | guided | `builds-on-your-iphone` |

28 needs a real key and must never put it in the computer; the kit would run the prototype's
boundary on the Mac. 30 needs the kit's VM helper to attach a USB device, which it does not do yet.

## What was not done

- Nothing ran on a Mac. Every number about a computer is from a source and needs the tests above.
- The re-signing prototype has no TLS: the boundary's ending of the connection is decided
  elsewhere and is tested by the kit's 21. It checks the logic, not the network path.
- Xcode's own size was not found for 27.
- Business material (prices, how many such developers, what they pay) stays in polarzero's private
  research and Linear, not here.

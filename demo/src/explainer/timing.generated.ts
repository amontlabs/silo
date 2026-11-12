// Written by scripts/explainer.mjs from the recorded narration. Do not edit.
export const TIMING: Record<string, { seconds: number; spoken: string }> = {
  "title--title.wav": {
    "seconds": 3.422,
    "spoken": "This is Silo: what it is, how it is built, and where its limits are."
  },
  "problem--reads.wav": {
    "seconds": 6.188,
    "spoken": "A coding agent reads things you did not write: issues, pull request comments, web pages, dependencies."
  },
  "problem--inject.wav": {
    "seconds": 3.39,
    "spoken": "Any of them can carry instructions. That is prompt injection."
  },
  "problem--reach.wav": {
    "seconds": 5.076,
    "spoken": "And the agent usually runs on your machine, next to your files, your network and your tokens."
  },
  "problem--sandbox.wav": {
    "seconds": 4.517,
    "spoken": "The sandboxes built into agent tools limit which files and hosts it can touch."
  },
  "problem--token.wav": {
    "seconds": 6.311,
    "spoken": "None of them narrows what a credential is allowed to do. A token that can push to every repository still can."
  },
  "answer--vm.wav": {
    "seconds": 5.726,
    "spoken": "Silo gives each agent its own computer: a virtual machine, with its own kernel and its own disk."
  },
  "answer--pick.wav": {
    "seconds": 3.119,
    "spoken": "It holds only the repositories and credentials you pick."
  },
  "answer--hostile.wav": {
    "seconds": 3.463,
    "spoken": "Silo assumes the agent inside is hostile, and has root there."
  },
  "answer--two.wav": {
    "seconds": 4.441,
    "spoken": "So it limits two things: what the agent can reach, and what it is authorised to do."
  },
  "answer--extras.wav": {
    "seconds": 4.867,
    "spoken": "Each computer also has a desktop the agent can drive, and checkpoints to go back to."
  },
  "answer--local.wav": {
    "seconds": 3.994,
    "spoken": "All of it runs locally. There is no Silo account and no Silo server."
  },
  "words--device.wav": {
    "seconds": 5.845,
    "spoken": "Five words first. A device is a physical machine that runs Silo: your Mac, or a Linux server."
  },
  "words--computer.wav": {
    "seconds": 3.568,
    "spoken": "A computer is a virtual one, which Silo creates for an agent."
  },
  "words--workspace.wav": {
    "seconds": 4.797,
    "spoken": "The workspace is one folder inside a computer, slash workspace, on its own disk."
  },
  "words--checkpoint.wav": {
    "seconds": 4.8,
    "spoken": "A checkpoint is a saved state of a computer. A fork is a new computer made from one."
  },
  "words--connections.wav": {
    "seconds": 4.079,
    "spoken": "Connections links devices, so one Silo manages computers on another."
  },
  "words--colours.wav": {
    "seconds": 9.798,
    "spoken": "And three colours for the whole film. Blue is your device, which is trusted. Violet is a computer and its agent, which is not. Orange is Silo's engine, in between."
  },
  "map--clients.wav": {
    "seconds": 8.971,
    "spoken": "At the top are the clients: the Silo app, the silo command line, an M C P server for agents, a C I runner, and other apps that embed the engine."
  },
  "map--socket.wav": {
    "seconds": 3.03,
    "spoken": "They all speak one protocol, over one local socket."
  },
  "map--engine.wav": {
    "seconds": 7.057,
    "spoken": "Behind the socket is the engine, a daemon called silo dee. It owns the computers, the credentials and the network rules."
  },
  "map--runtimes.wav": {
    "seconds": 8.211,
    "spoken": "The engine drives the runtimes that run the computers: MicroSandbox for Linux, and Apple's Virtualization framework for mac O S."
  },
  "map--guests.wav": {
    "seconds": 3.108,
    "spoken": "At the bottom are the computers, each with an agent inside."
  },
  "map--plan.wav": {
    "seconds": 8.448,
    "spoken": "We go through it in that order: the engine, the computers, the isolation, and the tools you work with. Then other devices, and the state of the project."
  },
  "status--who.wav": {
    "seconds": 4.693,
    "spoken": "Silo is M I T licensed, built by one person, and still a work in progress."
  },
  "status--versions.wav": {
    "seconds": 6.225,
    "spoken": "The app is at version 0.13, the engine at 0.2, and the protocol at version 7."
  },
  "status--host.wav": {
    "seconds": 5.732,
    "spoken": "The host is a Mac with Apple silicon. The engine also runs on Linux servers, without the app."
  },
  "status--guests.wav": {
    "seconds": 3.78,
    "spoken": "The guests are Ubuntu 24.04 and mac O S."
  },
  "processes--launchd.wav": {
    "seconds": 5.698,
    "spoken": "Part one: the engine. It is started by launch dee when you log in. One engine per user account."
  },
  "processes--app.wav": {
    "seconds": 5.891,
    "spoken": "The app is only a client. It never runs a computer itself, and a test fails if its code ever tries."
  },
  "processes--msb.wav": {
    "seconds": 6.887,
    "spoken": "Each Linux computer is a separate process named M S B: the MicroSandbox runtime, holding one virtual machine."
  },
  "processes--vz.wav": {
    "seconds": 3.498,
    "spoken": "mac O S computers run inside the engine's own process."
  },
  "processes--gateway.wav": {
    "seconds": 4.22,
    "spoken": "Each of those also gets a small gateway process, which is its only network."
  },
  "processes--quit.wav": {
    "seconds": 7.392,
    "spoken": "Because the engine owns the computers, quitting the app does not stop them. Quit asks you: cancel, keep running, or stop and quit."
  },
  "wire--file.wav": {
    "seconds": 4.509,
    "spoken": "The socket is a file, engine dot sock, in the dot silo folder of your home."
  },
  "wire--uid.wav": {
    "seconds": 4.357,
    "spoken": "Only your user can open it, and the engine checks the user of every connection."
  },
  "wire--frame.wav": {
    "seconds": 3.335,
    "spoken": "A message is jason, prefixed with its length in four bytes."
  },
  "wire--hello.wav": {
    "seconds": 7.902,
    "spoken": "The first message must be a handshake. Each side states the protocol versions it supports. The engine supports exactly one: seven."
  },
  "wire--request.wav": {
    "seconds": 5.477,
    "spoken": "After that, a request carries an I D and a method name, and the response carries the same I D."
  },
  "wire--events.wav": {
    "seconds": 5.72,
    "spoken": "The engine also pushes events on the same connection. A computer changed. A job made progress."
  },
  "methods--count.wav": {
    "seconds": 5.136,
    "spoken": "The protocol has 175 methods, each with typed parameters and results."
  },
  "methods--query.wav": {
    "seconds": 3.385,
    "spoken": "Queries read state. Commands change it, and answer at once."
  },
  "methods--job.wav": {
    "seconds": 4.934,
    "spoken": "Jobs are for slow work, like creating a computer. They return a job I D immediately."
  },
  "methods--stream.wav": {
    "seconds": 8.108,
    "spoken": "Streams turn the connection into raw bytes, mostly for S S H and for screens. Subscriptions deliver events and file changes."
  },
  "methods--groups.wav": {
    "seconds": 6.55,
    "spoken": "The methods are grouped by area: computers, checkpoints, changes, guest, display, GitHub, secrets, network, and more."
  },
  "methods--schema.wav": {
    "seconds": 6.115,
    "spoken": "The contract is one Rust crate, and a jason schema generated from it, so any language can have a client."
  },
  "jobs--progress.wav": {
    "seconds": 3.424,
    "spoken": "A job reports progress as events, then one final result."
  },
  "jobs--stored.wav": {
    "seconds": 8.571,
    "spoken": "Jobs are stored in a small sequel light database. If the engine restarts in the middle of one, the client is told so, instead of waiting forever."
  },
  "jobs--ack.wav": {
    "seconds": 2.651,
    "spoken": "The client acknowledges a result once it has it."
  },
  "jobs--lanes.wav": {
    "seconds": 5.829,
    "spoken": "All work passes through one queue. Each computer has its own lane, so two computers work in parallel."
  },
  "jobs--wide.wav": {
    "seconds": 3.553,
    "spoken": "Work that touches the whole device waits for everything else to finish."
  },
  "crates--api.wav": {
    "seconds": 4.617,
    "spoken": "Most of the code is one Rust workspace. silo API is the contract."
  },
  "crates--client.wav": {
    "seconds": 3.664,
    "spoken": "silo-client is a thin client: framing, handshake, events."
  },
  "crates--engine.wav": {
    "seconds": 4.072,
    "spoken": "silo-engine is the engine as a library, with no interface code in it."
  },
  "crates--silod.wav": {
    "seconds": 4.239,
    "spoken": "silo dee wraps it into the daemon: the socket, the dispatcher, the timers."
  },
  "crates--ui.wav": {
    "seconds": 4.26,
    "spoken": "silo U I is the app: a tow-ree shell in Rust, with a React interface."
  },
  "crates--cli.wav": {
    "seconds": 3.423,
    "spoken": "silo C L I is the command line and the M C P server."
  },
  "crates--rest.wav": {
    "seconds": 8.413,
    "spoken": "Around them: helpers shared by the app and the engine, the network gateway, a container helper, and the C I runner, which is a separate Go program."
  },
  "crates--context.wav": {
    "seconds": 10.425,
    "spoken": "The engine touches the outside world through one interface, the engine context: paths, identity, publishing events, the main thread. That is what lets it run without the app."
  },
  "disk--home.wav": {
    "seconds": 6.12,
    "spoken": "State lives in three places. The dot silo folder holds the socket, a lock, and the job database."
  },
  "disk--appdata.wav": {
    "seconds": 5.759,
    "spoken": "Application Support holds the rest: settings, the list of computers, their disks, and the guest image."
  },
  "disk--keychain.wav": {
    "seconds": 6.328,
    "spoken": "Secret values and GitHub tokens are in the Keychain. The settings files hold only names and references."
  },
  "disk--keys.wav": {
    "seconds": 7.659,
    "spoken": "Some keys are plain files that only you can read: S S H keys, and the certificate authority that the Linux proxy signs with."
  },
  "disk--channels.wav": {
    "seconds": 7.719,
    "spoken": "There are two builds, production and Dev. Each has its own folders, its own socket and its own Keychain entries. They never share state."
  },
  "lifecycle--sign.wav": {
    "seconds": 9.289,
    "spoken": "When the engine starts on a Mac, it first checks its own code signature against a requirement in its bundle. A production engine refuses to start without one."
  },
  "lifecycle--lock.wav": {
    "seconds": 2.382,
    "spoken": "Then it takes the lock, so only one can run."
  },
  "lifecycle--open.wav": {
    "seconds": 7.163,
    "spoken": "It converts older saved data and opens the socket. Computers to resume, or to start at launch, follow in the background."
  },
  "lifecycle--update.wav": {
    "seconds": 8.431,
    "spoken": "Updates arrive with the app. Before installing, the engine saves the running computers and exits. The new engine resumes exactly that set."
  },
  "lifecycle--watch.wav": {
    "seconds": 6.157,
    "spoken": "While computers run, a health watch polls every ten seconds, and reports any change that nobody asked for."
  },
  "linux-stack--msb.wav": {
    "seconds": 8.941,
    "spoken": "Part two: the computers. A Linux computer is a stack. The engine calls M S B, MicroSandbox version zero point seven point six."
  },
  "linux-stack--krun.wav": {
    "seconds": 6.073,
    "spoken": "MicroSandbox uses lib K run, a small virtual machine monitor that brings its own Linux kernel."
  },
  "linux-stack--hv.wav": {
    "seconds": 4.433,
    "spoken": "That sits on Apple's Hypervisor framework. On a Linux server, on K V M."
  },
  "linux-stack--own.wav": {
    "seconds": 4.787,
    "spoken": "So every computer has its own kernel, and two disks: a root disk, and the workspace."
  },
  "linux-stack--nofolder.wav": {
    "seconds": 1.951,
    "spoken": "No folder of yours is shared into it."
  },
  "linux-stack--share.wav": {
    "seconds": 8.039,
    "spoken": "With one exception: the runtime's own control folder, mounted read-write at slash dot M S B. It has not had a security review."
  },
  "linux-stack--patches.wav": {
    "seconds": 8.724,
    "spoken": "Silo carries 24 patches on MicroSandbox, each pinned by checksum. They add what Silo needs, like publishing ports on a running machine."
  },
  "image--base.wav": {
    "seconds": 6.978,
    "spoken": "Every Linux computer starts from one image: Ubuntu 24.04, about 350 megabytes."
  },
  "image--inside.wav": {
    "seconds": 6.295,
    "spoken": "It contains Git and the GitHub tools, a desktop with a video streamer, and Koo-ah Driver for computer use."
  },
  "image--noagent.wav": {
    "seconds": 6.82,
    "spoken": "It contains no agent. Claude Code is installed on request, at a pinned version, and Silo never signs in for you."
  },
  "image--fetch.wav": {
    "seconds": 7,
    "spoken": "The image is not in the installer. The engine downloads it once, in the background, and checks it against a pinned checksum."
  },
  "image--pin.wav": {
    "seconds": 5.613,
    "spoken": "A new image version affects only new computers. Existing ones keep the image they were made from."
  },
  "create--config.wav": {
    "seconds": 13.741,
    "spoken": "Creating a computer is a job with visible steps. First its size. The app proposes 8 C P U cores, 32 gigabytes of memory and a 120 gigabyte workspace. The command line defaults are far smaller."
  },
  "create--made.wav": {
    "seconds": 4.257,
    "spoken": "Then the engine waits for the image, and has the runtime create the machine, stopped."
  },
  "create--verify.wav": {
    "seconds": 4.618,
    "spoken": "It verifies the tools, then boots once to set up the desktop and computer use."
  },
  "create--states.wav": {
    "seconds": 5.306,
    "spoken": "From then on a Linux computer is in one of four states: stopped, starting, running or failed."
  },
  "create--boot.wav": {
    "seconds": 5.381,
    "spoken": "On every boot the engine checks the working account first. If that fails, the computer is stopped."
  },
  "inside--user.wav": {
    "seconds": 7.239,
    "spoken": "Inside, everything runs as one user, named silo, with sue do and no password. So the agent is effectively root."
  },
  "inside--workspace.wav": {
    "seconds": 3.212,
    "spoken": "Projects live in slash workspace, on the persistent disk."
  },
  "inside--exec.wav": {
    "seconds": 5.752,
    "spoken": "The engine reaches in through the runtime's guest agent. It runs commands, and reads and writes files."
  },
  "inside--ssh.wav": {
    "seconds": 4.61,
    "spoken": "S S H is served from the host side. The guest runs no S S H server."
  },
  "inside--tools.wav": {
    "seconds": 5.461,
    "spoken": "That is how the app gives you a terminal, a file browser, and your own editor, opened on the computer."
  },
  "macos--vz.wav": {
    "seconds": 5.143,
    "spoken": "mac O S computers use Apple's Virtualization framework, on Apple silicon only."
  },
  "macos--first.wav": {
    "seconds": 8.016,
    "spoken": "The first one downloads mac O S, about 20 gigabytes, and takes about fifteen minutes. Later ones clone a template in about a minute."
  },
  "macos--setup.wav": {
    "seconds": 6.85,
    "spoken": "Setup is automatic: an account named silo, remote login, developer tools with Claude Code, and Koo-ah Driver."
  },
  "macos--sip.wav": {
    "seconds": 4.421,
    "spoken": "It also boots Recovery and turns off System Integrity Protection in the guest."
  },
  "macos--window.wav": {
    "seconds": 5.616,
    "spoken": "The screen is a native window drawn by the engine. From another Mac it is relayed as screen sharing."
  },
  "macos--two.wav": {
    "seconds": 3.517,
    "spoken": "Apple allows two mac O S virtual machines running at once."
  },
  "macos--gap.wav": {
    "seconds": 9.194,
    "spoken": "One caveat. Until setup completes, and in Recovery, a mac O S computer uses Apple's own network. It can reach your Mac and your local network."
  },
  "macos--thin.wav": {
    "seconds": 4.569,
    "spoken": "And in the app, mac O S computers have a thinner interface than Linux ones today."
  },
  "containers--third.wav": {
    "seconds": 3.842,
    "spoken": "There is a third kind, on Linux hosts only: the container computer."
  },
  "containers--kernel.wav": {
    "seconds": 7.459,
    "spoken": "It runs as a rootless container and shares the host's kernel. It is less isolated than a virtual machine, and never the default."
  },
  "containers--hardened.wav": {
    "seconds": 5.646,
    "spoken": "It is hardened: no capabilities, no new privileges, a system call filter, no host folders."
  },
  "containers--same.wav": {
    "seconds": 7.807,
    "spoken": "It uses the same image and has the same features, except memory in checkpoints. And its disk size is reported, not enforced."
  },
  "containers--net.wav": {
    "seconds": 4.439,
    "spoken": "Its only way out is the same network gateway that mac O S computers use."
  },
  "desktop--xfce.wav": {
    "seconds": 3.641,
    "spoken": "A Linux computer has a desktop by default, on a virtual screen."
  },
  "desktop--stream.wav": {
    "seconds": 4.676,
    "spoken": "A streamer called sell keys encodes it as video. It listens only inside the guest."
  },
  "desktop--tunnel.wav": {
    "seconds": 3.268,
    "spoken": "The engine tunnels to it, and puts a private proxy in front."
  },
  "desktop--viewer.wav": {
    "seconds": 4.149,
    "spoken": "The app shows it in a viewer window. Several viewers can share one desktop."
  },
  "desktop--clipboard.wav": {
    "seconds": 4.851,
    "spoken": "The clipboard is never synced. You paste into the computer, or copy from it, on purpose."
  },
  "computer-use--cua.wav": {
    "seconds": 9.423,
    "spoken": "Computer use means an agent driving that desktop. Silo uses Koo-ah Driver, an open source M C P server that can see the screen, click and type."
  },
  "computer-use--register.wav": {
    "seconds": 4.935,
    "spoken": "After every boot, Silo registers it with Claude Code and Codex inside the computer."
  },
  "computer-use--mode.wav": {
    "seconds": 7.034,
    "spoken": "You choose whether the agent must ask before using it. That is a convenience, not a security boundary. The agent has root."
  },
  "computer-use--outside.wav": {
    "seconds": 7.203,
    "spoken": "An agent outside the computer can drive a desktop too, through Silo's own M C P server. That works on forks only."
  },
  "checkpoints--save.wav": {
    "seconds": 5.599,
    "spoken": "A checkpoint saves a computer. If it is running, that includes its memory, as well as both disks."
  },
  "checkpoints--chain.wav": {
    "seconds": 3.84,
    "spoken": "Checkpoints form a chain of layers. Each one stores only what changed."
  },
  "checkpoints--restore.wav": {
    "seconds": 4.186,
    "spoken": "Restore first saves the present as a recovery checkpoint. Then it goes back."
  },
  "checkpoints--fork.wav": {
    "seconds": 4.843,
    "spoken": "A fork is a new computer made from a checkpoint. It always boots cold, from the disks."
  },
  "checkpoints--quiet.wav": {
    "seconds": 9.235,
    "spoken": "A fork starts quiet: a new identity, scheduled jobs disabled, and no secrets or GitHub access unless you name them. GitHub then comes read-only."
  },
  "checkpoints--files.wav": {
    "seconds": 5.206,
    "spoken": "But files on the disk are copied as they are. A login saved in a file travels with the fork."
  },
  "export--file.wav": {
    "seconds": 5.607,
    "spoken": "A computer or a checkpoint can be exported as one file, and imported on this device or another."
  },
  "export--never.wav": {
    "seconds": 3.853,
    "spoken": "The file never carries memory, secret values or published ports."
  },
  "export--disk.wav": {
    "seconds": 6.051,
    "spoken": "It does carry the whole disk, unfiltered. Silo scans for known logins, and asks before you share."
  },
  "export--import.wav": {
    "seconds": 6.482,
    "spoken": "An import is a new, stopped computer, with no secrets and a closed network. Import only files you trust."
  },
  "export--sparse.wav": {
    "seconds": 3.978,
    "spoken": "As for space: disks are sparse, so they take only what the guest has used."
  },
  "export--trim.wav": {
    "seconds": 4.89,
    "spoken": "When the guest deletes files, Silo trims the workspace, and the space returns to your Mac."
  },
  "threat--attacker.wav": {
    "seconds": 8.2,
    "spoken": "Part three, the one that matters most. The attacker is the agent: prompt injected, root in its computer, and able to read Silo's source."
  },
  "threat--assets.wav": {
    "seconds": 7.916,
    "spoken": "Silo protects, in order: your Mac and your account, your GitHub authority, your secret values, your private network, and your other computers."
  },
  "threat--notdata.wav": {
    "seconds": 4.333,
    "spoken": "It does not protect the data inside the computer. The agent can read all of it."
  },
  "threat--bounds.wav": {
    "seconds": 7.006,
    "spoken": "Four boundaries do the work: the virtual machine, the network policy, the secrets proxy, and tokens minted on the host."
  },
  "network--where.wav": {
    "seconds": 7.205,
    "spoken": "All of a computer's traffic passes through a network stack inside its runtime process, on your Mac. The policy is applied there."
  },
  "network--public.wav": {
    "seconds": 3.747,
    "spoken": "The rule looks at the destination address. Public addresses are allowed."
  },
  "network--private.wav": {
    "seconds": 7.45,
    "spoken": "Everything private is refused: your Mac's own loopback, your local network, your V P N, cloud metadata, and other computers."
  },
  "network--range.wav": {
    "seconds": 6.442,
    "spoken": "The test is the address range, nothing else. A machine on your network that has a public address counts as public."
  },
  "network--dns.wav": {
    "seconds": 5.351,
    "spoken": "D N S goes to your Mac's resolvers. An answer that points to a private address is dropped."
  },
  "network--open.wav": {
    "seconds": 9.092,
    "spoken": "But there is no allowlist, and no traffic limit. A computer can send anything it can read to any public host. And D N S itself is an open channel."
  },
  "ports--publish.wav": {
    "seconds": 6.499,
    "spoken": "The other direction is explicit. You publish a port of the computer, and it appears on your Mac's loopback, nowhere else."
  },
  "ports--url.wav": {
    "seconds": 6.388,
    "spoken": "Web servers also get a U R L per computer: the port, the computer's name, then silo dot localhost."
  },
  "ports--proxy.wav": {
    "seconds": 3.893,
    "spoken": "One local proxy in the engine routes those names to the right computer."
  },
  "ports--who.wav": {
    "seconds": 3.451,
    "spoken": "Any program on your Mac can open them. Other machines can not."
  },
  "secrets--store.wav": {
    "seconds": 5.936,
    "spoken": "A secret's value is stored in the Keychain. You assign it to computers, and to the hosts allowed to receive it."
  },
  "secrets--placeholder.wav": {
    "seconds": 5.114,
    "spoken": "The computer gets an environment variable. Its content is a placeholder, not the value."
  },
  "secrets--send.wav": {
    "seconds": 3.067,
    "spoken": "The agent sends a request, with the placeholder in a header."
  },
  "secrets--intercept.wav": {
    "seconds": 4.728,
    "spoken": "The proxy on your Mac intercepts that connection, because its host is on the secret's list."
  },
  "secrets--swap.wav": {
    "seconds": 5.993,
    "spoken": "It verifies the real server's certificate, then swaps the placeholder for the value. In headers only."
  },
  "secrets--never.wav": {
    "seconds": 2.104,
    "spoken": "The value never enters the computer."
  },
  "secret-limits--scope.wav": {
    "seconds": 8.694,
    "spoken": "Only connections to a secret's hosts, and to GitHub, are intercepted. Other H T T P S traffic passes with the server's own certificate."
  },
  "secret-limits--use.wav": {
    "seconds": 5.197,
    "spoken": "The agent can still use the secret. It can spend your credits. It cannot take the key elsewhere."
  },
  "secret-limits--echo.wav": {
    "seconds": 5.652,
    "spoken": "One real weakness: if an allowed host echoes request headers back, the agent reads the value."
  },
  "secret-limits--any.wav": {
    "seconds": 6.196,
    "spoken": "And a secret can be assigned to any host, after an explicit warning. That removes the boundary for that secret."
  },
  "secret-limits--other.wav": {
    "seconds": 8.416,
    "spoken": "Sent to any other host, the placeholder goes out as it is: a harmless string. For mac O S computers, the gateway blocks that request instead."
  },
  "secret-limits--log.wav": {
    "seconds": 4.727,
    "spoken": "Uses and blocked attempts are logged by name, host and count. Never by value."
  },
  "github--signin.wav": {
    "seconds": 9.075,
    "spoken": "GitHub works the same way, with one more step. You sign in once, in your browser. The engine talks to GitHub directly, and your token stays in the Keychain."
  },
  "github--grant.wav": {
    "seconds": 7.056,
    "spoken": "For each computer, you pick repositories, or a whole owner. Read-only is the default. Write access is a separate switch."
  },
  "github--mint.wav": {
    "seconds": 6.163,
    "spoken": "From that, the engine asks GitHub for child tokens, limited to those repositories and those permissions."
  },
  "github--guest.wav": {
    "seconds": 3.441,
    "spoken": "The computer holds only a placeholder, and Git works normally."
  },
  "github--pick.wav": {
    "seconds": 5.851,
    "spoken": "The proxy picks the right token for each request, read or write. GitHub itself enforces the scope."
  },
  "github--workflows.wav": {
    "seconds": 6.77,
    "spoken": "One thing to know: write access includes workflows. An agent that can write to a repository can change its C I."
  },
  "push-token--push.wav": {
    "seconds": 7.322,
    "spoken": "There is also a Push button that runs on your Mac. The engine fetches the commits out of the computer, and pushes one branch itself."
  },
  "push-token--single.wav": {
    "seconds": 8.789,
    "spoken": "When you are signed in, it uses a token for that single repository, and revokes it afterwards. None of the guest's Git configuration runs on your Mac."
  },
  "push-token--pat.wav": {
    "seconds": 4.969,
    "spoken": "The alternative to signing in is a personal access token. It is simpler, and weaker."
  },
  "push-token--full.wav": {
    "seconds": 4.919,
    "spoken": "In that mode nothing is scoped. The agent, and Push, use the token's full permissions."
  },
  "ssh--enable.wav": {
    "seconds": 4.685,
    "spoken": "You can open one computer to S S H, for an agent or a tool running somewhere else."
  },
  "ssh--listener.wav": {
    "seconds": 3.143,
    "spoken": "The listener runs on your Mac, with its own key, on loopback."
  },
  "ssh--lan.wav": {
    "seconds": 4.004,
    "spoken": "After a confirmation, it can also listen on one address of your network."
  },
  "ssh--end.wav": {
    "seconds": 3.138,
    "spoken": "Turning it off, or stopping the computer, ends every session."
  },
  "local-trust--anyone.wav": {
    "seconds": 4.252,
    "spoken": "Now the engine itself. Its socket accepts any process that runs as you."
  },
  "local-trust--noauth.wav": {
    "seconds": 3.463,
    "spoken": "There is no per-client authentication, and no audit log yet."
  },
  "local-trust--malware.wav": {
    "seconds": 9.918,
    "spoken": "So malware under your account controls every computer. In the evaluations, an unsigned script ran commands in a computer, saved a secret, and published a port."
  },
  "local-trust--escape.wav": {
    "seconds": 7.428,
    "spoken": "And an escape from a virtual machine lands in your user account. The runtime runs as you, with no second sandbox around it."
  },
  "local-trust--scope.wav": {
    "seconds": 3.994,
    "spoken": "Silo calls both out of scope today. Client identity is on the roadmap."
  },
  "evals--ten.wav": {
    "seconds": 6.381,
    "spoken": "These claims are tested by isolation evaluations: ten scenarios, in which a probe plays the root attacker."
  },
  "evals--control.wav": {
    "seconds": 5.796,
    "spoken": "Each blocked attempt is paired with a control that must succeed. Otherwise the result is inconclusive."
  },
  "evals--pass.wav": {
    "seconds": 9.311,
    "spoken": "Seven pass, over two runs: host files, host network, computer to computer, secret in the guest, secret to other hosts, GitHub scope, and the engine socket."
  },
  "evals--gaps.wav": {
    "seconds": 5.872,
    "spoken": "Three are known gaps, and they reproduce: the reflected secret, open egress, and the same-user client."
  },
  "evals--cover.wav": {
    "seconds": 13.125,
    "spoken": "The evaluations cover Linux computers on a Mac. Not mac O S computers, not containers, not Linux hosts, not personal tokens. And the last run is from the third of November, on the previous engine version."
  },
  "app--sidebar.wav": {
    "seconds": 5.348,
    "spoken": "Part four: working with it. The app is the graphical client, and its sidebar is the feature list."
  },
  "app--computers.wav": {
    "seconds": 3.005,
    "spoken": "Computers, with files, logs, network and activity."
  },
  "app--access.wav": {
    "seconds": 2.398,
    "spoken": "GitHub and Secrets, where you grant access."
  },
  "app--pending.wav": {
    "seconds": 2.591,
    "spoken": "Pending changes, where forks wait for your review."
  },
  "app--settings.wav": {
    "seconds": 3.085,
    "spoken": "And settings: general, connections, notifications."
  },
  "app--onboard.wav": {
    "seconds": 5.41,
    "spoken": "The first launch walks through four steps: dependencies, a first computer, GitHub, and a review."
  },
  "app--tray.wav": {
    "seconds": 4.504,
    "spoken": "A menu bar panel lists your computers. Closing the window leaves the app running."
  },
  "app--history.wav": {
    "seconds": 6.301,
    "spoken": "To look back, Activity keeps the history of operations, and Logs keeps each computer's output for seven days."
  },
  "app--fixtures.wav": {
    "seconds": 6.846,
    "spoken": "The interface is one React bundle. It runs on the engine in production, and on fixtures for design and tests."
  },
  "cli--binary.wav": {
    "seconds": 4.528,
    "spoken": "The same engine is driven by a command line tool, silo. The app installs it."
  },
  "cli--basics.wav": {
    "seconds": 4.78,
    "spoken": "It covers the basics: create, start, stop, exec, checkpoint, publish, export."
  },
  "cli--json.wav": {
    "seconds": 5.001,
    "spoken": "Every command can print jason. Long work prints a job I D that you can wait on again."
  },
  "cli--project.wav": {
    "seconds": 4.738,
    "spoken": "It also knows about projects. It looks for a dot silo folder in your repository."
  },
  "cli--recipe.wav": {
    "seconds": 6.106,
    "spoken": "There, a recipe describes a computer: the system, packages, a setup script, services and ports."
  },
  "cli--rules.wav": {
    "seconds": 6.004,
    "spoken": "A second file holds the rules for agents: which secrets a fork may get, and what may be kept without asking."
  },
  "up--build.wav": {
    "seconds": 4.701,
    "spoken": "Building a recipe makes a hidden computer, runs the setup, and saves a checkpoint."
  },
  "up--cache.wav": {
    "seconds": 5.837,
    "spoken": "That checkpoint is cached under a hash of the recipe and the image. Change either one, and it rebuilds."
  },
  "up--up.wav": {
    "seconds": 5.78,
    "spoken": "silo up forks a computer from that checkpoint, starts it, publishes its ports and sets up S S H."
  },
  "up--main.wav": {
    "seconds": 2.523,
    "spoken": "That computer is named main. It is your base."
  },
  "fork-loop--idea.wav": {
    "seconds": 4.369,
    "spoken": "Here is the workflow Silo is built around. An agent does not work in main."
  },
  "fork-loop--fork.wav": {
    "seconds": 3.192,
    "spoken": "You fork main, once per task, or several times at once."
  },
  "fork-loop--work.wav": {
    "seconds": 3.575,
    "spoken": "The agent works in its fork, with everything installed and running."
  },
  "fork-loop--steps.wav": {
    "seconds": 3.518,
    "spoken": "Each command run through Silo is recorded as a numbered step."
  },
  "fork-loop--diff.wav": {
    "seconds": 3.12,
    "spoken": "Then silo diff shows what changed since the fork was made."
  },
  "fork-loop--decide.wav": {
    "seconds": 4.995,
    "spoken": "Then comes the decision. Fold keeps the changes into main. Drop throws the fork away."
  },
  "fork-loop--request.wav": {
    "seconds": 4.806,
    "spoken": "Or fold as a request. Nothing is kept until you approve it, in Pending changes."
  },
  "diff--whole.wav": {
    "seconds": 5.929,
    "spoken": "The diff is not only Git. It compares the whole computer with a record taken when the fork first started."
  },
  "diff--buckets.wav": {
    "seconds": 5.638,
    "spoken": "Changes are sorted into buckets: code, packages, settings and files, databases, and other."
  },
  "diff--notkept.wav": {
    "seconds": 6.226,
    "spoken": "Some things are reported but never kept: new processes, listening ports, services, and credential files."
  },
  "diff--noise.wav": {
    "seconds": 4.446,
    "spoken": "Background noise can be removed, by measuring an idle fork of the same checkpoint."
  },
  "diff--creds.wav": {
    "seconds": 2.685,
    "spoken": "The diff also lists which credentials the fork used."
  },
  "diff--reviewer.wav": {
    "seconds": 6.25,
    "spoken": "An optional hosted reviewer can comment on a diff. It is off unless you configure it, and it only advises."
  },
  "fold--three.wav": {
    "seconds": 4.019,
    "spoken": "Fold is a three-way merge: where the fork started, the fork, and the target."
  },
  "fold--pinned.wav": {
    "seconds": 4.288,
    "spoken": "It is pinned to the diff you reviewed. If the fork changed since, it refuses."
  },
  "fold--safe.wav": {
    "seconds": 7.148,
    "spoken": "It saves a checkpoint of the target first. If anything fails, the target is restored. And silo undo takes you back later."
  },
  "fold--refuse.wav": {
    "seconds": 5.993,
    "spoken": "It refuses on conflicts, on credentials in Git history, and on database changes it cannot cover."
  },
  "fold--writeback.wav": {
    "seconds": 4.54,
    "spoken": "Packages the fork added are written back into the recipe, so the next build has them."
  },
  "fold--auto.wav": {
    "seconds": 8.871,
    "spoken": "Rules can keep some changes without asking: for example, code only, with tests passing. Those tests run inside the fork, so a fork could fake them."
  },
  "fold--promote.wav": {
    "seconds": 2.478,
    "spoken": "Or you promote: the fork becomes the new main."
  },
  "folder-pr--folder.wav": {
    "seconds": 5.734,
    "spoken": "You can also fork a folder on your Mac. Silo copies its committed Git history into a fresh computer."
  },
  "folder-pr--back.wav": {
    "seconds": 3.831,
    "spoken": "Folding brings the commits back, writes a backup reference, then merges."
  },
  "folder-pr--pr.wav": {
    "seconds": 6.092,
    "spoken": "Or fold as a pull request. Your Mac pushes a branch and opens it, with the review summary as its text."
  },
  "folder-pr--own.wav": {
    "seconds": 4.234,
    "spoken": "That push uses your own GitHub credentials, on the host. Never the fork's."
  },
  "mcp--serve.wav": {
    "seconds": 4.802,
    "spoken": "silo M C P gives the same operations to an agent, as 39 tools."
  },
  "mcp--flow.wav": {
    "seconds": 5.177,
    "spoken": "An agent on your Mac can fork, run commands, read the diff, and fold, or leave a request."
  },
  "mcp--gui.wav": {
    "seconds": 2.338,
    "spoken": "It can also see and click in a fork's desktop."
  },
  "mcp--limits.wav": {
    "seconds": 6.209,
    "spoken": "It can not read secret values, export or import. And it can not drop main, or drive main's desktop."
  },
  "mcp--power.wav": {
    "seconds": 8.541,
    "spoken": "Everything else, it can do: keep, promote, undo, run commands in main. It has your authority at the command line. Only the rules file narrows it."
  },
  "in-guest--why.wav": {
    "seconds": 4.343,
    "spoken": "Last, an agent that lives inside a computer can ask for forks of itself."
  },
  "in-guest--token.wav": {
    "seconds": 5.442,
    "spoken": "You install a scoped token in that computer. Its requests travel through the runtime's shared folder."
  },
  "in-guest--allowed.wav": {
    "seconds": 6.517,
    "spoken": "The token allows thirteen methods: list, fork itself, start and stop its own forks, diff, and request review."
  },
  "in-guest--caps.wav": {
    "seconds": 2.374,
    "spoken": "At most 32 forks, three of them running."
  },
  "in-guest--denied.wav": {
    "seconds": 6.6,
    "spoken": "It can never keep, delete, run commands elsewhere, or change access. What it produces waits in Pending changes."
  },
  "connections--what.wav": {
    "seconds": 5.967,
    "spoken": "Part five: beyond one Mac. Connections lets one Silo manage the computers of another device."
  },
  "connections--ssh.wav": {
    "seconds": 6.234,
    "spoken": "The link is plain S S H. Your key is added on the other device, restricted to one command: Silo's bridge."
  },
  "connections--bridge.wav": {
    "seconds": 2.884,
    "spoken": "The bridge passes your requests to that device's engine."
  },
  "connections--owner.wav": {
    "seconds": 4.784,
    "spoken": "The computers, their secrets and their GitHub sign-in stay on the device that owns them."
  },
  "connections--authority.wav": {
    "seconds": 8.266,
    "spoken": "A connected controller has roughly that engine's full authority. Its key does not expire, and revoking it does not close open sessions."
  },
  "connections--version.wav": {
    "seconds": 4.642,
    "spoken": "The bridge has its own protocol version, and both devices must run the same one."
  },
  "linux-host--service.wav": {
    "seconds": 4.283,
    "spoken": "The engine runs on a Linux server as a user service, with no interface."
  },
  "linux-host--init.wav": {
    "seconds": 5.202,
    "spoken": "silo host init checks for K V M, registers the service, and authorises your key."
  },
  "linux-host--device.wav": {
    "seconds": 4.076,
    "spoken": "From your Mac, you add the device. Then most commands take a device option."
  },
  "linux-host--differs.wav": {
    "seconds": 6.736,
    "spoken": "Two differences: secrets are files only that account can read, instead of a Keychain. And nothing there is signed yet."
  },
  "runner--what.wav": {
    "seconds": 3.804,
    "spoken": "silo-runner makes a host the machine your GitHub Actions jobs run on."
  },
  "runner--fresh.wav": {
    "seconds": 3.508,
    "spoken": "Each job gets a fresh computer, forked from a recipe's checkpoint."
  },
  "runner--cli.wav": {
    "seconds": 6.789,
    "spoken": "The runner speaks to GitHub, and drives Silo only through the command line. The engine never learns GitHub's protocol."
  },
  "runner--end.wav": {
    "seconds": 5.512,
    "spoken": "When a job ends, its computer is deleted. Failed ones are kept a few days, for inspection."
  },
  "runner--private.wav": {
    "seconds": 7.564,
    "spoken": "It refuses public repositories, and checks again before every job, because any pull request author could run code on your machine."
  },
  "runner--secrets.wav": {
    "seconds": 5.667,
    "spoken": "A job's own GitHub secrets go straight into its computer. Silo's placeholders do not cover them."
  },
  "runner--live.wav": {
    "seconds": 4.317,
    "spoken": "It was checked live on a Mac. A job through a Linux host has not been run yet."
  },
  "build--stack.wav": {
    "seconds": 7.791,
    "spoken": "Part six: the state of the project. The engine and the app shell are Rust. The interface is React and TypeScript. The runner is Go."
  },
  "build--ci.wav": {
    "seconds": 5.217,
    "spoken": "C I runs the interface tests, the Rust tests on a Mac, and the engine's tests on Linux."
  },
  "build--kvm.wav": {
    "seconds": 4.533,
    "spoken": "A separate job boots a real computer on a Linux runner, and drives it end to end."
  },
  "build--dev.wav": {
    "seconds": 4.131,
    "spoken": "Development always uses the Dev build, which can not touch production data."
  },
  "release--two.wav": {
    "seconds": 7.928,
    "spoken": "There are two release lines. The app, as a disk image that updates itself. And the engine alone, as an archive for other apps to embed."
  },
  "release--embed.wav": {
    "seconds": 5.226,
    "spoken": "An embedding app bundles silo dee, signs it, and states which signature the engine must carry."
  },
  "release--notarized.wav": {
    "seconds": 8.811,
    "spoken": "Today the app is not notarized, so the first launch needs your approval in System Settings. Updates are signed with a separate key, and always verified."
  },
  "release--feed.wav": {
    "seconds": 4.029,
    "spoken": "The engine has no update channel of its own yet. It updates with the app."
  },
  "gaps--egress.wav": {
    "seconds": 6.374,
    "spoken": "So, the honest list. There is no egress allowlist and no traffic limit: data the agent can read can leave."
  },
  "gaps--echo.wav": {
    "seconds": 1.98,
    "spoken": "Reflected secrets are not redacted."
  },
  "gaps--socket.wav": {
    "seconds": 3.159,
    "spoken": "The socket has no client identity and no audit log."
  },
  "gaps--share.wav": {
    "seconds": 2.07,
    "spoken": "The runtime's shared folder is unreviewed."
  },
  "gaps--macos.wav": {
    "seconds": 5.278,
    "spoken": "mac O S computers are exposed during setup, and are not covered by the evaluations."
  },
  "gaps--pat.wav": {
    "seconds": 1.753,
    "spoken": "Personal tokens are not scoped."
  },
  "gaps--protocol.wav": {
    "seconds": 4.501,
    "spoken": "The protocol accepts a single version, so every change breaks older clients."
  },
  "gaps--docs.wav": {
    "seconds": 6.577,
    "spoken": "And some documentation trails the code. The read me still calls the Linux engine the next milestone, though it runs."
  },
  "roadmap--now.wav": {
    "seconds": 3.692,
    "spoken": "In progress: finishing the move to Koo-ah Driver for computer use."
  },
  "roadmap--next.wav": {
    "seconds": 13.86,
    "spoken": "Next, needing no funding: an egress policy per computer. Redacting reflected secrets. Client identity and an audit log. A review of the shared folder. Finishing the Linux host, a protocol reference, and project governance."
  },
  "roadmap--then.wav": {
    "seconds": 7.614,
    "spoken": "Waiting for funding: evaluations in C I, an external security review, signed and notarized releases, and published crates."
  },
  "recap--engine.wav": {
    "seconds": 4.432,
    "spoken": "So. Silo is a local engine that gives each agent its own virtual machine."
  },
  "recap--clients.wav": {
    "seconds": 5.877,
    "spoken": "Clients talk to it over one socket: an app, a command line, an M C P server, a C I runner."
  },
  "recap--hostile.wav": {
    "seconds": 9.044,
    "spoken": "The agent is assumed hostile. It reaches public addresses only. It uses credentials it can not read. It sees only the repositories you grant."
  },
  "recap--loop.wav": {
    "seconds": 5.318,
    "spoken": "You work by forking a base computer, reviewing what the fork changed, and keeping it or dropping it."
  },
  "recap--limits.wav": {
    "seconds": 5.785,
    "spoken": "Its limits are stated: what the agent can read can leave, and anything running as you controls the engine."
  },
  "recap--end.wav": {
    "seconds": 0.847,
    "spoken": "That is Silo."
  }
};

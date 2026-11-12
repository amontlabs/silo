/**
 * The film's narration, and nothing else: no layout, no timing. Each scene is a
 * list of sentences ("beats"). A beat's key is what the scene's picture refers
 * to, so a sentence can be reworded or moved without touching coordinates.
 *
 * `{written|spoken}` shows one thing in the caption and hands another to the
 * voice, for names it would mispronounce.
 */
export type BeatOptions = { pause?: number };
export type Beat = [key: string, text: string, options?: BeatOptions];
export type SceneNarration = { id: string; chapter: number; title: string; beats: Beat[] };

export const CHAPTERS = [
  "Overview",
  "The engine",
  "Computers",
  "Isolation",
  "Working with it",
  "Beyond one Mac",
  "State of the project",
];

const SILOD = "{silod|silo dee}";
const MSB = "{msb|M S B}";
const MACOS = "{macOS|mac O S}";
const CLI = "command line";

export const SCENES: SceneNarration[] = [
  // ───────────────────────────── Overview ─────────────────────────────
  {
    id: "title",
    chapter: 0,
    title: "",
    beats: [["title", "This is Silo: what it is, how it is built, and where its limits are."]],
  },
  {
    id: "problem",
    chapter: 0,
    title: "The problem",
    beats: [
      ["reads", "A coding agent reads things you did not write: issues, pull request comments, web pages, dependencies."],
      ["inject", "Any of them can carry instructions. That is prompt injection."],
      ["reach", "And the agent usually runs on your machine, next to your files, your network and your tokens."],
      ["sandbox", "The sandboxes built into agent tools limit which files and hosts it can touch."],
      ["token", "None of them narrows what a credential is allowed to do. A token that can push to every repository still can."],
    ],
  },
  {
    id: "answer",
    chapter: 0,
    title: "Silo: computers for agents",
    beats: [
      ["vm", "Silo gives each agent its own computer: a virtual machine, with its own kernel and its own disk."],
      ["pick", "It holds only the repositories and credentials you pick."],
      ["hostile", "Silo assumes the agent inside is hostile, and has root there."],
      ["two", "So it limits two things: what the agent can reach, and what it is authorised to do."],
      ["extras", "Each computer also has a desktop the agent can drive, and checkpoints to go back to."],
      ["local", "All of it runs locally. There is no Silo account and no Silo server."],
    ],
  },
  {
    id: "words",
    chapter: 0,
    title: "Five words, three colours",
    beats: [
      ["device", "Five words first. A device is a physical machine that runs Silo: your Mac, or a Linux server."],
      ["computer", "A computer is a virtual one, which Silo creates for an agent."],
      ["workspace", "The workspace is one folder inside a computer, {/workspace|slash workspace}, on its own disk."],
      ["checkpoint", "A checkpoint is a saved state of a computer. A fork is a new computer made from one."],
      ["connections", "Connections links devices, so one Silo manages computers on another."],
      ["colours", "And three colours for the whole film. Blue is your device, which is trusted. Violet is a computer and its agent, which is not. Orange is Silo's engine, in between."],
    ],
  },
  {
    id: "map",
    chapter: 0,
    title: "The whole system on one screen",
    beats: [
      ["clients", `At the top are the clients: the Silo app, the silo ${CLI}, an {MCP|M C P} server for agents, a {CI|C I} runner, and other apps that embed the engine.`],
      ["socket", "They all speak one protocol, over one local socket."],
      ["engine", `Behind the socket is the engine, a daemon called ${SILOD}. It owns the computers, the credentials and the network rules.`],
      ["runtimes", `The engine drives the runtimes that run the computers: MicroSandbox for Linux, and Apple's Virtualization framework for ${MACOS}.`],
      ["guests", "At the bottom are the computers, each with an agent inside."],
      ["plan", "We go through it in that order: the engine, the computers, the isolation, and the tools you work with. Then other devices, and the state of the project."],
    ],
  },
  {
    id: "status",
    chapter: 0,
    title: "Where it stands",
    beats: [
      ["who", "Silo is {MIT|M I T} licensed, built by one person, and still a work in progress."],
      ["versions", "The app is at version 0.13, the engine at 0.2, and the protocol at version 7."],
      ["host", "The host is a Mac with Apple silicon. The engine also runs on Linux servers, without the app."],
      ["guests", `The guests are Ubuntu 24.04 and ${MACOS}.`],
    ],
  },

  // ───────────────────────────── The engine ─────────────────────────────
  {
    id: "processes",
    chapter: 1,
    title: "What runs on your Mac",
    beats: [
      ["launchd", "Part one: the engine. It is started by {launchd|launch dee} when you log in. One engine per user account."],
      ["app", "The app is only a client. It never runs a computer itself, and a test fails if its code ever tries."],
      ["msb", `Each Linux computer is a separate process named ${MSB}: the MicroSandbox runtime, holding one virtual machine.`],
      ["vz", `${MACOS} computers run inside the engine's own process.`],
      ["gateway", "Each of those also gets a small gateway process, which is its only network."],
      ["quit", "Because the engine owns the computers, quitting the app does not stop them. Quit asks you: cancel, keep running, or stop and quit."],
    ],
  },
  {
    id: "wire",
    chapter: 1,
    title: "The socket and the protocol",
    beats: [
      ["file", "The socket is a file, {engine.sock|engine dot sock}, in the {.silo|dot silo} folder of your home."],
      ["uid", "Only your user can open it, and the engine checks the user of every connection."],
      ["frame", "A message is {JSON|jason}, prefixed with its length in four bytes."],
      ["hello", "The first message must be a handshake. Each side states the protocol versions it supports. The engine supports exactly one: seven."],
      ["request", "After that, a request carries an {ID|I D} and a method name, and the response carries the same {ID|I D}."],
      ["events", "The engine also pushes events on the same connection. A computer changed. A job made progress."],
    ],
  },
  {
    id: "methods",
    chapter: 1,
    title: "175 methods, five kinds",
    beats: [
      ["count", "The protocol has 175 methods, each with typed parameters and results."],
      ["query", "Queries read state. Commands change it, and answer at once."],
      ["job", "Jobs are for slow work, like creating a computer. They return a job {ID|I D} immediately."],
      ["stream", "Streams turn the connection into raw bytes, mostly for {SSH|S S H} and for screens. Subscriptions deliver events and file changes."],
      ["groups", "The methods are grouped by area: computers, checkpoints, changes, guest, display, GitHub, secrets, network, and more."],
      ["schema", "The contract is one Rust crate, and a {JSON|jason} schema generated from it, so any language can have a client."],
    ],
  },
  {
    id: "jobs",
    chapter: 1,
    title: "Jobs and the queue",
    beats: [
      ["progress", "A job reports progress as events, then one final result."],
      ["stored", "Jobs are stored in a small {SQLite|sequel light} database. If the engine restarts in the middle of one, the client is told so, instead of waiting forever."],
      ["ack", "The client acknowledges a result once it has it."],
      ["lanes", "All work passes through one queue. Each computer has its own lane, so two computers work in parallel."],
      ["wide", "Work that touches the whole device waits for everything else to finish."],
    ],
  },
  {
    id: "crates",
    chapter: 1,
    title: "The code, crate by crate",
    beats: [
      ["api", "Most of the code is one Rust workspace. {silo-api|silo API} is the contract."],
      ["client", "silo-client is a thin client: framing, handshake, events."],
      ["engine", "silo-engine is the engine as a library, with no interface code in it."],
      ["silod", `${SILOD} wraps it into the daemon: the socket, the dispatcher, the timers.`],
      ["ui", "{silo-ui|silo U I} is the app: a {Tauri|tow-ree} shell in Rust, with a React interface."],
      ["cli", `{silo-cli|silo C L I} is the ${CLI} and the {MCP|M C P} server.`],
      ["rest", "Around them: helpers shared by the app and the engine, the network gateway, a container helper, and the {CI|C I} runner, which is a separate Go program."],
      ["context", "The engine touches the outside world through one interface, the engine context: paths, identity, publishing events, the main thread. That is what lets it run without the app."],
    ],
  },
  {
    id: "disk",
    chapter: 1,
    title: "Where state lives",
    beats: [
      ["home", "State lives in three places. The {.silo|dot silo} folder holds the socket, a lock, and the job database."],
      ["appdata", "Application Support holds the rest: settings, the list of computers, their disks, and the guest image."],
      ["keychain", "Secret values and GitHub tokens are in the Keychain. The settings files hold only names and references."],
      ["keys", "Some keys are plain files that only you can read: {SSH|S S H} keys, and the certificate authority that the Linux proxy signs with."],
      ["channels", "There are two builds, production and Dev. Each has its own folders, its own socket and its own Keychain entries. They never share state."],
    ],
  },
  {
    id: "lifecycle",
    chapter: 1,
    title: "Starting, updating, watching",
    beats: [
      ["sign", "When the engine starts on a Mac, it first checks its own code signature against a requirement in its bundle. A production engine refuses to start without one."],
      ["lock", "Then it takes the lock, so only one can run."],
      ["open", "It converts older saved data and opens the socket. Computers to resume, or to start at launch, follow in the background."],
      ["update", "Updates arrive with the app. Before installing, the engine saves the running computers and exits. The new engine resumes exactly that set."],
      ["watch", "While computers run, a health watch polls every ten seconds, and reports any change that nobody asked for."],
    ],
  },

  // ───────────────────────────── Computers ─────────────────────────────
  {
    id: "linux-stack",
    chapter: 2,
    title: "A Linux computer, layer by layer",
    beats: [
      ["msb", `Part two: the computers. A Linux computer is a stack. The engine calls ${MSB}, MicroSandbox version {0.7.6|zero point seven point six}.`],
      ["krun", "MicroSandbox uses {libkrun|lib K run}, a small virtual machine monitor that brings its own Linux kernel."],
      ["hv", "That sits on Apple's Hypervisor framework. On a Linux server, on {KVM|K V M}."],
      ["own", "So every computer has its own kernel, and two disks: a root disk, and the workspace."],
      ["nofolder", "No folder of yours is shared into it."],
      ["share", `With one exception: the runtime's own control folder, mounted read-write at {/.msb|slash dot M S B}. It has not had a security review.`],
      ["patches", "Silo carries 24 patches on MicroSandbox, each pinned by checksum. They add what Silo needs, like publishing ports on a running machine."],
    ],
  },
  {
    id: "image",
    chapter: 2,
    title: "The guest image",
    beats: [
      ["base", "Every Linux computer starts from one image: Ubuntu 24.04, about 350 megabytes."],
      ["inside", "It contains Git and the GitHub tools, a desktop with a video streamer, and {Cua|Koo-ah} Driver for computer use."],
      ["noagent", "It contains no agent. Claude Code is installed on request, at a pinned version, and Silo never signs in for you."],
      ["fetch", "The image is not in the installer. The engine downloads it once, in the background, and checks it against a pinned checksum."],
      ["pin", "A new image version affects only new computers. Existing ones keep the image they were made from."],
    ],
  },
  {
    id: "create",
    chapter: 2,
    title: "Creating a computer",
    beats: [
      ["config", "Creating a computer is a job with visible steps. First its size. The app proposes 8 {CPUs|C P U cores}, 32 gigabytes of memory and a 120 gigabyte workspace. The command line defaults are far smaller."],
      ["made", "Then the engine waits for the image, and has the runtime create the machine, stopped."],
      ["verify", "It verifies the tools, then boots once to set up the desktop and computer use."],
      ["states", "From then on a Linux computer is in one of four states: stopped, starting, running or failed."],
      ["boot", "On every boot the engine checks the working account first. If that fails, the computer is stopped."],
    ],
  },
  {
    id: "inside",
    chapter: 2,
    title: "Inside the computer",
    beats: [
      ["user", "Inside, everything runs as one user, named silo, with {sudo|sue do} and no password. So the agent is effectively root."],
      ["workspace", "Projects live in {/workspace|slash workspace}, on the persistent disk."],
      ["exec", "The engine reaches in through the runtime's guest agent. It runs commands, and reads and writes files."],
      ["ssh", "{SSH|S S H} is served from the host side. The guest runs no {SSH|S S H} server."],
      ["tools", "That is how the app gives you a terminal, a file browser, and your own editor, opened on the computer."],
    ],
  },
  {
    id: "macos",
    chapter: 2,
    title: "macOS computers",
    beats: [
      ["vz", `${MACOS} computers use Apple's Virtualization framework, on Apple silicon only.`],
      ["first", `The first one downloads ${MACOS}, about 20 gigabytes, and takes about fifteen minutes. Later ones clone a template in about a minute.`],
      ["setup", "Setup is automatic: an account named silo, remote login, developer tools with Claude Code, and {Cua|Koo-ah} Driver."],
      ["sip", "It also boots Recovery and turns off System Integrity Protection in the guest."],
      ["window", "The screen is a native window drawn by the engine. From another Mac it is relayed as screen sharing."],
      ["two", `Apple allows two ${MACOS} virtual machines running at once.`],
      ["gap", `One caveat. Until setup completes, and in Recovery, a ${MACOS} computer uses Apple's own network. It can reach your Mac and your local network.`],
      ["thin", `And in the app, ${MACOS} computers have a thinner interface than Linux ones today.`],
    ],
  },
  {
    id: "containers",
    chapter: 2,
    title: "Container computers",
    beats: [
      ["third", "There is a third kind, on Linux hosts only: the container computer."],
      ["kernel", "It runs as a rootless container and shares the host's kernel. It is less isolated than a virtual machine, and never the default."],
      ["hardened", "It is hardened: no capabilities, no new privileges, a system call filter, no host folders."],
      ["same", "It uses the same image and has the same features, except memory in checkpoints. And its disk size is reported, not enforced."],
      ["net", `Its only way out is the same network gateway that ${MACOS} computers use.`],
    ],
  },
  {
    id: "desktop",
    chapter: 2,
    title: "The desktop",
    beats: [
      ["xfce", "A Linux computer has a desktop by default, on a virtual screen."],
      ["stream", "A streamer called {Selkies|sell keys} encodes it as video. It listens only inside the guest."],
      ["tunnel", "The engine tunnels to it, and puts a private proxy in front."],
      ["viewer", "The app shows it in a viewer window. Several viewers can share one desktop."],
      ["clipboard", "The clipboard is never synced. You paste into the computer, or copy from it, on purpose."],
    ],
  },
  {
    id: "computer-use",
    chapter: 2,
    title: "Computer use",
    beats: [
      ["cua", "Computer use means an agent driving that desktop. Silo uses {Cua|Koo-ah} Driver, an open source {MCP|M C P} server that can see the screen, click and type."],
      ["register", "After every boot, Silo registers it with Claude Code and Codex inside the computer."],
      ["mode", "You choose whether the agent must ask before using it. That is a convenience, not a security boundary. The agent has root."],
      ["outside", "An agent outside the computer can drive a desktop too, through Silo's own {MCP|M C P} server. That works on forks only."],
    ],
  },
  {
    id: "checkpoints",
    chapter: 2,
    title: "Checkpoints, restore, fork",
    beats: [
      ["save", "A checkpoint saves a computer. If it is running, that includes its memory, as well as both disks."],
      ["chain", "Checkpoints form a chain of layers. Each one stores only what changed."],
      ["restore", "Restore first saves the present as a recovery checkpoint. Then it goes back."],
      ["fork", "A fork is a new computer made from a checkpoint. It always boots cold, from the disks."],
      ["quiet", "A fork starts quiet: a new identity, scheduled jobs disabled, and no secrets or GitHub access unless you name them. GitHub then comes read-only."],
      ["files", "But files on the disk are copied as they are. A login saved in a file travels with the fork."],
    ],
  },
  {
    id: "export",
    chapter: 2,
    title: "Export, import, storage",
    beats: [
      ["file", "A computer or a checkpoint can be exported as one file, and imported on this device or another."],
      ["never", "The file never carries memory, secret values or published ports."],
      ["disk", "It does carry the whole disk, unfiltered. Silo scans for known logins, and asks before you share."],
      ["import", "An import is a new, stopped computer, with no secrets and a closed network. Import only files you trust."],
      ["sparse", "As for space: disks are sparse, so they take only what the guest has used."],
      ["trim", "When the guest deletes files, Silo trims the workspace, and the space returns to your Mac."],
    ],
  },

  // ───────────────────────────── Isolation ─────────────────────────────
  {
    id: "threat",
    chapter: 3,
    title: "The threat model",
    beats: [
      ["attacker", "Part three, the one that matters most. The attacker is the agent: prompt injected, root in its computer, and able to read Silo's source."],
      ["assets", "Silo protects, in order: your Mac and your account, your GitHub authority, your secret values, your private network, and your other computers."],
      ["notdata", "It does not protect the data inside the computer. The agent can read all of it."],
      ["bounds", "Four boundaries do the work: the virtual machine, the network policy, the secrets proxy, and tokens minted on the host."],
    ],
  },
  {
    id: "network",
    chapter: 3,
    title: "The network: public addresses only",
    beats: [
      ["where", "All of a computer's traffic passes through a network stack inside its runtime process, on your Mac. The policy is applied there."],
      ["public", "The rule looks at the destination address. Public addresses are allowed."],
      ["private", "Everything private is refused: your Mac's own loopback, your local network, your {VPN|V P N}, cloud metadata, and other computers."],
      ["range", "The test is the address range, nothing else. A machine on your network that has a public address counts as public."],
      ["dns", "{DNS|D N S} goes to your Mac's resolvers. An answer that points to a private address is dropped."],
      ["open", "But there is no allowlist, and no traffic limit. A computer can send anything it can read to any public host. And {DNS|D N S} itself is an open channel."],
    ],
  },
  {
    id: "ports",
    chapter: 3,
    title: "Reaching a computer's services",
    beats: [
      ["publish", "The other direction is explicit. You publish a port of the computer, and it appears on your Mac's loopback, nowhere else."],
      ["url", "Web servers also get a {URL|U R L} per computer: the port, the computer's name, then {silo.localhost|silo dot localhost}."],
      ["proxy", "One local proxy in the engine routes those names to the right computer."],
      ["who", "Any program on your Mac can open them. Other machines can not."],
    ],
  },
  {
    id: "secrets",
    chapter: 3,
    title: "Secrets the agent can use but not read",
    beats: [
      ["store", "A secret's value is stored in the Keychain. You assign it to computers, and to the hosts allowed to receive it."],
      ["placeholder", "The computer gets an environment variable. Its content is a placeholder, not the value."],
      ["send", "The agent sends a request, with the placeholder in a header."],
      ["intercept", "The proxy on your Mac intercepts that connection, because its host is on the secret's list."],
      ["swap", "It verifies the real server's certificate, then swaps the placeholder for the value. In headers only."],
      ["never", "The value never enters the computer."],
    ],
  },
  {
    id: "secret-limits",
    chapter: 3,
    title: "What that does, and does not, give you",
    beats: [
      ["scope", "Only connections to a secret's hosts, and to GitHub, are intercepted. Other {HTTPS|H T T P S} traffic passes with the server's own certificate."],
      ["use", "The agent can still use the secret. It can spend your credits. It cannot take the key elsewhere."],
      ["echo", "One real weakness: if an allowed host echoes request headers back, the agent reads the value."],
      ["any", "And a secret can be assigned to any host, after an explicit warning. That removes the boundary for that secret."],
      ["other", "Sent to any other host, the placeholder goes out as it is: a harmless string. For {macOS|mac O S} computers, the gateway blocks that request instead."],
      ["log", "Uses and blocked attempts are logged by name, host and count. Never by value."],
    ],
  },
  {
    id: "github",
    chapter: 3,
    title: "GitHub, per computer and per repository",
    beats: [
      ["signin", "GitHub works the same way, with one more step. You sign in once, in your browser. The engine talks to GitHub directly, and your token stays in the Keychain."],
      ["grant", "For each computer, you pick repositories, or a whole owner. Read-only is the default. Write access is a separate switch."],
      ["mint", "From that, the engine asks GitHub for child tokens, limited to those repositories and those permissions."],
      ["guest", "The computer holds only a placeholder, and Git works normally."],
      ["pick", "The proxy picks the right token for each request, read or write. GitHub itself enforces the scope."],
      ["workflows", "One thing to know: write access includes workflows. An agent that can write to a repository can change its {CI|C I}."],
    ],
  },
  {
    id: "push-token",
    chapter: 3,
    title: "Push from the host, and personal tokens",
    beats: [
      ["push", "There is also a Push button that runs on your Mac. The engine fetches the commits out of the computer, and pushes one branch itself."],
      ["single", "When you are signed in, it uses a token for that single repository, and revokes it afterwards. None of the guest's Git configuration runs on your Mac."],
      ["pat", "The alternative to signing in is a personal access token. It is simpler, and weaker."],
      ["full", "In that mode nothing is scoped. The agent, and Push, use the token's full permissions."],
    ],
  },
  {
    id: "ssh",
    chapter: 3,
    title: "SSH access for outside tools",
    beats: [
      ["enable", "You can open one computer to {SSH|S S H}, for an agent or a tool running somewhere else."],
      ["listener", "The listener runs on your Mac, with its own key, on loopback."],
      ["lan", "After a confirmation, it can also listen on one address of your network."],
      ["end", "Turning it off, or stopping the computer, ends every session."],
    ],
  },
  {
    id: "local-trust",
    chapter: 3,
    title: "The local trust model",
    beats: [
      ["anyone", "Now the engine itself. Its socket accepts any process that runs as you."],
      ["noauth", "There is no per-client authentication, and no audit log yet."],
      ["malware", "So malware under your account controls every computer. In the {evals|evaluations}, an unsigned script ran commands in a computer, saved a secret, and published a port."],
      ["escape", "And an escape from a virtual machine lands in your user account. The runtime runs as you, with no second sandbox around it."],
      ["scope", "Silo calls both out of scope today. Client identity is on the roadmap."],
    ],
  },
  {
    id: "evals",
    chapter: 3,
    title: "What was actually tested",
    beats: [
      ["ten", "These claims are tested by isolation {evals|evaluations}: ten scenarios, in which a probe plays the root attacker."],
      ["control", "Each blocked attempt is paired with a control that must succeed. Otherwise the result is inconclusive."],
      ["pass", "Seven pass, over two runs: host files, host network, computer to computer, secret in the guest, secret to other hosts, GitHub scope, and the engine socket."],
      ["gaps", "Three are known gaps, and they reproduce: the reflected secret, open egress, and the same-user client."],
      ["cover", `The {evals|evaluations} cover Linux computers on a Mac. Not ${MACOS} computers, not containers, not Linux hosts, not personal tokens. And the last run is from the third of November, on the previous engine version.`],
    ],
  },

  // ───────────────────────────── Working with it ─────────────────────────────
  {
    id: "app",
    chapter: 4,
    title: "The Silo app",
    beats: [
      ["sidebar", "Part four: working with it. The app is the graphical client, and its sidebar is the feature list."],
      ["computers", "Computers, with files, logs, network and activity."],
      ["access", "GitHub and Secrets, where you grant access."],
      ["pending", "Pending changes, where forks wait for your review."],
      ["settings", "And settings: general, connections, notifications."],
      ["onboard", "The first launch walks through four steps: dependencies, a first computer, GitHub, and a review."],
      ["tray", "A menu bar panel lists your computers. Closing the window leaves the app running."],
      ["history", "To look back, Activity keeps the history of operations, and Logs keeps each computer's output for seven days."],
      ["fixtures", "The interface is one React bundle. It runs on the engine in production, and on fixtures for design and tests."],
    ],
  },
  {
    id: "cli",
    chapter: 4,
    title: "The silo command line",
    beats: [
      ["binary", `The same engine is driven by a ${CLI} tool, silo. The app installs it.`],
      ["basics", "It covers the basics: create, start, stop, exec, checkpoint, publish, export."],
      ["json", "Every command can print {JSON|jason}. Long work prints a job {ID|I D} that you can wait on again."],
      ["project", "It also knows about projects. It looks for a {.silo|dot silo} folder in your repository."],
      ["recipe", "There, a recipe describes a computer: the system, packages, a setup script, services and ports."],
      ["rules", "A second file holds the rules for agents: which secrets a fork may get, and what may be kept without asking."],
    ],
  },
  {
    id: "up",
    chapter: 4,
    title: "Recipes and silo up",
    beats: [
      ["build", "Building a recipe makes a hidden computer, runs the setup, and saves a checkpoint."],
      ["cache", "That checkpoint is cached under a hash of the recipe and the image. Change either one, and it rebuilds."],
      ["up", "silo up forks a computer from that checkpoint, starts it, publishes its ports and sets up {SSH|S S H}."],
      ["main", "That computer is named main. It is your base."],
    ],
  },
  {
    id: "fork-loop",
    chapter: 4,
    title: "The fork loop",
    beats: [
      ["idea", "Here is the workflow Silo is built around. An agent does not work in main."],
      ["fork", "You fork main, once per task, or several times at once."],
      ["work", "The agent works in its fork, with everything installed and running."],
      ["steps", "Each command run through Silo is recorded as a numbered step."],
      ["diff", "Then silo diff shows what changed since the fork was made."],
      ["decide", "Then comes the decision. Fold keeps the changes into main. Drop throws the fork away."],
      ["request", "Or fold as a request. Nothing is kept until you approve it, in Pending changes."],
    ],
  },
  {
    id: "diff",
    chapter: 4,
    title: "What a diff contains",
    beats: [
      ["whole", "The diff is not only Git. It compares the whole computer with a record taken when the fork first started."],
      ["buckets", "Changes are sorted into buckets: code, packages, settings and files, databases, and other."],
      ["notkept", "Some things are reported but never kept: new processes, listening ports, services, and credential files."],
      ["noise", "Background noise can be removed, by measuring an idle fork of the same checkpoint."],
      ["creds", "The diff also lists which credentials the fork used."],
      ["reviewer", "An optional hosted reviewer can comment on a diff. It is off unless you configure it, and it only advises."],
    ],
  },
  {
    id: "fold",
    chapter: 4,
    title: "Keeping changes",
    beats: [
      ["three", "Fold is a three-way merge: where the fork started, the fork, and the target."],
      ["pinned", "It is pinned to the diff you reviewed. If the fork changed since, it refuses."],
      ["safe", "It saves a checkpoint of the target first. If anything fails, the target is restored. And silo undo takes you back later."],
      ["refuse", "It refuses on conflicts, on credentials in Git history, and on database changes it cannot cover."],
      ["writeback", "Packages the fork added are written back into the recipe, so the next build has them."],
      ["auto", "Rules can keep some changes without asking: for example, code only, with tests passing. Those tests run inside the fork, so a fork could fake them."],
      ["promote", "Or you promote: the fork becomes the new main."],
    ],
  },
  {
    id: "folder-pr",
    chapter: 4,
    title: "Folders and pull requests",
    beats: [
      ["folder", "You can also fork a folder on your Mac. Silo copies its committed Git history into a fresh computer."],
      ["back", "Folding brings the commits back, writes a backup reference, then merges."],
      ["pr", "Or fold as a pull request. Your Mac pushes a branch and opens it, with the review summary as its text."],
      ["own", "That push uses your own GitHub credentials, on the host. Never the fork's."],
    ],
  },
  {
    id: "mcp",
    chapter: 4,
    title: "The MCP server",
    beats: [
      ["serve", "{silo mcp|silo M C P} gives the same operations to an agent, as 39 tools."],
      ["flow", "An agent on your Mac can fork, run commands, read the diff, and fold, or leave a request."],
      ["gui", "It can also see and click in a fork's desktop."],
      ["limits", "It can not read secret values, export or import. And it can not drop main, or drive main's desktop."],
      ["power", "Everything else, it can do: keep, promote, undo, run commands in main. It has your authority at the command line. Only the rules file narrows it."],
    ],
  },
  {
    id: "in-guest",
    chapter: 4,
    title: "The in-guest client",
    beats: [
      ["why", "Last, an agent that lives inside a computer can ask for forks of itself."],
      ["token", "You install a scoped token in that computer. Its requests travel through the runtime's shared folder."],
      ["allowed", "The token allows thirteen methods: list, fork itself, start and stop its own forks, diff, and request review."],
      ["caps", "At most 32 forks, three of them running."],
      ["denied", "It can never keep, delete, run commands elsewhere, or change access. What it produces waits in Pending changes."],
    ],
  },

  // ───────────────────────────── Beyond one Mac ─────────────────────────────
  {
    id: "connections",
    chapter: 5,
    title: "Connections",
    beats: [
      ["what", "Part five: beyond one Mac. Connections lets one Silo manage the computers of another device."],
      ["ssh", "The link is plain {SSH|S S H}. Your key is added on the other device, restricted to one command: Silo's bridge."],
      ["bridge", "The bridge passes your requests to that device's engine."],
      ["owner", "The computers, their secrets and their GitHub sign-in stay on the device that owns them."],
      ["authority", "A connected controller has roughly that engine's full authority. Its key does not expire, and revoking it does not close open sessions."],
      ["version", "The bridge has its own protocol version, and both devices must run the same one."],
    ],
  },
  {
    id: "linux-host",
    chapter: 5,
    title: "A Linux server as a host",
    beats: [
      ["service", "The engine runs on a Linux server as a user service, with no interface."],
      ["init", "silo host init checks for {KVM|K V M}, registers the service, and authorises your key."],
      ["device", "From your Mac, you add the device. Then most commands take a device option."],
      ["differs", "Two differences: secrets are files only that account can read, instead of a Keychain. And nothing there is signed yet."],
    ],
  },
  {
    id: "runner",
    chapter: 5,
    title: "The CI runner",
    beats: [
      ["what", "silo-runner makes a host the machine your GitHub Actions jobs run on."],
      ["fresh", "Each job gets a fresh computer, forked from a recipe's checkpoint."],
      ["cli", `The runner speaks to GitHub, and drives Silo only through the ${CLI}. The engine never learns GitHub's protocol.`],
      ["end", "When a job ends, its computer is deleted. Failed ones are kept a few days, for inspection."],
      ["private", "It refuses public repositories, and checks again before every job, because any pull request author could run code on your machine."],
      ["secrets", "A job's own GitHub secrets go straight into its computer. Silo's placeholders do not cover them."],
      ["live", "It was checked live on a Mac. A job through a Linux host has not been run yet."],
    ],
  },

  // ───────────────────────────── State of the project ─────────────────────────────
  {
    id: "build",
    chapter: 6,
    title: "Building and testing",
    beats: [
      ["stack", "Part six: the state of the project. The engine and the app shell are Rust. The interface is React and TypeScript. The runner is Go."],
      ["ci", "{CI|C I} runs the interface tests, the Rust tests on a Mac, and the engine's tests on Linux."],
      ["kvm", "A separate job boots a real computer on a Linux runner, and drives it end to end."],
      ["dev", "Development always uses the Dev build, which can not touch production data."],
    ],
  },
  {
    id: "release",
    chapter: 6,
    title: "Releases",
    beats: [
      ["two", "There are two release lines. The app, as a disk image that updates itself. And the engine alone, as an archive for other apps to embed."],
      ["embed", `An embedding app bundles ${SILOD}, signs it, and states which signature the engine must carry.`],
      ["notarized", "Today the app is not notarized, so the first launch needs your approval in System Settings. Updates are signed with a separate key, and always verified."],
      ["feed", "The engine has no update channel of its own yet. It updates with the app."],
    ],
  },
  {
    id: "gaps",
    chapter: 6,
    title: "What is missing, in one list",
    beats: [
      ["egress", "So, the honest list. There is no egress allowlist and no traffic limit: data the agent can read can leave."],
      ["echo", "Reflected secrets are not redacted."],
      ["socket", "The socket has no client identity and no audit log."],
      ["share", "The runtime's shared folder is unreviewed."],
      ["macos", `${MACOS} computers are exposed during setup, and are not covered by the {evals|evaluations}.`],
      ["pat", "Personal tokens are not scoped."],
      ["protocol", "The protocol accepts a single version, so every change breaks older clients."],
      ["docs", "And some documentation trails the code. The {README|read me} still calls the Linux engine the next milestone, though it runs."],
    ],
  },
  {
    id: "roadmap",
    chapter: 6,
    title: "The roadmap",
    beats: [
      ["now", "In progress: finishing the move to {Cua|Koo-ah} Driver for computer use."],
      ["next", "Next, needing no funding: an egress policy per computer. Redacting reflected secrets. Client identity and an audit log. A review of the shared folder. Finishing the Linux host, a protocol reference, and project governance."],
      ["then", "Waiting for funding: {evals|evaluations} in {CI|C I}, an external security review, signed and notarized releases, and published crates."],
    ],
  },
  {
    id: "recap",
    chapter: 6,
    title: "Silo in one minute",
    beats: [
      ["engine", "So. Silo is a local engine that gives each agent its own virtual machine."],
      ["clients", `Clients talk to it over one socket: an app, a ${CLI}, an {MCP|M C P} server, a {CI|C I} runner.`],
      ["hostile", "The agent is assumed hostile. It reaches public addresses only. It uses credentials it can not read. It sees only the repositories you grant."],
      ["loop", "You work by forking a base computer, reviewing what the fork changed, and keeping it or dropping it."],
      ["limits", "Its limits are stated: what the agent can read can leave, and anything running as you controls the engine."],
      ["end", "That is Silo."],
    ],
  },
];

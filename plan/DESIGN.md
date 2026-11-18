# Silo: design record

Date: 18 November 2026. This is the public copy: the section of evidence is left out.

This file records the design the owner and Claude reached in a discussion on that day. It is meant
to be the one base from which Silo's documentation and plan are rewritten. It covers what Silo is,
what it does, how its core is built, how to say it, the evidence behind it, and what is still open.

Where it differs from what was written before (a fork per task, a diff of the whole machine, a
merge into main), this file is the newer word. Appendix A lists every difference.

## Contents

0. How to read this file
1. Silo in short
2. Words
3. Computers
4. The boundary
5. The record
6. Going back
7. What you see
8. Recipes, tasks and moving in
9. The engine
10. Properties
11. How to say it
12. How to reason about an addition
13. Evidence (not in this copy)
14. Open questions and things to find out
15. What the discussion said about order
16. Later

Appendix A. What changes in what was recorded before
Appendix B. Sources

---

## 0. How to read this file

### 0.1 Marks

Every statement that matters carries a mark. A mark on a heading covers the whole section unless a
line carries its own.

| Mark | Meaning |
|---|---|
| **Decided** | Recorded by the owner before this discussion: in `plan/decisions.json`, in `plan/PRODUCT.md`, or under "Decided" in a feature's notes. Appendix A says which of these this design changes. |
| **Agreed** | Put forward in this discussion and accepted by the owner in it. Not yet recorded as a decision. |
| **Owner** | Said by the owner in this discussion, as an idea or a direction. |
| **Notes** | Proposed in the plan's notes before this discussion. This design relies on it and keeps it. It is still a proposal. |
| **Proposed** | Claude's proposal, made in this discussion or while writing this file. The owner has not answered it. |
| **Open** | A choice nobody has made. |
| **To find out** | A fact to establish by research or by measuring. |

### 0.2 Two rules for whoever uses this file

1. **Only "Decided" is a decision.** "Agreed" and "Owner" are the owner's words in a conversation.
   They become decisions when the owner records them. "Notes" and "Proposed" must not be written
   down as decisions.
2. **The evidence is not here.** It comes from private conversations and stays out of this
   repository: no quote, no count, no name.

### 0.3 Where it comes from

- The discussion of 18 November 2026, in three exchanges: the product seen from the research; the
  record as the core, with labels, rules and plugins; the owner's agreements and the priority given
  to macOS.
- Private research into what heavy users of agents struggle with, taken from group chats, and
  read again in full for this discussion. It is kept outside this repository.
- The architecture conversation of 16 November 2026 and its reading notes, kept outside this
  repository.
- The plan's notes in the Silo repository as they stood on 18 November 2026.

---

## 1. Silo in short

### 1.1 One sentence (Proposed wording; the design behind it is Agreed)

> Agents work in a computer of their own. Silo records everything they do there, shows you what
> matters, and takes you back to any moment.

The computer is a virtual machine on your Mac. It runs macOS or Linux. Nothing of yours is inside
unless you put it there.

### 1.2 Who it is for

- The first user is the owner, on a Mac. A developer in a company fits the same product. Team
  features come later, on the same product. (Decided)
- Developers and teams who run many agents and move fast. (Owner)

### 1.3 The problems it answers

1. A useful agent needs full access. On your own machine that puts your files, your keys and your
   accounts in its reach.
2. You cannot see what agents are doing while they run, and after a long run you cannot tell what
   they did or what it cost.
3. When an agent breaks something you cannot go back. Part of the damage happens outside the
   machine, where no restore reaches.
4. Agents leave things behind: full disks, processes that keep running, folders nobody can tie to
   a session.
5. Rules that live inside the agent program are walked around by the agent.

### 1.4 The idea

**Only agents work there.** (Owner) On your own laptop, a recording of everything would be mostly
you, and nobody wants that. In a computer where no person works, every process, every file change
and every connection is agent work. So a complete account is possible there, and it means
something.

**One position gives all of it.** The agent runs inside a machine whose network, disk and screen
pass through parts of Silo that it cannot touch (the boundary, section 4). From that one position
Silo can limit what the computer reaches, save and restore it, count what it spends, and record
what happened.

| | A boundary the agent cannot touch | An account of what happened |
|---|---|---|
| Sandboxes and virtual machines for agents | yes | no |
| Agent programs | no: their rules live inside, next to the agent | of themselves only |
| Silo | yes | of everything in the computer, under any agent program |

### 1.5 The shape (Agreed)

- One long-lived computer, **main**, where your agents work.
- **The record** of everything that happens in it. This is the core of the product.
- **Going back**: checkpoints, taken on their own, that you can restore or fork from.
- **Forks** for the moments that call for one: a risky try, a tool to test before trusting it, a
  task that must run on a clean machine.

The earlier shape was a fork per task, a diff of the whole machine, then a merge. Merging a fork's
machine state into main is not part of the core any more (6.3).

### 1.6 What Silo does not do

- It writes no agent program of its own and bundles none. It runs the vendors' programs,
  unmodified. (Notes)
- It never chooses or switches a model. (Notes)
- It does no orchestration and passes no messages between agents. (Notes)
- It builds no chat window. You talk to your agent in a terminal inside the computer, or through
  a tool that connects in, such as T3 Code. (Notes; T3 Code connecting in is Decided)
- It does not hide or wrap the agent program's own interface. (Notes)
- It builds no board for tracking work. (Notes)
- It is not a sandbox around one process on your own machine, and not a project folder on your
  Mac. (Decided)
- It is not a CI product. (Decided)
- It is not a memory system for agents. An agent's memory is the files of its computer. (Notes)
- It does not merge one computer's state into another, item by item. (Agreed)
- It disguises nothing: it does not hide that an agent runs in a virtual machine. (Notes, Agreed)

---

## 2. Words

One word per idea. The words for computers are Git's (Decided).

| Word | Meaning |
|---|---|
| **Silo** | The product: the engine, the Mac app and the `silo` command. |
| **computer** | A virtual machine that Silo runs. It has macOS or Linux inside. |
| **main** | The long-lived computer where your agents work. |
| **fork** | A copy of a computer, or of one of its checkpoints, that then lives its own life. |
| **checkpoint** | A saved moment of a computer. It never changes once saved. |
| **restore** | Put a computer back to a checkpoint. |
| **drop** | Remove a fork. It goes to the bin first. |
| **bin** | Where a dropped fork waits for a time before it is removed for good. |
| **boundary** | The edge of a computer. Everything that enters or leaves it passes through parts of Silo that run outside it. The discussion also called it "the wall"; use "boundary". |
| **record** | Everything Silo has written down about what happened: all the facts. |
| **fact** | One thing that was observed, with its time, its computer and where it was seen. |
| **label** | An opinion computed from facts: which run a fact belongs to, what kind of file this is, "stuck". |
| **rule** | Data that blocks something, holds it for your answer, or tells you about it. "Block" is the word for a rule; "stop" is kept for a computer. |
| **plugin** | A program that reads the record and reports. It runs in a computer. |
| **receipt** | The account of one run, one task or one fork, made from the record and not by the agent. |
| **flag** | What a rule or a plugin raises for your attention. |
| **ask** | A request that waits for your answer because it would widen access. |
| **agent program** | Claude Code, Codex and the like. The chats call it a harness; use "agent program". |
| **run** | One launch of an agent program, and every process descended from it. |
| **conversation** | One exchange with a model. A run can hold several: subagents, a cleared session. |
| **turn** | One reply of the model in a conversation. |
| **tool call** | One thing a turn asked for: a command, a file edit, a search. |
| **trace** | What a tool call left behind: processes, file changes, connections. The discussion called it "effect"; that word is kept for the engine's request to a driver (9.1). |
| **recipe** | A text that says how to make a computer. |
| **task** | A command run in a fork or in a fresh computer, with an end and a receipt. |
| **only here** | Said of something that nothing else would bring back if it vanished (6.4). |
| **secret** | A value a computer can use and cannot read. |
| **login** | A sign-in to a model provider, by subscription or by key. |

Words not to use for the product: "operating system for agents", "sandbox", "safe", "proven",
"observability" (section 11).

---

## 3. Computers

### 3.1 Main

- There is one main computer, and the user's agents live in it. Forks and fresh computers serve
  tasks. (Decided)
- Several agents working in one main is the normal case. A fork is the exception. (Agreed)
- Whether a user has one main or several (one per client, or one per level of trust) is Open in
  the notes. The research has signs for several.
- Main holds several repositories, not one. (Agreed)
- Nothing from your device is shared into a computer. (Decided)
- Windows and apps an agent opens stay inside the computer. They never take the focus or the
  screen on your device unless you open them. (Notes)
- A computer cannot reach your device: not your Mac, your local network, your VPN or cloud
  metadata. (Notes)
- Inside, agents have full access (`sudo`) and nothing asks them for approval. Limits are set at
  the boundary, never inside. (Notes, Agreed)

### 3.2 macOS and Linux

**The owner's direction (Owner, 18 November 2026):** main is a macOS computer first, and forks are
macOS or Linux. The owner expects much more interest this way.

Claude first proposed Linux for main and macOS later, then agreed with the owner, for these
reasons (Proposed):

1. The visible thing people already do is give their agent a Mac of its own.
2. A macOS computer is what Linux sandboxes in the cloud cannot offer.
3. Agents that build Mac or iOS software need macOS. That includes the agents that build Silo's
   own app.
4. Under this design a fork is occasional. The limit of two macOS computers per Mac blocked the
   earlier shape, where every task took a fork. It does not block this one.

Facts this direction has to respect (Notes, from earlier work on macOS computers):

- Apple allows two macOS computers running on one Mac. With main on macOS, one macOS fork can run
  beside it.
- A fork of a macOS computer is a clone of its disk and boots cold. A saved memory state cannot
  move to a new identity under Apple's framework.
- Apple forbids shipping a pre-installed macOS image. Each Mac downloads and installs its own, so
  the first run takes longer than with a Linux image.
- A macOS computer needs a second VM engine, Apple's virtualization framework, driven from Swift
  on the engine side (Decided). Its network goes through Silo's boundary, never Apple's NAT.
- A macOS computer is larger than a Linux one, in disk and in memory.

What follows for the design (Proposed):

1. **A Linux computer is never a copy of a macOS main.** It is made from a recipe, and the code
   reaches it as a commit (8.2). So recipes and tasks come early, not late.
2. **Containers.** A macOS computer is not expected to run virtual machines, so Docker would run
   in a Linux computer beside main, as it does on any Mac. (To find out) The owner finds a Linux
   computer that holds several containers appealing. (Owner)
3. **The record's two halves.** What the boundary sees (network, model calls, secrets, spend,
   checkpoints, the screen) is the same for both systems. What is seen from inside (which process
   wrote which file) has to be built once per system (9.7).
4. **If the inside half is not ready on macOS**, the first version there ships with boundary facts
   only. That is still most of the product, and the facts say where they were seen.
5. **x86.** On a Mac, a Linux computer is arm64. Real x86 Linux needs a Linux device as host
   (Decided: the engine also runs on Linux).
6. **The engine core stays neutral.** Nothing in the core knows which system a computer runs
   (9.4).

macOS and Linux computers stay consistent: the same page in the app, the same image contents, an
upgrade path (Decided). An agent can drive the screen of a macOS computer as well as a Linux
desktop (Decided).

### 3.3 Who works in a computer

- Agents do. Each launch of an agent program is a run (5.2).
- You visit: a terminal over SSH, your editor opened on a folder, T3 Code connecting in. (Notes)
- Anything not descended from an agent program is you. The record tells the two apart by the
  process tree. (Proposed)

### 3.4 Size and memory (Agreed)

- A Mac holds a few running computers, not fifty. Silo says so plainly.
- Memory decides how many fit. A computer that is idle gives memory back to the Mac. This moves
  early in the order.
- A fork shares its parent's disk at block level and stores only what changes afterwards. So a
  fork starts with main's build output and pays only for what it rebuilds. (Notes)
- A cache that several computers can write to is a path between them. It stays out until it has
  been measured. (Notes)
- The many-forks case belongs on a server or a cloud machine, later.
- Sizing aims at about 16 agents running at once on one device as the heavy case, and one or two
  forks as the common case. (Notes)
- The owner wants memory handling to be measured and to be a point where Silo is better than the
  alternatives. (Owner)

---

## 4. The boundary

### 4.1 The framing (Decided)

- The agent is considered hostile.
- Rules exist to make an attack predictable: a hostile agent can do what the rules allow and
  nothing else.
- No destination is special. The user sets the rules for GitHub as for everything else.
- Silo is as secure as the rules the user sets, and it makes a secure setup as easy as the rest.
- Security is built properly and is not the current focus.

What follows (Notes):

- Silo enforces the rules exactly and shows what they allow. The user chooses the rules.
- Everything Silo guarantees is done by the engine, outside the computer. None of it rests on a
  prompt or on the agent program's own checks.
- Each rule states its worst case in plain words when it is set. Example: allowing this host means
  anything this computer can read can be sent there.
- A fresh install with nothing configured is the safe setup.

What the discussion added (Agreed):

- Rules that live inside an agent program are not a boundary. Agents fetch a package by hand to
  get past a rule on package age, or read a file through the shell when the file tool refuses.
- Isolation is wanted for speed as much as for safety: full access with no approval prompts.

### 4.2 Doors (Notes)

Every channel between a computer and the device is listed, has a rule, and none exists by default:
ports, files, secrets, the screen, the clipboard, repository access. Each says what it lets
through and in which direction.

### 4.3 Secrets (Notes)

- The computer holds a placeholder. The real value is swapped in by the network stack that carries
  the computer's requests, only on the way to the hosts you allowed. No proxy is configured inside.
- Nothing run inside a computer yields a secret's real value.
- A real value is never written to the record, a log or an event.
- A secret is entered in Silo's own prompt, outside any chat, so the agent learns only its name.
- A secret can be limited to reading (sent only with GET requests), and can have an end time.
- Swapping a secret into a request to a model provider leaves the rest of the request unchanged
  byte for byte, so the provider's prompt cache still hits.

### 4.4 Logins and accounts (Open)

The owner's position (Owner, 18 November 2026): how people set up their accounts is their own
business, and Silo has no say in it. Sharing one login between computers, or switching between
several accounts, is something people do if they want. The owner does not yet have a clear idea of
this part and does not want it designed before the whole design is understood. Claude had proposed
that onboarding state each provider's terms; the owner saw that as taking on a responsibility
that is not Silo's, and the proposal is withdrawn.

What is recorded and stands:

- Agent logins need special handling so that a login does not end up in forks or shared computers
  by accident. How is decided later. (Decided, 10 November 2026)
- A subscription login is supported as fully as an API key. (Notes)
- Silo disguises nothing. (Notes, Agreed)

What is known and will bear on the design:

- A fresh computer that receives a copy of a login looks like a new machine to the provider and
  gets signed out.
- Several computers that hold the same login can fight over refreshing it.
- Many people hold several accounts and switch between them by hand.
- Two mechanisms exist. Either the vendor's program keeps the login inside the computer, or Silo
  holds it outside and swaps it in like a secret. The second makes one login work in every computer
  and makes assigning an account a rule. Whether a subscription sign-in works with substitution at
  all is To find out (Notes).
- The choice is tied to 5.8: if Silo holds logins, it already opens the traffic to model providers.
- Anthropic's terms, read on 18 November 2026, say that a developer may not collect, store or pass
  on the credentials of a Claude subscription, and that the sign-in must complete through
  Anthropic's own flow. An API key that its owner places in a secrets manager is allowed. So the
  second mechanism fits an API key and does not fit a subscription login. The terms of other
  providers were not read.
- Reading the traffic (5.8) holds no login. Claude Code documents that a proxy which inspects
  traffic works once its certificate is trusted.
- Claude Code has an official long-lived token for a machine with no window (`claude
  setup-token`). It can only make model requests. Whether a user may keep it as a Silo secret is
  a question for Anthropic.

What the owner said after reading this (Owner, 18 November 2026): how a login gets into a computer
is not for Silo to solve. A person connects the vendor's own app the official way, over SSH, or
runs the agent program inside the computer with an API key held as a Silo secret. The question
that stays open is wider than logins: how agent apps connect to computers at all (8.5).

One line of the notes is now in question and is the owner's to settle: "Silo builds nothing that
helps get around a provider's terms, such as rotating accounts".

### 4.5 Network rules

- The network policy is set by the user, with requests from the agent. (Decided)
- Each computer is open, limited to a list of hosts, or closed. Opening or closing is one step.
  (Notes)
- Nothing done inside a computer can change or invalidate its network rules. (Notes)
- Main has rules of its own. Holding every project does not let it reach everything. (Notes)
- An allowed host is still a way out. So the record shows what was sent to each host, not only
  which hosts were reached. (Agreed)
- A host seen for the first time is flagged. Blocked attempts are recorded with the host, the time
  and the process. (Agreed, Notes)
- Rules can be proposed from the record: run open for a while, then keep the hosts that were
  actually used. (Agreed)
- A rule on the age of packages is enforced at the boundary, because agents get around the same
  rule set inside a tool. (Notes for the rule; Agreed for where it sits)
- A fork can run with its network closed, for a check or a benchmark, and its record shows the
  network stayed closed. (Notes)
- Calls that change something elsewhere (section 5.3, "outside actions") are listed. A rule can
  say "ask each time" for a named secret or a named destination. A blanket preset "read freely,
  hold every write" is not offered, because a read carries data out too. (Notes)

### 4.6 Budgets

- A spending limit is enforced outside the agent, where model traffic leaves the computer. (Notes)
- Spend is shown in dollars or as a share of a plan, never as raw tokens. (Notes)
- A budget on a computer, and a budget on a conversation, can be exact, because the boundary sees
  both. A budget on a run depends on a label and can only tell, not block. (Proposed)
- Whether a budget blocks or holds at each threshold, and whether it can be a rate, is Open in the
  notes.
- All of this depends on 5.8.

### 4.7 Asks

- Approvals are native to Silo: shown in the Silo app, two-way with the agent, not a prompt of the
  agent program. No broker agent. (Decided)
- An ask exists only for something that widens access. It is shown in Silo's words, with exactly
  what it would allow. Work inside a computer never prompts per command. (Notes)
- No model decides a widening of access. A cheap model may only label a request, and the label is
  shown as a guess. (Notes)
- A pending ask holds only the call that asked. The computer keeps running. (Notes)
- An ask has to reach you where you are. A chat app or a phone may notify and lead back to Silo;
  it does not answer. (Notes; the phone app itself comes later, Owner)

### 4.8 Stop

- One action pauses every running computer, and one resumes them. (Notes)
- Stopping or deleting a computer, or dropping a fork, ends every process started in it, detached
  ones included. (Notes; the owner called this useful)
- Stopping a computer, removing a secret or closing a network never waits for an approval. (Notes)

---

## 5. The record

The owner's words for it: a good system to track what happens (which agent made which calls,
when, with which secret, at what cost, which files it wrote and deleted), and a way to label it
(which session wrote what, what might be a problem, what takes space and can be purged), with
rules on top and a base on which people can notice patterns. The owner called this the core.

### 5.1 Four layers and the rule between them (Agreed)

| Layer | What it holds | Who writes it | Can it be wrong? |
|---|---|---|---|
| **Facts** | Every process, file change, connection, model call, secret use and checkpoint. Append-only. Each fact says where it was seen. | The engine and its drivers | Not when it was seen from outside: a gap is recorded as a gap. A fact reported from inside is what the inside said (5.2). |
| **Labels** | Which run a fact belongs to, what kind of file or call it is, "stuck", "repeated", "risky". | Functions over facts, plus data about agent programs and ecosystems, plus the user's answers, plus plugins | Yes. So labels are recomputed from facts and never written into them. |
| **Rules** | Block, hold, tell. | The user, as data. Silo can propose them from the record. | A rule that blocks or holds reads only facts seen from outside the computer. A rule that tells may read labels. |
| **Plugins** | Anything that looks and reports: a script, a classifier, an agent, a notifier, a company's set. | Anyone | A plugin can flag and, if the user grants it, checkpoint, pause or stop a computer. It cannot approve or change a rule. |

**The rule between the layers: nothing flows down.**

- An opinion is never stored as a fact.
- A rule that blocks or holds never depends on an opinion.
- Something that watches never widens access.

"Down" means toward what is trusted more, and the law is these three sentences, no more. A plugin
that adds labels crosses nothing: its labels are opinions like any other. A plugin that pauses or
stops a computer on an opinion crosses nothing either: the second sentence binds rules, and a
pause or a stop takes access away and gives none. (Proposed)

Why it matters. A guess does harm when it is stored as truth and built on: every later guess
inherits the error. Here the facts hold no opinion, and each says where it was seen. Everything that is a matter of
judgement (what can be rebuilt, which run did it, what looks wrong) is a function on top. A better
function replaces a worse one and the labels are computed again. Nothing is migrated. (Agreed; the
owner's words: it is much easier to improve step by step than something that needs migrations.)

This is the same idea as the engine's "data first, with events" (Decided): a log of facts is the
primary thing, and every other structure is a view derived from it that can be rebuilt (Appendix
B, Kleppmann).

### 5.2 Facts

**What a fact is.** One thing that was observed. It carries its time, its computer and its
source. It is written once and never edited. A correction is a new fact.

**Sources.** Each fact says where it was seen, and the app shows it.

| Source | What it sees | How far it can be trusted |
|---|---|---|
| **engine** | Every call that changed something, every decision, checkpoint, ask and answer | Certain |
| **boundary: network** | Every connection in and out, and the content of traffic it opens (5.8) | Certain |
| **boundary: disk** | What changed on the disk between two moments (how is To find out, 9.7) | Certain |
| **boundary: screen** | The computer's display and the input sent to it | Certain |
| **inside** | Processes and which process touched which file, reported from inside the computer | As told from inside. An agent with full access can interfere with it. Never used to block or hold anything. |

**Kinds of facts** (Proposed; the list is meant to stay short and fixed)

| Kind | What it says |
|---|---|
| control | A computer was created, started, stopped or dropped. A checkpoint, a fork, a restore. A rule was set. An ask was raised and answered. A secret was given or taken back. A plugin was given access. Each with who did it. |
| process | Started: command, parent, user. Exited: code, and totals of processor, memory and disk use. |
| file | Created, changed, deleted or renamed: path and change in size, and the process when known. |
| connection | Host, port, bytes each way. Allowed, blocked or held, and by which version of which rule. For opened traffic: method, path, status and sizes. The secret used, by name. |
| model call | Provider and login by name. Model asked and model that answered. Tokens in, out and cached, and cost. Conversation and turn. The tool calls the reply asked for, and the result sent back for each, which includes the output of commands. A refusal or a reroute. What the provider says is left of a limit. |
| screen | A stretch of recording while an agent drives the screen, and the input events. |
| meter | Disk, memory and processor use of a computer, sampled. |
| gap | A source lost facts between two moments, with the count when known. |

**The chain** (Agreed). Facts hang on one chain. It is what lets a flag open on the reasoning step
that produced it, which is the use the owner asked for.

```text
computer
  run              one launch of an agent program, and every process descended from it
    conversation   one exchange with a model
      turn         one reply of the model
        tool call  one thing the reply asked for
          trace    what it left: processes, file changes, connections
```

Where each link comes from:

| Link | Known from | Certainty |
|---|---|---|
| tool call → turn → conversation | Model traffic at the boundary | Certain, when the traffic can be read (5.8) |
| trace → process | Inside | As told from inside |
| process → run | The process tree, and a data list of agent programs | As told from inside |
| conversation → run | Which process owned the connection | As told from inside |
| tool call → the processes it started | Time. Ambiguous only when one process runs several tool calls at the same instant | A label |

**Not hooks** (Agreed). The owner suggested catching tool calls through each agent program's
hooks. The discussion settled on the opposite:

- Every tool call is already visible at the boundary. It is in the model's reply, and its result
  is in the next request. That needs three or four provider formats, not one adapter per agent
  program. The exception is a local model running inside the computer, whose calls never cross the
  boundary.
- A hook is the agent program reporting on itself. The app shows what the engine saw, never what
  an agent reports. (Notes)
- A hook sees the call, not its traces. One build command writes tens of thousands of files, and a
  server it starts keeps writing after the call returns. The traces are the disk problem.
- Which run did something is the operating system's knowledge: a run is a process tree.
- A hook earns a place in one case: several subagents in one process running tools at the same
  instant. There it can stamp the call's identifier on the command it starts.

**The books must balance** (Agreed). This is how the design avoids "best effort", which the owner
wants avoided because it compounds.

- What runs are shown to have written adds up to what the boundary saw change. What is left over
  is shown as "unattributed". It is never spread over the runs.
- A source that loses facts records a gap. The record of a run says how it was captured and how
  complete it is. (Notes)
- Target (To find out, per system): attributing work to the wrong run should be impossible without
  an act that itself leaves a fact. Work that cannot be attributed is shown as unattributed.

**Other rules for facts**

- File reads are not recorded on the disk: a build reads everything, and it would be noise. What
  entered a model's context is counted in the model traffic, in tokens. (Agreed)
- A flood coarsens the present and never erases the past. Under pressure, facts of one computer
  are merged (per folder, per host) and marked as merged. Older facts are not evicted to make
  room, because an agent could then erase its trail by making noise. (Proposed, from a risk in the
  notes)
- Git history inside a computer is content the agent controls. It never serves as the record.
  (Notes)
- Nothing written inside a computer can change the record. A restore or a drop cannot erase it.
  (Notes)

### 5.3 Labels

**What a label is.** An opinion computed from facts by a labeler. A labeler is a function: facts
and label rules in, labels out. Labels are stored apart from facts, carry the labeler's name and
version, can lag behind the facts, and can be deleted and computed again at any time.

**Built-in labelers** (Proposed list)

1. **Runs, and the run of each conversation.** The joins of the chain above that the boundary does
   not see. A conversation itself is read from model traffic at the boundary (5.2).
2. **How a path would come back** (6.4): by a pull, by a rebuild, or by nothing ("only here").
   Also a coarse kind: code, build output, dependency, cache, agent data, scratch, log,
   configuration, credential-like.
3. **Kind of call.** Model, package registry, Git, an outside action (a call that changes
   something elsewhere), other. And whether the host is seen for the first time.
4. **State of a run.** Working, waiting on the model, waiting on a command, idle, or stuck: no
   model call and no process activity, with a process that sleeps or waits for input.
5. **Stretches of a run.** Orienting, thrashing or progress, from exit codes and what was read
   and written. (Notes)
6. **Repeats.** The same command, the same install, the same content entering context again.
7. **Owner of leftovers.** The last run that wrote a path, and whether that run is still alive.
8. **Heat.** Paths written by many runs. (Agreed: the map only, see 5.7)

**The user's own labels** (Owner). The app can ask "is this a build?", and the user's answer marks
the path. An answer is stored as a label rule (a path pattern and a kind). It applies to the past
as well as the future, because labels are recomputed.

**Improving labels** (Owner, Proposed). The user's answers are also test cases. A new version of a
labeler is run on recorded facts and compared with the answers before it replaces the old one.

**What a label may read.** A labeler may read content inside a computer, such as the state of a
Git repository. That content is under the agent's control. This is acceptable for a label and is
the reason a label never blocks or holds anything. Such a label, and a label from a plugin or a
model, can come out differently when it is computed again. It carries where it came from.
(Proposed)

**How a label is shown.** As a label. A guess is shown as a guess. (Notes)

### 5.4 Rules

- A rule is data. Every rule is set by picking among choices, and can be shown and set as text
  through the command line. Using Silo never requires writing code. (Notes)
- A rule does one of three things: **block**, **hold** for your answer, or **tell** you. (Agreed:
  "block" is the word for a rule, and "stop" is kept for a computer.)
- A rule that blocks or holds reads only facts seen by the engine or at the boundary. It is
  enforced at the boundary, and each decision is recorded with the version of the rule. (Agreed)
- A rule that tells may read labels. It raises a flag. (Agreed)
- Silo can propose rules from the record, such as the list of hosts a computer actually used.
  (Agreed) The owner's wording: rules that are opinionated depending on usage.
- A named set of rules can be switched on a computer in one step. (Notes)
- Each rule states its worst case when it is set. (Notes)

Examples of rules that block or hold: a host not on the list; a secret sent to a host it is not
bound to; a budget on a computer or on a conversation reached; a package younger than a set age.

A rule that blocks or holds carries what it needs as its own data, such as the hosts and paths of
package registries. It does not read the label "kind of call". (Proposed)

What Silo does when the Mac or a computer runs low on memory or disk is Open (14.1, question 14).

Examples of rules that tell: a run with no activity for a set time; a disk growing fast; a host
seen for the first time; writes that no run accounts for; a change to a file that looks like a
credential.

### 5.5 Plugins (Agreed)

**What a plugin is.** A program that reads the record and reports. It runs in a computer. The
notes already hold the rule: Silo is extended by a client of the engine's calls or by a program
inside a computer, and nothing loads into the engine or the app at run time.

**Its access.** Reading the record of the computers you name. This is a new kind of access, and
like any access it is granted by the user and states its worst case: the plugin can read
everything recorded about those computers and send it wherever its own network rules allow.

**What it receives.** The facts of those computers (as a stream and by query), their labels, and
their state.

**What it emits.**

- Labels.
- Flags, each pointing at the facts that caused it, so it opens on the chain.
- Panels, as data that the app draws. (Notes)
- Questions to the user.

**What it may do, if granted.** Take a checkpoint, pause a computer, stop a computer.

**What it can never do.** Answer an ask, change a rule, give a secret, write into another
computer. No model decides a widening of access. (Notes)

**Why it runs in a computer.** It reads text written by the agents it watches: file names,
commands, web pages. That is the path by which an agent could give it instructions. In a computer
it has the same boundary as everything else. If it sends to a chat app, that is a network rule you
granted, and a line in the record. A plugin has a receipt like any agent.

**Three tiers** (Agreed). Each wakes only when the one before cannot judge.

1. **Rules.** In the engine. They always run and cost nothing.
2. **A classifier.** A plugin with a small model. It sorts what the rules cannot judge: normal,
   risky, needs you.
3. **An agent.** A plugin. It wakes when the classifier escalates or when you ask it a question.

Nothing Silo itself does to watch or report spends the user's tokens (Notes). What a plugin
spends is the user's choice, and it is metered like any computer.

**The monitor agent** (Owner's idea, Agreed form). The owner proposed an agent shipped inside the
app, with tools to use the app and look inside computers, not allowed to change files or use the
network, sitting at the app's level with the whole picture. The agreed form is the third tier
above: the user's own agent program, in its own computer, with read access to the record. Silo
ships the recipe, the instructions and the commands. It is not Silo's own agent and it does not
run beside the app.

**Built-in plugins** (Owner). Silo ships some, and people write their own. Shipping them as files
anyone can read is Proposed. A notifier is the first example: with it an ask or a flag reaches a
phone long before Silo has a phone app.

**Why plugins at all** (Owner). People want different ways to watch a lot of work and hold
different opinions about it. The owner's list for the interface: what a plugin receives, what it
can fetch from Silo, how it hooks in, how it sends things outside or into the app, and extra tools
it brings. The first four are covered above. Extra tools that a plugin would offer to agents in
other computers are not designed, and a route between computers is a door (Open).

**Knowing what a plugin can do, and what it does** (the owner's question; the answer is Proposed).
The owner wants to plug something in and know, completely, what it can affect on the network and
the disk, and what goes in and out of a computer. Three kinds of knowledge answer it.

- **What it can do, at most.** This is known before it runs, from its access alone: the rules of
  its own computer, and the calls its access allows. Silo can compute it as paths: from which
  computer data can reach which host. Giving a plugin read access to main adds one path: what is
  recorded about main, to the plugin, to the hosts the plugin may reach. The app shows the paths
  an access adds before the user confirms it.
- **That this bound is true of Silo.** This is where formal methods fit (9.5). The rule that
  access widens only through the user's own input is checked on every sequence of calls of a small
  model, by the simulator on the real core, and by hostile tests on the real boundary. It is
  checked once for Silo, not once per plugin.
- **What it does.** This is measured. Because facts are kept, a plugin can be run against recorded
  history before it goes live. Silo replays past facts to it in a fresh computer and shows its
  receipt: what it flagged, which calls it made, where it tried to connect. For a classifier, the
  user's own answers give a score.

### 5.6 Receipts

The owner called the receipt the core of what Silo gives. A receipt is the account of one run,
one task or one fork. It is made from the record. The agent does not write it. Its job is to turn
an hour of agent work into something a person can judge in a minute.

What it holds:

- the request that started the run, word for word
- the commands that ran, with their exit codes
- the checks that ran, and their results
- the files that changed, grouped, and what is only here
- the hosts reached and what was sent to them
- the secrets used, by name
- the model calls: spend, which models answered, limits reached
- the time, split between work and waiting
- the asks raised and their answers
- recordings of the screen, when an agent drove it
- what could not be attributed, and any gap

The agent's own summary can be shown beside it, marked as the agent's. A summary written by an
agent is untrusted content. (Notes)

Each action that was approved or that widened access leaves a receipt as well: what was approved,
what was done, and the result. (Notes)

### 5.7 Patterns (Agreed)

The owner wants the record to be a ground on which people notice patterns and improve them:
repeated calls, repeated writes and reads, the shape of agents' work.

- Patterns are shown as lists ranked by what they cost in time, money and disk. Example: an
  install that ran 41 times in 12 runs, with its total time and size.
- Graphs of agents are not the main view. They met doubt each time they came up in the research.
- Subagents are shown as a tree.
- Hot files: the record can show which paths every run touches and who is writing what now. Silo
  ships that map. It does not build the part that splits work between agents.

### 5.8 What all of this depends on: reading model traffic at the boundary

Spend per conversation, budgets that block, the model that really answered, the tool calls, the
turns, and telling "stuck" from "working" all come from the traffic between an agent program and
its model provider.

- To read it, the boundary has to end the encrypted connection for those hosts, with a
  certificate the computer trusts. This is the mechanism secrets already use.
- Silo reads this traffic. It changes nothing in it except a secret or a login it swaps in.
- A subscription sign-in and an API key make no difference to reading. Both are requests to the
  provider over an encrypted connection, and they differ only in the credential they carry.
  (Owner, 18 November 2026) The earlier notes did not plan to open traffic under a subscription
  sign-in, only because nothing was swapped into it.
- What has to be verified is narrower: that each agent program accepts a certificate from Silo.
  Companies inspect traffic this way, and agent programs offer a setting for a company
  certificate, so this is expected to work. It is a test to run once per program.
- One point is specific to a subscription. The provider then sees a connection made by Silo's
  network stack and not by the agent program, as it does behind any company's inspecting proxy.
  Whether a provider treats that differently is To find out.

To find out, for each agent program and provider:

- Does the program accept Silo's certificate, or does it refuse the connection?
- Which transport does it use (plain requests, a stream, a persistent socket), and can each be
  read?
- Does the sign-in flow of a subscription still work?
- Does reading leave requests unchanged byte for byte?

If a program cannot be read, the boundary still gives hosts, sizes and timing, and the
conversation would have to come from the program's own files inside the computer. Those facts
would carry the source "inside".

---

## 6. Going back

### 6.1 Checkpoints

- Silo takes checkpoints on its own while agents work: at intervals, and at moments the engine
  sees, such as each ask. Nobody has to remember. (Notes, Agreed)
- When the model traffic can be read, a turn is such a moment. (Proposed)
- A checkpoint is stored where the agent in the computer cannot reach it. It never changes once
  saved. (Notes)
- A restore first saves where you were. (Notes)
- A restore to a moment in the middle of a tool call can repeat an outside action,
  so the moment before the call is the one to use. (Notes)
- Old automatic checkpoints are thinned, and removing them really frees space. (Notes)
- A checkpoint that also keeps running programs (memory) comes later. (Notes)

### 6.2 Two grains, each with its own promise (Agreed)

**A whole computer at one moment.** It is consistent, because the agents' conversations are files
on that disk. A restore or a fork returns the disk and the conversations to the same moment, and
an agent program's resume works there. (Notes for the last point)

- On a main shared by several agents you do not rewind. You **fork from the moment**. The past is
  read-only, you continue from it in a fork, main loses nothing, and the result leaves the fork
  through Git.
- Restoring main itself remains possible. Everyone in it goes back.

**One run inside a shared main.** It gets a smaller promise: **put back these paths.**

- The record lists what the run changed.
- Paths that nobody touched since go back in one step. The others are shown.
- Inside a repository, Git is the tool, and the record says which commits came from which run.
- Rewinding one run's conversation stays the agent program's job.

Undoing one agent's work among several, automatically, is what every agent program attempts and
gets wrong. The owner called it scope creep on a shared computer, and this design does not attempt
it.

### 6.3 Forks (Agreed)

**When a fork is made.**

- To let an agent loose on something risky.
- To try a tool or an update before trusting it: install it in a fork, read what it wrote and
  where it connected, then keep it or drop it. (Proposed)
- For a task that must run apart or on a clean machine (8.2).
- To drive a screen without competing for main's.
- To hand someone the exact state of a computer. (Later)

**What a fork gets.**

- No secrets, and read-only repository access, unless its parent hands over more. Never more than
  the parent has. (Notes)
- A new identity. Its scheduled jobs and named services are off, so it never repeats what its
  source does. (Notes)
- Its own display. (Notes)

**How a fork ends.** In exactly one outcome, which is reported.

- **Dropped.** It goes to the bin and can be brought back for a time. (Proposed.) Before the drop, Silo shows what is only here (6.4), and chosen files can be brought back
  to the parent.
- **It becomes main.** The old main is kept aside. If main changed since the fork was made, Silo
  first shows what is only in the old main.

Code leaves a fork through Git at any time.

**What is no longer in the core.** Merging a fork's machine state into main, item by item, with
conflicts. The owner's words: the whole merging and diffing should not be the core at all. The
view of what changed on a computer since a moment stays, as part of the Disk view (7.3).

### 6.4 What stays on disk: how it would come back (Agreed)

The owner wanted to know, when several runs work in a repository, which run did what, what was
pushed, and what remains: a build, a cache, or something to keep such as an instruction added to a
main instruction file. The owner was wary of sliding back into merging, and reminded that most
people work with Git and GitHub, so most things can be pulled again.

The answer is one question asked of every path: **if this vanished, what would bring it back?**

| What brings it back | Examples | How Silo knows | What Silo may do |
|---|---|---|---|
| **A pull** | Commits that are on the remote | Git, exactly | Nothing is needed. |
| **A rebuild** | Dependencies, build output, package caches | A list of known names per ecosystem, the recipe, and the user's answers | Offer to purge, biggest first. Leave it out of long history. |
| **Nothing: it is only here** | Unpushed commits, uncommitted work, an untracked `.env`, an edited instruction file, a new skill, a database, anything unknown | Everything not in the first two | Never purge it. List it before a drop. Offer to save it. |

Anything unknown counts as "only here", so a mistake falls on the safe side.

What this gives:

- **Purge** offers what a rebuild brings back, biggest first.
- **Dropping a fork** is safe when "only here" is empty. Otherwise Silo shows the list.
- **Keeping something from a fork** means saving its "only here" items: push the commits, copy the
  few files. There is no merge.
- **A package installed by hand** and absent from the computer's recipe is "only here". Silo
  offers to add it to the recipe.
- **Checkpoints stay light**: what a rebuild brings back does not need a long history (9.6).
  This lets a label decide what is forgotten, which is Open (14.1, question 16).

This is the diff of the whole machine reduced to the part worth reading.

---

## 7. What you see

The sketches below show content, not design. How a screen is built is governed by the
repository's interface rules.

### 7.1 The window and the first screen (Agreed)

The first screen is a short ranked list of what needs you. It is not a dashboard.

```text
 main        ● 3 agents     NEEDS YOU
  └ try-bun  ○ fork, idle    codex      asks to reach api.stripe.com            [allow] [deny]
 client-a    ● 1 agent       claude #2  nothing for 38 min, it runs `sleep 600`  [open]
                             disk       +41 GB today, six copies of target/      [reclaim]
                             network    new host paste.rs, 2.1 MB sent by codex  [block]
                            DONE
                             claude #1  1 h 12 · $6.40 · 38 files · tests passed [receipt]
 [Checkpoint] [Fork] [Stop all]                      Now · Disk · Network · Timeline
```

- Everything that waits on the user sits in one queue across all computers, ordered by
  importance. Each item is answered in place. (Notes)
- Silo notifies only on a change that needs a person. It never sends a stream of every event.
  (Notes)
- Each line of the list opens one of the four places below.

### 7.2 Now

Each run with one line: what it runs, since when, whether it is working, waiting or stuck, and
what it has spent. The line comes from observed facts. A model's own estimate is never shown as a
fact. (Notes)

### 7.3 Disk

What takes space, who put it there, what changed and what was produced.

```text
 78 GB  ~/work/app/target ×6   builds      claude #2, codex   today    [reclaim 65 GB]
 52 GB  ~/.codex/sessions      agent data  codex              +3 GB a day
 31 GB  ~/work/app-wt-* (9)    worktrees   no live run        9 days   [bin]
 Changed since 14:00   code 38 files · packages +3 · services +1 · ~/.ssh/authorized_keys ⚠
 Only here             2 unpushed commits · .env · CLAUDE.md (edited)
 Produced              report.html · 4 screenshots · run.mp4
```

### 7.4 Network

Where the computer connected, what it sent, what it installed and what it spent.

```text
 api.anthropic.com  1,204 calls  $18.40  claude #1, #2  14 answered by another model ⚠
 github.com           212 calls  3 pushes to you/app    secret used: github
 registry.npmjs.org    58 calls  3 packages younger than 3 days ⚠
 paste.rs               1 call   2.1 MB sent  codex     first time seen ⚠   [block]
 Blocked            your Mac ×2 · cloud metadata ×4
```

### 7.5 Timeline

One line of time per computer: checkpoints, turns, commands, asks, recordings.

```text
 13:00 ──●─────○──────◆────────●──▶ now      ● checkpoint  ○ turn  ◆ selected moment
 ◆ 13:42  claude #2 · "clean the build" · 3 commands · −12 GB
          [fork from here] [restore to before] [put back paths]
```

- A routine stretch collapses into one line with a count. Decisions and thrashing stay visible.
  (Notes)
- A flag opens here, on the turn that led to it.

### 7.6 The same record for agents (Agreed)

Everything the app shows can be read with the `silo` command, as plain text that a program can
parse. An agent can then answer "what did that run do" from facts and not from guessing.

Whether a computer may read its own record by default is Open.

### 7.7 Principles

- Lists ranked by cost, not graphs. A tree for subagents. (Agreed)
- Facts and labels look different on screen. A label is never shown as a fact. (Agreed)
- The app is views only and holds no logic. (Decided)
- Three levels: a status, an overview with a small answer to give, the detail. (Notes, idea)

---

## 8. Recipes, tasks and moving in

### 8.1 Recipes

- A recipe sets up a whole working computer, and making one must be convenient. (Decided)
- A recipe is a property of a computer that Silo stores, and its rules live in the app. (Decided)
- A `.silo` folder in a repository on the Mac, with a flow that starts from a project folder, is
  not what Silo is. (Decided)
- A recipe is written in an existing format that people know and models write correctly. Making
  one can start from a project's Dockerfile or devcontainer. (Notes)

**The owner's idea (Owner, 18 November 2026).** A repository can hold recipes, in the repository
itself or in the app: one for macOS computers, one for Linux computers. Tasks use them.

**How the two fit (Proposed).** The lesson behind the decisions above was that an agent can edit a
rules file that sits in a repository. So:

- The source of a recipe may sit in a repository. It describes the machine and nothing else.
- Silo stores the recipe it built from that source, as a property of the computer.
- Rules stay in the app.

Whether this is how the owner wants it is Open (Appendix A).

**Recipes made of pieces (Owner, 18 November 2026).** A computer is blank by default: a bare macOS
or Linux machine. A recipe says what goes in it. Silo ships recipes, and people write and share
their own. A recipe is made of pieces that combine. Two examples: the tools of the computer that
watches the others (11.6), and computer use. Most pieces are the same for every agent program.
Some have a part that differs, because each program keeps its instructions and skills in a
different place.

- A piece is a small recipe, and a recipe can include other recipes. (Proposed)
- The data file of an agent program (8.3) also says where the program keeps its instructions and
  skills, so that a piece is written once for every program. (Proposed)
- A computer made from a recipe asks for no approval by hand for what the recipe installs, such as
  the permissions computer use needs. (Owner) The old Silo did this on macOS computers. During
  set-up it wrote the approvals for Accessibility and Screen Recording into the system's
  permission database, with System Integrity Protection turned off in the computer. A fork's disk
  copy carried them. Whether this holds on the current macOS, and for a fork of a running
  computer, is To find out.

**Nix (Owner, 18 November 2026).** The owner sees Nix as what recipes need: security,
predictability, and machines that can be rebuilt exactly. Silo audits its built-in recipes and
updates them often. A company does the same for its own. What Nix gives and costs as the base of
recipes was researched the same day.

What that research found:

- Nix pins every package of a machine to an exact version and checks signatures. It does not show
  that anyone reviewed what was pinned. The audit is work Silo does: a pinned list, a wait before
  taking new versions, and a reading of what changed between two versions of a machine.
- The products built on Nix for development machines hide it behind a short file or a command
  (Replit, Google's Firebase Studio, devbox, Flox). One that showed it to its users left it, for
  that reason among others.
- On Linux, Nix can describe the whole machine. On macOS it covers one layer: command-line tools
  and settings. The base system comes from Apple's installer, apps come through Homebrew, and
  privacy permissions are outside it.
- A machine managed wholly by Nix does not run a program downloaded from the internet without a
  fix. Agents download such programs all the time.
- Nix does not see what an agent installs by other means, and nothing was found that detects it.
  The "only here" list is what covers this (6.4).
- Agent programs are packaged for Nix within hours of their release, outside the main package
  set. A module exists that places Claude Code's settings, instructions, skills and servers. None
  was found for the other agent programs.
- The Nix store is heavy. Linux computers can share one store, read-only. Whether macOS computers
  can is To find out.

What follows (Proposed):

- Nix is the engine under recipes. It is not what a user writes: a user picks pieces and writes a
  short list.
- A recipe has the same shape on both systems: a pinned base system, then a layer built by Nix.
  On Linux the base is an ordinary distribution, so that what agents download runs.
- What Nix cannot describe is a step Silo owns: the base image, and on macOS the permissions of
  computer use.

**State and steps** (Notes, adapted). A computer can be described by its state or by its steps. A
checkpoint is exact and opaque. A recipe is readable and repeatable, and it can drift from the
computer it made. Silo needs both. The "only here" list (6.4) is the bridge between them: it
shows what is in main and not in its recipe.

### 8.2 Tasks

A task is a command run in a fork or in a fresh computer, with an end and a receipt.

The owner's example: while working, you want to test your changes. Silo makes a Linux computer
and a macOS computer from the repository's recipes, with the current changes, and runs the tests
in each.

How it works (Proposed):

- Main hands each computer one commit that includes the uncommitted work. The recipe brings the
  rest.
- The engine runs the checks, on a machine the agent never touched. The agent cannot fake that
  result. (Notes: checks that decide are defined outside the fork and run by the engine)
- The results come back as receipts, side by side.
- The limit of two macOS computers applies.
- A task started from outside starts from a recipe or a checkpoint, so it has no setup to wait
  for. (Decided)

### 8.3 Agent programs

- Any agent program runs in a computer, unmodified, including one Silo does not offer to install.
  The list of programs Silo offers is data. (Notes)
- Silo integrates with an agent program only through its documented surfaces. (Notes)
- What Silo needs to know about a program is a small data file: its name, where it keeps its
  sessions, how it resumes. (Proposed)
- The adapters that carry weight sit at the boundary and follow providers, not programs: three or
  four formats cover nearly all model traffic. (Agreed)
- A third kind of data covers ecosystems: which folders a rebuild brings back, which hosts are
  package registries. (Proposed)

### 8.4 Moving in

The owner's question: everyone has their own agent programs, providers and habits; how does all
of it work inside their Silo computers? The owner wants good onboarding for it. (Owner)

- First run reads what is on the Mac (agent programs, their settings, skills, repositories) and
  offers to bring each inside. (Proposed. Whether main copies from the Mac or starts from a guided
  recipe is Open in the notes.)
- With main on macOS, moving in keeps the same system, paths and tools.
- A computer is reached over SSH, so the terminal, the editor and the tools people already use
  keep working. (Notes)
- Silo needs no account, no sign-up and no key. It works with no setting touched. (Notes)
- First run must not fail. It sets up only what will work, and asks where it would otherwise
  guess. (Notes)
- With macOS, the wait for the first download and install is long. It has to show something worth
  looking at. (Notes)
- A user who leaves takes each computer as a standard disk image, and each recipe and rule as
  text. (Notes)
- How logins come in is part of 4.4 and 8.5, and is Open.

### 8.5 Connecting agent apps to computers (Open)

The owner's question (Owner, 18 November 2026): people use the app of their agent program on the
Mac and connect it to a machine over SSH. That can be the way into a computer. It is not yet clear
how it works with many computers. Adding each one to SSH by hand is too heavy, a user switches
between computers, and an agent may start other computers. The owner expects this to need several
discussions, and wants it researched first: similar tools, and people who have the same problem.

What is known:

- Over SSH, the app on the Mac is only the window. The agent program runs inside the computer, so
  its model traffic still crosses the boundary (5.8). The Claude desktop app installs Claude Code
  on the remote machine, and the Codex app starts its server there.
- The Claude desktop app appears to keep the login on the Mac and to give the session a
  short-lived token. With Codex, the sign-in is done on the remote machine. Where the token lands
  is To find out for each app.
- An agent that starts with no app connected has nobody to hand it a login. It needs a sign-in
  done inside the computer, or an API key.

The question in three parts (Proposed):

1. **Reaching a computer.** How any program on the Mac reaches any computer, including one made a
   moment ago, with nothing added by hand.
2. **Following the work.** A conversation lives in one computer. What the user's app shows, and
   where the conversation continues, when work moves to a fork.
3. **Agents making computers.** Whether an agent may ask for a fork or a fresh computer, and how
   its work there reaches the user.

What the research of 18 November 2026 found. Its files are kept outside this repository.

- No product moves a conversation from one machine to another. In every one, a conversation stays
  on the machine where it started, and the window attaches to it there.
- A window finds a conversation in two ways. The user points the app at a machine over SSH (the
  Claude desktop app, the Codex app, T3 Code, Zed, Cursor). Or the conversation announces itself
  through the vendor's account (Claude Code's Remote Control, the vendors' cloud sessions).
- No app documents a way for another program to add a machine to it. Most read the Mac's SSH
  settings. The Codex app lists only the machines named one by one there.
- What people report most is not adding machines by hand. It is signing in again on every fresh
  machine, several agents in one machine destroying each other's work, and losing sight of what
  sub-agents do.
- The tools people praise for following many agents show a list of pieces of work, and which one
  needs the user. They do not show a list of machines.
- The SSH mode of the desktop apps is young. It has open bugs with several conversations on one
  machine, and for Claude it takes no API key. A terminal over SSH is the path that always works.

What follows (Proposed):

- Silo does not try to make the window of an agent app follow the work. The agent app is where the
  user talks to one agent in one computer. Silo's window is where the user sees all the work, from
  the record, whichever agent program did it.
- Silo keeps one file of SSH entries: one named entry per computer, with its key. The Mac's SSH
  settings include that file once, with the user's yes. Every tool then reaches every computer by
  name, with no prompt.
- A fork carries the conversation with it, because it is a copy of the whole computer. The agent
  program resumes it there. Whether each program does so in a copied computer is To find out.
- From a run in Silo's window, one action opens a terminal in the right computer, resumed in the
  right conversation.

---

## 9. The engine

The engine is written in Zig. It is the core piece: performant, secure, easy to reason about. The
app, the command line and every tool only talk to it. Its design is data first, with events.
(Decided) The owner wants to go slowly here, to explore and research, and to build on bases that
are extremely solid. (Owner)

### 9.1 The shape (Notes)

```text
calls (app, command, agents) ──┐
driver results ────────────────┼─▶ one queue ─▶ core(state, input) ─▶ new state
clock ─────────────────────────┘                no disk, no network,   events  ─▶ clients follow
                                                no clock of its own    effects ─▶ drivers
```

- **State** is a small set of flat tables of plain rows that refer to each other by id.
- **A call** asks for a change. **An event** says what happened. **An effect** is a request to a
  driver.
- **The core** decides and touches nothing. **Drivers** do things: run a virtual machine, carry
  the network, take a checkpoint.
- **Wanted and seen.** Each row keeps what was asked and what was last observed. While they
  differ, the core issues effects to make them match, or says what it is waiting for. What it
  does when a computer stops by itself is Open (14.1, question 15). A virtual machine can die by
  itself and the device can reboot,
  so the saved state is never assumed to describe the world.

Three ideas stand behind it:

| Idea | Question it answers | What Silo takes |
|---|---|---|
| Data-oriented design | What is the state? | Flat tables, sizes known in advance. Not the tuning of memory layout: the engine tracks dozens of computers. |
| Event-driven design | How does state change? | Only through inputs, one at a time, each leaving a numbered event. Not a message broker: the engine is one program per device. |
| TigerStyle | How is the code written so it can be trusted? | Limits on everything, assertions, no allocation after start, plain control flow, no dependencies in the core, a simulator. In full in the core, where it fits in drivers. |

### 9.2 Two paths (Proposed)

The record adds a second path beside the first. The first decides. The second observes.

```text
 control path
 calls, driver results, clock ─▶ core ─▶ state, events, effects ─▶ drivers
                                           │
                                           └─ every event is also written as a control fact
 fact path                                                         ▼
 drivers at the boundary and inside ─▶ facts ───────────────▶ the record ─▶ labelers ─▶ labels
                                                                   │                      │
                                                                   └──── queries ─────────┴─▶ app, command, plugins
```

Why two. The core handles a few inputs a second for dozens of computers. A build produces file
changes by the tens of thousands. Facts at that volume must not pass through the core, and the
core must never wait for them.

Rules for the fact path:

1. **Facts do not pass through the core.** The core receives an input only when something has to
   be decided or its state has to change: something is held, a budget is reached, a source is lost
   or back, a checkpoint is done.
2. **Enforcing does not depend on recording.** The boundary enforces from the rules it was given.
   If the record cannot be written, facts wait in a buffer of fixed size, and then a gap is
   recorded.
3. **A driver enforces only rules the core issued**, identified by a version. Each decision names
   the version. What is wanted and what is seen applies to rules as to everything else.
4. **Observing never makes a computer wait.** Enforcing may: a connection waits for its rule.
5. **Every source numbers its facts.** A missing number is a gap, and a gap is a fact.
6. **Everything has a limit.** Under pressure a source merges facts before it loses any, and marks
   them as merged.
7. **A fact is written once.** A correction is a new fact.
8. **A secret's real value is never written.**
9. **Bulky content is stored once**, by its hash, apart from the facts that point to it: the text
   of a message, the output of a command, a recording.
10. **Labels are stored apart**, with the labeler's version. Deleting every label computed from facts alone loses nothing.

What stays true of the core: it contains nothing about agents (Notes). Runs are labels. A
conversation is read from model traffic at the boundary, and which run it belongs to is a label.
Model call facts come from the network driver. A budget is, for the core, a counter with a
threshold.

The record is also the history and the audit journal. The core's own saved state stays small.
(Notes: state plus the inputs since the last save is the suggested choice, still Open.)

### 9.3 Processes

| Process | What it holds | If it stops |
|---|---|---|
| **Engine** (core and server) | State and decisions | Running computers keep running under the rules they had. Changes and asks wait. |
| **Helper**, one per computer | The virtual machine and its boundary: the network stack with rules, secrets and reading; the disk; the screen | That computer stops. Nothing else does. |
| **Recorder** | The record, the built-in labelers, the rules that tell | Facts wait, then gaps. Enforcing is not affected. Flags stop. |
| **App**, command | Views | Nothing stops. |
| **Plugin**, in a computer | Its own work | Its flags stop. |

- The engine is its own program, not a library inside the app. (Notes)
- Computers outlive the engine. (Notes, suggested) The price is one helper per computer.
- A virtual machine lives in the process that created it. Whether a restarted engine can find its
  helpers and take them back is To find out, for Apple's framework and for MicroSandbox. (Notes)
- Whether the boundary lives in the helper, as the table assumes, is To find out. The notes also
  ask which process holds real secret values.
- Whether the recorder is its own process or a part of the engine program outside the core is
  Open. (Proposed either way: a labeler that crashes or runs long must never stop the core.)
- The Mac app is Swift and only renders. The client that speaks to the engine is Zig, linked into
  the app through a C interface, and it also serves the command line. (Notes; the owner corrected
  an earlier draft to this on 16 November)
- The code that drives Apple's virtualization framework is Swift, belongs to the engine side, and
  never runs inside the app. (Decided, Notes)

### 9.4 Rules for the core (Notes)

1. **One door.** Nothing changes state except an input handled by the core. Calls, driver results
   and the clock all enter the same way, through one queue.
2. **Deterministic.** The same inputs in the same order give the same state and the same outputs.
   Time and randomness are inputs. The core runs on one thread.
3. **Wanted and seen**, as above.
4. **An effect is a request, not a call.** It has an id and no return value. Its answer, or a
   timeout, comes back later as an input.
5. **Every effect is safe to issue twice**, or is marked once only and guarded by a key.
6. **Every call carries an id chosen by the caller.** Repeating it returns the first result.
7. **Limits are part of the model.** Tables have a fixed size. Reaching a limit fails with a
   named error.
8. **No memory is allocated in the core after start.**
9. **Assertions stay on in release builds.** On a broken assumption the engine stops and restarts
   from saved state. Rule 10 makes this acceptable.
10. **Computers outlive the engine.**
11. **No dependencies in the core.** Established tools sit behind drivers.
12. **Events are numbered.** A client is the state at number N plus every event after N.
13. **One name per concept everywhere**: a part of the plan, a group of calls, an engine file, a
    command, a screen.
14. **Every multi-step operation advances through explicit steps**, each safe to stop after. At
    start each driver reports what really exists, and the engine reconciles.
15. **When a failure forces a choice, the engine takes longer and never loses state or work.**
16. **Every input passes one validation at the door**, which caps every field.
17. **The core holds nothing about agents.**
18. **A new VM engine or device type is a driver**, with a fault model and a fake, added without
    changing the core.

The plan's notes for the engine hold these rules in more detail, with their tests. They do not
depend on the shape of the product and should be carried over (Appendix A).

### 9.5 The simulator (Notes, with additions)

- The core touches nothing, so a simulator can drive it with millions of random sequences of
  inputs, crashes included, and check every property after every step.
- Any message may be delayed without bound, dropped or reordered.
- Each fake driver misbehaves on purpose, according to a written fault model: a virtual machine
  dies, the disk fills, a reply never comes, a reply comes twice.
- Every real failure becomes a behaviour of a fake.
- On a failure the simulator removes inputs until the smallest failing sequence is left.
- The simulator proves the core against the fakes. The real drivers and the real boundary need
  their own hostile tests and live checks.
- A feature is ready to build when its properties can be written.

Additions for the record (Proposed):

- A fake source that drops facts, repeats them and delivers them late. The property: every loss
  shows as a gap.
- A fake inside that lies about which process did something. The property: no rule that blocks
  or holds changes its decision.
- A fake computer that floods. The property: older facts stay.

**Formal methods.** Properties are stated and checked as we build: "state properties, formal
verification, whatever shows what always holds" (Decided). The owner raised formal verification
again on 18 November 2026, for plugins in particular. There are four ways to check that something
always holds, and they are not the same thing (Proposed):

| Way | What it lets you say | Cost | Where it fits in Silo |
|---|---|---|---|
| Assertion | "The engine checked this while running, and would have stopped." | Low | Everywhere in the core |
| Simulation | "This held in every one of the random histories we ran, crashes included." | The simulator, built once | The real Zig core against fake drivers |
| Checking a model | "This holds in every history of a small model of the design, up to a size." | A model of a few hundred lines, kept in step | The rules of access: who can get what |
| Proof | "This holds for this code, for every input." | Very high: the proof is many times larger than the code, and every change to the code breaks it | At most one small function, later |

- The core is a pure function over small tables. That is the shape these tools need, which is why
  they are within reach here and are not for most software.
- **A model of access comes early.** The tables of who holds what, the calls that touch them, and
  one property: access widens only through an input from the user. A model checker tries every
  sequence of calls among a few computers, secrets and hosts. It costs little, it finds flaws in
  the design before any code exists, and it is short enough for a person to read. The simulator
  then compares the Zig core with it. The notes suggest waiting for the starter before proving
  anything; that still holds for proof, and this is not proof.
- **Proof stays later**, and only where testing is not enough. (Notes)
- **What none of this covers.** A model is not the code, and the core is not the boundary. The
  virtual machine engine and the network stack are checked by hostile tests on the real thing.
- **Words.** Say what was checked and how: "checked on every sequence of up to N calls", "held in
  these runs". Never "proven" for the product. (Notes)

**Where each kind of checking goes** (Proposed). A function can only be checked against a
statement of what it must do. Where such a statement exists, it is tested or proved. Where the
answer is an opinion, it is scored. The layers say which is which: what can block or hurt is
checked hardest, and labels were made unable to block anything so that they do not need proof.

| What | Is there an exact statement of "right"? | How it is checked |
|---|---|---|
| Recording facts | Yes: everything that crossed the boundary has a fact, or a gap | Hostile tests on the real boundary, the balance of 5.2, the simulator with faulty sources |
| The core's decisions: access, rules that block or hold | Yes: access widens only through the user | The model of access, the simulator, proof later if needed |
| A labeler's mechanics | Yes: for a labeler that reads facts alone, the same facts give the same labels; no fact changes; anything unknown is "only here" | Random inputs with these properties stated |
| A labeler's opinion | No: "is this folder a build?" has no exact answer | Scored against the user's answers, and compared with the previous version on recorded history |
| A plugin | Its bound, yes. Its behaviour, no. | Paths for the bound; a trial on recorded history for the behaviour (5.5) |

### 9.6 The store and its weight

The owner's view: this can be designed well and improved over time by measuring. Once something
exists, a variety of tests will show what can be done. (Owner) The discussion named two traps and
their fixes. (Agreed as direction; every number below is a rough estimate, To find out.)

**Trap 1: storing requests.** Each request to a model repeats the whole conversation so far. Raw
traffic therefore grows with the square of a conversation's length. Storing each message once, by
its hash, removes that. A request is then a short list of hashes.

**Trap 2: checkpointing what a rebuild brings back.** One large build rewrites gigabytes, and
each checkpoint keeps the old ones. The label from 6.4 is the fix: what a rebuild brings back
needs no long history. A wrong label would then lose history that cannot be computed again: Open
(14.1, question 16).

Design constraints (Proposed):

- Append-only files for facts, compressed, with an index for queries.
- Content stored once by hash for message text, command output and recordings.
- A size limit the user sets. Recordings and message text go first. Control facts are kept whole.
- The record's own disk use is shown in the Disk view like anything else.
- The record can be queried with ordinary tools, for example through a read-only export. (Notes)
- Export in a standard format is an idea for later. (Notes, idea)

Rough sizes for one busy computer per day: tens of megabytes for process, file and connection
facts once merged and compressed; a few megabytes per long conversation; recordings are the heavy
item and are kept only while an agent drives the screen.

One line of the notes changes here: "the conversation is disposable, and Silo keeps none".
Opening a flag on the reasoning step that led to it needs that step's text. This is the owner's to
settle (Appendix A).

### 9.7 What can be seen, per system

| | Linux computer | macOS computer |
|---|---|---|
| VM engine | MicroSandbox, pinned and patchable (Notes) | Apple's virtualization framework, driven from Swift (Decided) |
| Network at the boundary | Silo's network stack | The same. Never Apple's NAT. (Notes) |
| Model calls, secrets, budgets | At the boundary | The same |
| Checkpoint | A snapshot of the disk. Memory later. | A clone of the disk. A memory state can be saved, and is refused after some updates of the host. (Notes) |
| Fork | Shares blocks. Under two seconds in earlier measurements. (Notes) | A clone of the disk and a cold boot. One other macOS computer may run. (Notes) |
| Screen | Needs a desktop in the computer. (To find out, Notes) | Native. |
| File changes, seen from outside | To find out | To find out |
| Processes, seen from inside | To find out | To find out |
| Memory given back when idle | To find out (Notes) | To find out |
| Containers inside | Yes | Expected no. (To find out) |
| Image | One Ubuntu image, downloaded once. (Notes) | Downloaded and installed on each Mac. (Notes) |

**Seeing file changes from outside: two routes** (To find out; this is the main research question
for the record)

- **The engine serves the work folders itself.** Every file operation passes through it, live.
  The cost is speed, exactly where builds run. A shared folder or a filesystem layer is measured
  before it is adopted, and a slowdown of eight times on parallel file work is the failure to
  avoid. (Notes)
- **The engine reads the disk at each checkpoint and compares.** Nothing is slowed while the
  computer runs. The result is only as fine as the checkpoints, and the engine has to read the
  computer's filesystem.

**Seeing processes from inside** (To find out)

- Linux has the means: the process tree, accounting per group of processes, file events that name
  the process. Which to use, and what a hostile agent can do to each, has to be established.
- On Linux the kernel can come from outside the computer's disk. If it also refuses changes, its
  word is worth much more than a program's. (To verify)
- macOS has Endpoint Security, which gives exact process and file events. It needs a permission
  from Apple, or the system's own command-line tool with full disk access. Whether either can be
  set up with no clicks is unknown.

### 9.8 Speed (Notes, Agreed)

- Disk speed is already the bottleneck for people who run many agents. Observing must not slow a
  computer in a way a person can feel.
- Fork, start, checkpoint and the memory of an idle computer each have a budget, written before
  their code, as a 99th percentile and a worst case.
- The overhead of recording is measured the same way: a build and a parallel test suite, in a
  computer and on a plain machine.
- Measure distributions, with repeated interleaved runs. One run before and one after proves
  nothing on a laptop.
- The core will never be the slow part. Starting virtual machines, copying disks and the network
  are.
- Recovery time after a crash is a metric.
- Measurements are published once they hold, each dated and on a named machine. (Notes,
  suggested; Owner: measure excellently and make a difference there)

### 9.9 How it is built (Notes, Owner)

- Verification comes first: the simulator and the checks before the features.
- The first engine files become the pattern every later agent copies, so a person reads those line
  by line.
- Rules for code are mechanical checks, not text in a prompt.
- A property's statement is written by a person.
- **The owner's part on the engine** (Proposed, after the owner said on 18 November 2026 that
  agreeing with engine statements he cannot judge brings nothing). He is not asked to approve
  mechanisms. He decides the promises, which are the properties in plain sentences; the choices
  where a mechanism has a price the user pays; and he says where an explanation lost him and what
  he has seen go wrong in practice, which becomes cases for the simulator's fakes. Mechanisms are
  checked against the promises by the simulator, by reviews and by engineers he trusts.
- No time estimates. (Decided)
- Properties are stated and checked as we build. (Decided)

---

## 10. Properties

A promise should be one sentence that serves three times: as what the user is told, as what the
simulator checks, and as what a hostile test tries to break. All Proposed unless marked.

| Sentence | Kind | Checked by |
|---|---|---|
| A computer cannot reach your device. (Notes) | safety | hostile test on the real boundary |
| What a computer can reach is exactly what its rules allow. (Notes) | safety | hostile test |
| Nothing done inside a computer can change its rules. (Notes) | safety | hostile test |
| The real value of a secret never enters a computer or the record. (Notes) | safety | hostile test |
| A fork never has more than its parent. (Notes) | safety | simulator |
| A fork never acts as its parent. (Notes) | safety | live check |
| Stopping a computer ends every process in it. (Notes) | safety | live check |
| A checkpoint never changes, and nothing inside a computer can reach it. (Notes) | safety | hostile test |
| A fact is never changed or removed by a call. (Notes) | safety | simulator |
| A restore or a drop cannot erase the record. (Notes) | safety | simulator |
| Every loss of facts is itself recorded as a gap. | safety | simulator with a faulty source |
| What runs are shown to have done adds up to what the boundary measured, and the rest is shown as unattributed. | safety | live check |
| A rule that blocks or holds reads only what the engine or the boundary saw. | safety | simulator with a lying inside |
| Deleting every label computed from facts alone and computing them again gives the same labels. | safety | test |
| A flood from one computer never removes older facts. | safety | simulator |
| Access widens only through an input from the user. | safety | model of access, simulator |
| Nothing a plugin can call gives any computer, or itself, more access than it had. | safety | model of access, simulator |
| Every path by which data can leave a computer is among the paths Silo shows for it. | safety | hostile test |
| A budget blocks what it meters: once it is reached, no further metered request leaves. | safety | hostile test |
| A dropped fork can be brought back until its time in the bin ends. After that nothing of it remains. | safety | simulator, live check |
| If the engine stops, running computers keep running under the rules they had. (Notes) | safety | live check |
| No effect happens twice: one issued again changes nothing. (Notes) | safety | simulator with crashes |
| A repeated call does nothing twice. (Notes) | safety | simulator |
| A call the engine has accepted is never forgotten: it is carried out, or it fails with a reason. | safety | simulator with crashes |
| The app never shows a state the engine never had. (Notes) | safety | simulator |
| Nothing grows without limit. (Notes) | safety | assertions |
| Every change is recorded with who made it. (Notes) | safety | simulator |
| After a crash, every operation is finished or rolled back. (Notes) | liveness | simulator with crashes |
| What is wanted and what is seen agree in the end, or the engine says what it is waiting for. (Notes) | liveness | simulator |
| Recording slows a computer by no more than a stated amount. | measured | benchmark |

Rules for the list (Notes): each liveness property gives "in the end" a number; results are
reported as the runs covered, never as "proven"; Silo promises in public only the properties whose
check has run and passed.

---

## 11. How to say it

The owner's instruction: phrase the product in accurate, simple words backed by the research, and
treat the owner's own rough wording with caution. If interest comes from an accurate and simple
description, that is the best proof the product is interesting. (Owner)

### 11.1 The sentence and the reason

> Agents work in a computer of their own. Silo records everything they do there, shows you what
> matters, and takes you back to any moment.

The reason it can be said: only agents work there, so a complete account is possible.

### 11.2 What to say

- A computer of their own, macOS or Linux, on your Mac.
- Full access inside, and nothing asks for approval.
- You see what they did: what ran, what changed, where it connected, what it cost.
- You can go back to any moment, or continue from it in a fork.
- It works under any agent program.

Three things cap what a person gets from agents: what they dare let agents do, how fast they can
judge the result, and whether they can go back. The boundary answers the first, the record the
second, checkpoints the third. (Proposed, adapted from the notes)

Several cloud products checkpoint and fork virtual machines for agents, so "computers for agents"
does not set Silo apart. The record does, together with going back and macOS computers on your
own Mac. (Proposed; the market was checked lightly on 16 November 2026, not surveyed)

### 11.3 What not to say (Agreed)

- **"Operating system for agents."** The owner used it as a rough image and asked that it be put
  in perspective. In the research it is a running joke. It can stay as a private long view.
- **"Sandbox."** A tired word, and people cannot tell sandbox products apart.
- **"Safe", "secure", "proven".** Silo enforces the rules the user sets and shows what they allow.
  Say "enforced", "what the rules allow", "checked by a simulator against these failures".
- **"Observability", "dashboard", "monitoring".** People ask for less to look at, not more.
- **Fear.** Safety is not what this audience asks for. Full access without worry is.

### 11.4 Rules for presenting it (Notes)

- Lead with the problem, not with a category.
- Say what it replaces, with pictures: the Mac bought for the agent, the spare laptop, the cheap
  server, worktrees, the dashboard built by hand.
- The first run must not fail.
- People understand by seeing it, then by examples, then by documentation. The research has a
  request for a short video of going back in time.
- It must be easy to try alone.
- Open source is part of the trust. What sits between a person and their agent has to be readable.

### 11.5 Objections to expect, and the answers (Proposed)

| Objection | Answer |
|---|---|
| "I run agents with every check off and take backups." | A backup returns files. It does not show what was sent out, what was spent, or what was done elsewhere. |
| "A virtual machine per agent is too heavy." | One main holds all your agents. A fork is for the exception. |
| "I can build this myself." | Some do, repeatedly. Silo is one command, and the parts they do not build: the record, and going back for the whole machine. |
| "Session timelines already exist." | Those are an agent program's view of itself. Silo's is the computer's view, under any program. |
| "Watching agents is easy to do yourself." | For logs, yes. Not for the disk, the network and the past of a whole machine. |
| "The built-in sandbox of my agent program is enough." | Its rules live next to the agent, and the agent gets around them. |
| "Sandbox products already exist." | They give a boundary. They do not say what happened inside or take you back. |

### 11.6 The image from the television series (Owner)

The owner took the name from the series *Silo* and finds that this design fits it better than the
first one: Silo 1 watches the other silos, wakes specialists when needed, talks to each silo
through a narrow defined channel, is unknown to them, and can end a silo that becomes a problem.

What the design takes from it (Agreed):

- Silo 1 is a silo too: what watches lives in a computer.
- One narrow line links it to each silo: the door through which an agent asks.
- It can seal a silo, and it never does a silo's work: plugins can stop, never widen.
- Specialists are woken when needed: the three tiers.

What it leaves: Silo 1 lies to the silos. This product promises the opposite: the record is what
happened.

The image is an aid for thinking. It is not wording for users.

---

## 12. How to reason about an addition

Three questions for anything proposed for Silo (Notes, adapted):

1. **Is it a door?** If something crosses a computer's boundary: who opens it, and what does the
   user see first?
2. **Does it need a new noun?** Everything is a computer or is defined through one (a checkpoint
   is a computer at a moment, a fork is a copy, a recipe is how to make one, a task is a command
   in one with an end), or is one of the four layers of the record (fact, label, rule, plugin). A
   receipt, a flag and the timeline are made from those. An addition that needs another noun
   deserves suspicion.
3. **Which layer is it?** A fact, a label, a rule or a plugin. If it is an opinion, it is a
   label, and it must not be stored as a fact. If it watches, it is a plugin, and it must not
   widen access.
4. **Does it make the result faster to get, or faster to judge?** If neither, it is later.

Concepts worth knowing (Notes):

- **Ambient authority** is the name of the first problem: on your own machine every program you
  run can do everything you can.
- **A boundary is as strong as it is narrow.** A sandbox around a process shares your whole
  system. A container shares the kernel. A virtual machine shares a handful of virtual devices.
  With a virtual machine the boundary is rarely what fails. The doors opened on purpose are.
- **Why "hostile" is the right assumption.** An agent is steered by what it reads: a web page, an
  issue, a dependency. It becomes dangerous when it has three things at once: private data,
  untrusted content, and a way to send data out. Untrusted content cannot be removed, so the rules
  act on the other two.
- **Access is handed down, never up.** A computer cannot get more than it has (Decided). You can
  pass on part of what you hold, never more.
- **Attention is scarce.** People who are asked often click yes, and the answer stops meaning
  anything. So everything inside the rules is free and silent, and an ask is rare.
- **Git versions files; Silo versions something alive.** A file does nothing when copied. A
  computer has an identity, scheduled jobs, open sessions and logins. That is why a fork must not
  act as its parent.

---

## 13. Evidence

The evidence behind this design comes from private conversations. It is kept outside this
repository and is not part of this copy.

---

## 14. Open questions and things to find out

### 14.1 For the owner to settle

1. **Logins and accounts** (4.4). The whole part.
2. **Is the text of conversations kept** (9.6)? The notes say none is.
3. **Reading another computer's record as a kind of access** (5.5). It has to exist for plugins.
4. **May a recipe's source sit in a repository** (8.1)?
5. **How a fork ends** (6.3): dropped or becomes main, in place of merged or dropped.
6. **One main or several** (3.1).
7. **May a computer read its own record by default** (7.6)?
8. **Does a budget block or hold** at each threshold (4.6)?
9. **The order of work** (section 15).
10. **Is a small model of access written and checked before the first engine code** (9.5)?
11. **Is "Git for computers" still the framing?** The decision `git-words` named it. The words
    stay Git's, but merging one computer into another has left the core. At the reset of
    18 November 2026 the home page took an interim headline, "Your agents work in a computer of
    their own.", and the decision's text was cut down to the words.
12. **How agent apps connect to computers** (8.5). The whole part.
13. **Are recipes made of pieces, and is Nix their base** (8.1)?
14. **What does Silo do when the Mac or a computer runs low on memory or disk** (5.4)? It was
    listed among the rules that block or hold, without saying what happens. The owner's first
    thoughts, rough: for disk, what many apps already do; for memory, pause and tell; how early
    is a setting with a few presets. (Owner) To settle with the product and its design.
15. **What does Silo do when a computer stops by itself** (9.1)? The engine notices it the way it
    notices anything: what is seen no longer matches what is wanted. Then it can (a) start the
    computer again and tell you, and after a few failures in a row stop trying and ask; (b) start
    it again silently, as often as needed; (c) leave it stopped and tell you. Under each, whatever
    was running in it is gone; whether agents start again with it is part of the question.
    (Proposed: a.) It was a choice under the film `engine-one-call`. The owner moved it here on
    18 November 2026: it is decided with the whole design.
16. **May a label decide what a checkpoint keeps** (6.4, 9.6)? Leaving out of long history what a
    rebuild brings back rests on a label. If the label is wrong, that history is gone, it cannot
    be computed again, and a restore does not bring back the whole computer. (Proposed: only under
    a rule the user set, and such a checkpoint says what it left out.)

### 14.2 To find out

On the record:

1. Can the boundary read the model traffic of each agent program (5.8)?
2. Which route gives file changes from outside, and what does each cost (9.7)?
3. Which means give processes from inside on Linux, and how far a hostile agent can interfere
   (9.7)?
4. Can macOS computers give processes and file events with no clicks (9.7)?
5. Can attributing work to the wrong run be made impossible without leaving a fact (5.2)?
6. How much disk the record takes on real work, per kind of fact (9.6)?
7. What recording costs in speed on a build and on a parallel test suite (9.8)?

On computers:

8. How long the first run takes with a macOS main.
9. Whether a macOS computer can run containers.
10. Whether macOS and Linux computers can give memory back when idle, and how much.
11. Whether a restarted engine can take back a helper per computer, on both VM engines.
12. Which process holds the boundary and the real secret values.
13. Which VM engine runs Linux computers on a Mac, given that the macOS one is built first.
14. Whether a subscription sign-in works with substitution at all.
15. Whether the old Silo's way of giving a macOS computer the approvals of computer use still
    works on the current macOS, and for a fork of a running computer (8.1).
16. Whether each agent program resumes a conversation in a copied computer (8.5).
17. Whether macOS computers can share one Nix store read-only, and what the first build of a
    recipe costs in time and disk (8.1).
18. Where the token lands when each agent app connects over SSH (8.5).
19. Where the engine's events are kept until the recorder has them, so that control facts never
    have a gap (9.2, 9.6).
20. How a conversation is recognised in each provider's traffic, and whether a program can get
    around a budget on a conversation by starting a new one (4.6). A budget on a computer has no
    such way around.

---

## 15. What the discussion said about order

The order of work is the owner's. No time estimates (Decided). What the discussion fixed:

- The record is the core. Merging is not.
- Main on macOS, and forks on macOS or Linux, have priority. (Owner)
- Recipes and tasks are wanted, and with a macOS main they are needed early, because a Linux
  computer can only be made from a recipe.
- Giving memory back when idle, and forks that share their parent's disk, move early. (Agreed)
- The hot-file map comes once the record exists, since it costs little then. (Agreed)
- Plugins follow the record. A notifier is the first.
- Recording the screen when an agent drives it is useful and not first. (Owner)
- The phone app is later. (Owner)
- Speed is part of the guidelines from the start. (Owner)

---

## 16. Later

- A phone app.
- A desktop in Linux computers, and recordings of computer use on both systems.
- Other devices: a Linux server as host, moving main to another machine, the same main on several
  devices.
- Teams: a shared main, several people watching, shared sets of rules and plugins.
- Checkpoints that keep running programs.
- Forks on a cloud machine.
- A place where people share recipes, rules and plugins.
- Export of the record in standard formats.
- A report, before installing, of what agents on your Mac can reach today.
- Each computer with accounts of its own.
- Comments on what a turn changed, sent back to the agent.
- What agents say they lacked: mistakes, learnings and wishes written by agents, shown per
  computer.

---

## Appendix A. What changes in what was recorded before

This appendix is for whoever rewrites the plan. It names existing decisions and notes by their
identifier in the repository. Nothing here is itself a decision.

### A.1 Decisions in `plan/decisions.json`

| Decision | Under this design |
|---|---|
| `git-words` | The words stand. The part that says "one verb, merge (fast-forward when main is unchanged, item by item with conflicts asked otherwise), plus replace main" changes: item-by-item merging leaves the core, and a fork ends by being dropped or by becoming main. The owner agreed in the discussion and has not recorded it. |
| `agent-lives-inside` | Stands. Sharpened: several agents in one main is the normal case, and a fork is the exception. |
| `security-framing`, `security-not-the-focus` | Stand, and are central. |
| `agents-get-every-tool` | Stands. Plugins get no call that widens access. |
| `engine-in-zig` | Stands. "Data first, with events" is what the record extends. |
| `mac-app-in-swift`, `engine-on-linux`, `runs-anywhere` | Stand. |
| `mac-and-linux-consistent` | Stands, and matters more with main on macOS. |
| `properties-checked` | Stands. The list of properties is section 10. |
| `tasks-start-ready` | Stands. |
| `first-user`, `one-feature-at-a-time`, `no-time-estimates`, `old-code-outside` | Stand. |
| The others (`github-is-storage`, `workshop-is-public`, `home-page-says-what-silo-is`, `recordings-in-the-repo`, `pages-are-short`, `stale-docs-are-deleted`, `agents-may-push`) | Not touched by this design. |

### A.2 Decided lines in feature notes

| Note | Under this design |
|---|---|
| `approvals`, `network-policy` | Stand. |
| `computer-use`, `macos-computer-use` | Stand. |
| `t3code` | Stands. |
| `working-recipe`, `recipes` | Stand. Whether a recipe's source may sit in a repository is for the owner (8.1). |
| `project-folder` | Stands as written. The owner's idea of recipe files in a repository has to be squared with it (8.1). |
| `logins-stay-put` | Stands. The "how" is Open (4.4). |
| `merge-and-recipe` | Concerns merging, which leaves the core. |
| `ci-runner`, `containers`, `personal-tokens`, `hosted-reviewer`, `resident-agents` | Not touched. |

### A.3 Lines of `plan/PRODUCT.md` that no longer describe the design

- "In one sentence" and "The loop": the shape is now main, the record, going back, and forks for
  the exception (1.5).
- "What comes first": see section 15.
- "Not a project folder on your Mac: there is no `.silo` folder and no setup kept in a
  repository": the second half is in question (8.1).
- "The problem": section 1.3 gives the problems the research supports. "Every new task
  starts with setup that was already done once" has support in one form only: worktrees that
  arrive without dependencies. "Agents working in parallel collide" holds mostly for files
  in one repository, which a copy of the machine does not fix.

### A.4 Proposed lines in the notes that change

| Note | Line | What changes |
|---|---|---|
| `main-and-forks` | "The conversation is disposable, and Silo keeps none." | In question (9.6). |
| `models-and-providers` | "Silo builds nothing that helps get around a provider's terms, such as rotating accounts." | In question (4.4). The half about not hiding the virtual machine stands. |
| `models-and-providers` | Open: "Does Silo run a model of its own?" | Answered by the three tiers: Silo itself runs none; a plugin may (5.5). |
| `fork` | "A fork's life ends in exactly one outcome, merged or dropped." | Dropped, or becomes main (6.3). |
| `spend-limits`, `spend-per-fork` | They ask whether spend under a subscription sign-in can be counted at the network, where Silo did not plan to open the traffic. | This design opens it, the same way for a subscription as for a key (5.8). |
| `logins-stay-put` | Suggested: only the vendor's program touches a subscription login. | Open again (4.4). |
| `tools-inside` | The agent docs describe the loop of fork, diff and merge. | To be rewritten around the record. |
| `properties-checked` | Suggested: nothing is proved formally before the starter is in daily use. | Stands for proof. Checking a small model of access is proposed early (9.5). |

### A.5 Notes this design is built from and that should be carried over

They do not depend on the shape of the product, or they become parts of the record:

- The engine: `engine-service`, `calls-and-events`, `crash-recovery`, `properties-checked`,
  `jobs`, `engine-style`, `core-ownership`, `order-of-work`, `zig-and-tree`, `vm-runtime`,
  `speed-and-shipping`, `on-the-device`. Where they name fork, diff or merge as the loop, the
  wording changes and the rule stays.
- The boundary: `guarantees`, `no-route-home`, `secrets`, `network-policy`, `approvals`,
  `agent-rule`, `hostile-content`, `emergency-stop`, `access-view`, `package-age`,
  `resource-floors`, `pause-idle`.
- The product's stance: `agent-programs`, `built-in-or-left`, `where-you-direct-agents`,
  `what-the-app-tells`, `rules-recipes-formats`, `agents-as-users`, `words-and-names`,
  `starting-and-leaving`, `make-main`.
- Parts of the record: `audit-log`, `session-record`, `task-record`, `steps`, `fork-status`,
  `process-list`, `storage`, `notifications`, `spend-per-fork`, `spend-limits`,
  `provider-limits`, `dns-log`, `credentials-used`, `honest-checks`, `custom-panels`,
  `engine-query`.
- Going back: `checkpoints`, `checkpoint-cleanup`, `checkpoint-memory`, `fork`, `quiet-forks`,
  `fork-access`, `replace-main`.

### A.6 Entries built around merging

`diff`, `merge`, `merge-conflicts`, `undo`, `no-credentials-in-merge`, `rebase-forks`,
`merge-requests`, `merge-queue`, `auto-merge-rules`, `merge-hooks`, `compare-forks`,
`fork-overlap`, `declared-scope`, `diff-noise`, `diff-map`. They assume a fork's machine state is
merged into main. That is outside this design's core. What happens to each is the owner's call.
Two ideas inside them survive in another form: seeing what changed on a computer since a moment
(the Disk view), and seeing where several runs write the same files (the hot-file map).

---

## Appendix B. Sources

**The discussion.** 18 November 2026, in the Silo repository's working session.

**The research.** Private, kept outside this repository.

**The architecture conversation of 16 November 2026**, with its notes, kept outside this repository.
Its parts on the engine, the security framing, properties and how to build are the base of section 9
here. Its parts on the loop of fork, diff and merge describe the earlier shape and were not used.

**Public texts behind section 9**

- [TigerStyle](https://tigerstyle.dev/) and
  [TIGER_STYLE.md](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md):
  limits, assertions, no dependencies, a deterministic core.
- [sled simulation guide](https://sled.rs/simulation.html): the simulator as a recipe.
- [sled performance guide](https://sled.rs/perf.html): measuring distributions, worst cases.
- [Turning the database inside-out](https://martin.kleppmann.com/2015/03/04/turning-the-database-inside-out.html)
  (Martin Kleppmann): a log of facts as the primary thing, and every other structure as a view
  derived from it. This is the ground under "facts, then labels".
- [NATS Server 2.15: desired state reconciliation](https://www.synadia.com/blog/how-we-move-streams-safely-nats-2-15-desired-state-reconciliation):
  wanted and seen, done by people who first got it wrong.
- [Integrating Zig and SwiftUI](https://mitchellh.com/writing/zig-and-swiftui) (Mitchell
  Hashimoto): a Zig core linked into a Swift app through a C interface.
- [The lethal trifecta](https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/) (Simon
  Willison): private data, untrusted content and a way to send data out.

**The plan's notes** in the Silo repository, under `plan/features/`, as they stood on 18 November
2026.

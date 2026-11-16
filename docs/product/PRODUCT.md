# Silo: the product

This page says what Silo is, who it is for and what comes first. It is the source of truth for
product questions: agents read it before they build, and the owner changes it by deciding
something (see [decisions](DECISIONS.md)). How people use Silo, step by step and with what works
today, is in [journeys](JOURNEYS.md). The other pages in `docs/` describe how the code works.

Status: first draft of 2026-11-16, written from the owner's answers. Not yet checked against the
running app.

## In one sentence

Silo gives an agent its own computer, lets it copy that computer for each task, shows you
everything the copy changed, and lets you merge it or drop it.

## Who it is for

A developer who runs several coding agents and wants them to work with full access, without
giving them their own machine. The first user is the owner, on a Mac. A developer inside a
company fits the same description. Team features come later, on the same journeys.

## The problem

- An agent that is useful needs root, the network, credentials and a real environment. On your
  own machine that puts your files, keys and accounts in its reach.
- Agents working in parallel collide: the same ports, the same database, the same checkout.
- A Git diff shows the code an agent changed. It does not show the packages it installed, the
  settings it edited or what it did to the database.
- Every new task starts with setup that was already done once.

## The loop

1. **A computer.** Your setup and your agent live in one computer, called main. It is a virtual
   machine on your device. Nothing from your device is shared into it.
2. **A copy per task.** The agent, or you, forks main, or starts a fresh computer from a recipe
   or a checkpoint. The copy is a whole machine, so two tasks never share a port or a database.
3. **Everything that changed.** The diff covers code, packages, settings, files and databases.
4. **Merge or drop.** Merging brings the changes into main and asks you first. Dropping leaves
   nothing behind.

This loop is the product. The parts around it (secrets an agent can use but not read, GitHub
access per repository, network rules per computer) exist so that the loop is safe to leave
running. Other tools have those parts; none has the loop for a whole machine.

## What comes first

Three journeys, in this order. They are described in [journeys](JOURNEYS.md).

1. **You live in a computer.** Your tools open into main as easily as they open a local folder.
2. **A computer per task.** You or your agent make another computer for a task and bring the
   result back.
3. **A task started from outside.** Something outside Silo (a GitHub event, a schedule, another
   program) starts a task in a fresh computer, on your device or another one, and gets the
   result.

The aim is the simplest version of these three that is solid and pleasant, then more.

## Everything else

Silo has many features that are not part of these three journeys. They stay in the product. Each
is reviewed in turn against one question: does it make the three journeys better? The verdict is
recorded in [decisions](DECISIONS.md). Until a feature is reviewed, nobody extends it.

## Rules that hold everywhere

- **One kind of thing.** A computer is a computer, whatever its system and whichever device runs
  it. Differences are only those the platform forces.
- **A computer cannot get more than it has.** An agent inside a computer can do anything that
  gives it no more access than it already has. Anything wider asks you, in the Silo app.
- **Git words.** Checkpoint, fork, diff, restore, merge, undo. One word per idea
  ([vocabulary](../SiloUI-VOCABULARY.md)).
- **Security is built properly, and is not the current focus.** A change is made safely; security
  work for its own sake waits until the journeys are good.

## What Silo is not

- Not a tool that runs agents on your own machine with a sandbox around one process.
- Not a hosted service. Computers run on devices you own.
- Not a project folder on your Mac: there is no `.silo` folder and no setup kept in a repository.
- Not a CI product. Running GitHub jobs is one case of the third journey, not a goal of its own.

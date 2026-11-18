# Silo: the product

This page says what Silo is, who it is for and how it is being built. It is the source of truth
for product questions: agents read it before they build, and the owner changes it by deciding
something (see [the decisions](decisions.json)).

Silo is being redesigned. The design is recorded in [DESIGN.md](DESIGN.md), written on
18 November 2026, and this page is its short form. The owner goes through the design part by
part. A part gets its features here once he has settled it. Until then the plan holds no feature,
and nothing is built.

## In one sentence

Agents work in a computer of their own. Silo records everything they do there, shows you what
matters, and takes you back to any moment.

## Who it is for

A developer who runs several coding agents and wants them to work with full access, without
giving them their own machine. The first user is the owner, on a Mac. A developer inside a
company fits the same description. Team features come later, on top of the same product.

## The problem

- A useful agent needs root, the network, credentials and a real environment. On your own
  machine that puts your files, keys and accounts in its reach.
- You cannot see what agents are doing while they run. After a long run you cannot tell what they
  did or what it cost.
- When an agent breaks something you cannot go back, and part of the damage happens outside the
  machine.
- Agents leave things behind: full disks, processes that keep running, folders nobody can tie to
  a session.
- Rules that live inside the agent program are walked around by the agent.

## The shape

1. **A computer.** Your agents live and work in one long-lived computer, called main. It is a
   virtual machine on your device, running macOS or Linux. Nothing from your device is shared
   into it.
2. **The record.** Silo writes down what happens in the computer, seen from outside it: every
   process, file change, connection and model call. Opinions about those facts, such as which
   run did this or what a rebuild would bring back, are computed on top of them and never stored
   as facts.
3. **Going back.** Silo takes checkpoints on its own. You restore one, or continue from it in a
   fork.
4. **Forks for the exception.** A fork is a copy of a computer: for a risky try, for a tool to
   test before trusting it, for a task that needs a clean machine. It is dropped, or it becomes
   main.

The parts around it (secrets an agent can use but not read, network rules per computer, limits
on spending, approvals only you can answer) exist so that this is safe to leave running.

## Where it runs

On your own machine first: a Mac. The same computers then run on a shared server or in a cloud.

## Rules that hold everywhere

- **One kind of thing.** A computer is a computer, whatever its system and whichever device runs
  it. Differences are only those the platform forces.
- **A computer cannot get more than it has.** An agent inside a computer can do anything that
  gives it no more access than it already has. Anything wider asks you, in the Silo app.
- **Nothing flows down.** An opinion is never stored as a fact. A rule that blocks or holds never
  depends on an opinion. Something that watches never widens access.
- **Git's words where Git has one.** Checkpoint, fork, restore. One word per idea.
- **Security is built properly, and is not the current focus.** A change is made safely; security
  work for its own sake waits until the first version is good.

## What Silo is not

- Not a tool that runs agents on your own machine with a sandbox around one process.
- Not an agent program. It runs the vendors' programs, unmodified, and never chooses a model.
- Not an orchestrator, a chat window or a board for tracking work.
- Not a project folder on your Mac.
- Not a CI product.

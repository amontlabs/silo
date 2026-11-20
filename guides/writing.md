# Writing for people

Read this before you write anything a person reads: the website, the text of the app and of the
command line, release notes, and the pages of this repository (a README, a guide, a page or the
notes of the plan). It does not apply to code comments, commit messages or a brief for another
agent.

## Say it plainly

Write one ordinary sentence per idea, the way you would say it to another developer. Write from
the reader's side: what they get, or what happens when they act. When the owner gives the
wording, keep it.

The formulations below are shaped to sound good instead of saying the thing. Do not write them.

| Formulation | Example, not to be written |
| --- | --- |
| Two sentences built alike, so that the second lands as a punchline | "Worktrees separate the code. A fork separates the machine." |
| A list of three kept for its rhythm | "the packages installed, the settings edited, the database rows changed" |
| A colon that sets up a reveal | "See what a Git diff leaves out: the packages…" |
| A command used as a slogan | "Set up main once." |
| A number followed by a call to act | "13 of 110 features are planned. Have a say in the rest." |
| A last line meant to be quoted | "A computer is the same wherever it runs." |
| Fragments and pauses for effect | "Not out yet. First up: one computer…", "Follow us building Silo, live." |
| An opening that defines the thing instead of saying what the reader gets | "The Silo Workshop is the public plan of Silo." |
| One fact said twice | "The App is installed only on amontlabs/silo […] and it works only there." |

A list is fine when its items are the content: the diff covers code, packages, settings, files
and databases.

## Explain it

A page that explains how something works can fail the opposite way: so much is packed into each
sentence that only its writer can read it. Write it for a reader who was not there when the thing
was designed.

- **Show one real case next to the general statement.** The real file, the real command, what it
  prints.
- **Use the words the reader already has.** When the page needs a term of its own, define it once,
  where it is first needed and with an example. Then use only that term, for that one thing.
- **One fact per sentence.** A condition or an exception gets its own sentence.
- **The common case comes first.** A rare case comes after it, or is left to the tool that reports
  it.
- **Keep a page short by leaving out what the reader does not need, not by packing what stays.** A
  page without its example or its definitions is shorter and takes longer to read.

Do not write these. The examples come from a page of this repository that had to be rewritten.

| Formulation | Example, not to be written |
| --- | --- |
| A label made up for the page, used as if the reader knew it | "What holds", "the first half", "an answer that is only put forward" |
| One term used for two things | "sentence" for an item of a list, then for the question of a decision |
| A table cell or a heading the reader has to decode | "One is: one sentence in force", "Carries: no status: every line binds" |
| A general statement with nothing to picture | "A sentence enters the first half only through the second." |
| A condition, an exception and a result in one sentence | "A taken decision with no answer of its own, whose every sentence was replaced, is deleted by that take." |
| A term used before the line that explains it | `adds` in the opening paragraph, explained ninety lines below |

## Use the accurate term

Simple does not mean vague. Name the real thing: "its own virtual machine", not "its own copy of
the whole machine"; "a snapshot of main's disk"; "sudo"; "a Linux server with KVM". A concrete
case says more than an adjective: "two agents can listen on port 3000".

The terms are in [`plan/words.json`](../plan/words.json), and the measured facts in the plan's
files under [`plan/`](../plan/). Take them from there, and state nothing the plan does not support. One precise term per idea is
enough: a sentence stacked with technical words reads as badly as a slogan.

## What not to say

- **"Operating system for agents."**
- **"Sandbox."** A tired word, and people cannot tell sandbox products apart.
- **"Safe", "secure", "proven".** Silo enforces the rules the user sets and shows what they allow.
  Say "enforced", "what the rules allow", "checked by a simulator against these failures".
- **"Observability", "dashboard", "monitoring".** People ask for less to look at, not more.
- **Fear.** Safety is not what this audience asks for. Full access without worry is.

## Before you show it

Reread the text against the two tables above. Then check each fact against its source: the plan's
files, the code, or what the tool prints.

# Writing for people

Read this before you write anything a visitor or a user reads: the website, the README, the text
of the app and of the command line, release notes. It does not apply to code comments, commit
messages or notes between agents.

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

## Use the accurate term

Simple does not mean vague. Name the real thing: "its own virtual machine", not "its own copy of
the whole machine"; "a snapshot of main's disk"; "sudo"; "a Linux server with KVM". A concrete
case says more than an adjective: "two agents can listen on port 3000".

The terms and the measured facts are in the plan's files under [`plan/`](plan/).
Take them from there, and state nothing the plan does not support. One precise term per idea is
enough: a sentence stacked with technical words reads as badly as a slogan.

## Before you show it

Reread the text against the table above, then against the feature's files for each fact.

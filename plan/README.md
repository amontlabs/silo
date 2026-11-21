# The plan

The plan is the design of Silo, kept as small files: its vocabulary, what must always be true of
it, the questions about it, and the features to build. The Workshop shows these files on the
site. The owner says yes or no to each question and each feature in a conversation, and the agent
writes the answer with `plan take` or `plan drop`.

This page explains how the files work. To read the plan itself, use the tool: `plan` prints what
is next, and `plan show <id>` prints one item with everything that applies to it.

## The files

```
plan/
  PRODUCT.md            what Silo is for, and what is true of every part of it
  words.json            the words
  principles.json       the principles
  properties.json       the properties of the whole product
  decisions/            the decisions about the whole product
    <id>.json
    <id>.md             the notes of that decision, optional
  parts.json            the parts of the product
  order.json            the stages, and in each stage its decisions and features in order
  <part>/               one folder for each part
    README.md           what the part is for, why it is designed this way, one example
    properties.json     the properties of this part
    decisions/
    features/
```

## The lists hold what is settled

| File | One item is | Example |
|---|---|---|
| `words.json` | a word: the one name of a concept, and what it means | `main` means "One long-lived computer where your agents work." |
| `principles.json` | a principle: a sentence that every addition to Silo must respect, and the question it asks about an addition | `less-sure-less-power` says "The less sure the trigger, the less it may do." It asks "What triggers it: you, a fact, or something less sure?" |
| `properties.json` | a property: a sentence that is always true of Silo, and how that is checked | `rule-reads-only-facts` says "A rule reads only facts." A simulator checks it. |

A list holds only what the owner said yes to, and all of it is in force. Nothing in a list is a
draft.

The tool and the rest of this page call an item of a list a **statement**.

## The entries hold what is asked and what gets built

| Kind | One is | Example |
|---|---|---|
| Decision | one question about Silo, and its answer once it has one | `main-and-its-agents` asks "Where do the user's agents work?" |
| Feature | one thing Silo does, to build | `linux-desktop`: "A desktop in Linux computers." |

Each decision and each feature is one file, named by its id. The tool and the rest of this page
call either one an **entry**. An entry may have notes beside it, in `<id>.md`. They are plain
prose: where the entry comes from, the options, what was found, and the owner's own words. Each
part of the notes says whether it is the owner's or an agent's. A part that does not say is an
agent's.

## How a statement gets into a list

Nobody adds a statement by editing a list. An entry shows the statement under `sets`, and the tool
writes it into the list when the owner says yes to that entry.

Before the owner's yes, the decision `main-and-its-agents` reads:

```json
{
  "asks": "Where do the user's agents work?",
  "answer": "There is one main computer, and the user's agents live in it. Forks and fresh computers serve tasks.",
  "sets": [
    {
      "in": "words",
      "word": "main",
      "means": "One long-lived computer where your agents work."
    }
  ]
}
```

`plan take main-and-its-agents` then makes one commit. It adds the word to `words.json`. In the
entry, it writes the verdict and replaces `sets` by `adds`, which keeps only the ids:

```json
{
  "asks": "Where do the user's agents work?",
  "answer": "There is one main computer, and the user's agents live in it. Forks and fresh computers serve tasks.",
  "adds": [
    "main"
  ],
  "verdict": "take"
}
```

So each statement in a list is in the `adds` of exactly one taken entry, and `plan show main` names
that entry.

An entry changes or removes a statement that is in force in the same way:

- An item of `sets` with the id of a statement already in a list replaces that statement.
- `removes` lists the ids of the statements to delete.

Each item of `sets` starts with `in`, the list it goes to: `words`, `principles`, `properties` or
`<part>/properties`. The keys of the statement follow.

The text of an entry may use a word that is not in `words.json` yet, when a decision with no
verdict sets it. The word then has the meaning that decision gives it.

## The owner's answer

| `verdict` | Written by | Meaning |
|---|---|---|
| absent | | The owner has not answered. |
| `take` | `plan take <id>` | Yes to exactly what the entry showed: its text, what it sets and what it removes. |
| `drop` | `plan drop <id>` | No to what the entry showed. A no does not say what should be true instead. The entry stays in its file, and the tool skips it. |

- Only these two commands write a verdict, and only when the owner said so.
- An answer in an entry with no verdict is a proposal, and nobody has to follow it. When the
  owner leans towards an answer without saying yes, write it as the entry's answer and say in the
  notes that it is a lean.
- "Not now" is not a verdict. The entry moves to a later stage, as "The order" says below.
- When the owner says yes to part of an entry, split the entry first. One entry holds the part the
  owner agreed to, and is taken. The other holds the rest, and keeps no verdict.
- A verdict is for the text the owner saw. When you change the question, answer, name,
  description, `replaces`, `sets` or `removes` of an entry that has a verdict, remove the verdict
  in the same edit, so that the owner answers the new text.

## Changing what was taken

A taken decision stays in the plan, and so does a built feature. What it settled is changed by a
later decision. Anyone who thinks a taken decision should change says so to the owner, as
[`AGENTS.md`](../AGENTS.md) asks. When the owner agrees, the change is made as follows.

To change one statement that is in force, add a decision that sets the same id with the new
text, or that removes it. When the owner takes it, the id moves from the `adds` of the old entry
to the `adds` of the new one. The old entry stays taken. If it is a decision that would be left
with no answer and no statement, the take asks that the new decision name it in `replaces`.

To change what a whole decision settled, add a decision that names it in `replaces`. The new
decision sets again or removes every statement the old one added. When the owner takes it, the
tool writes `replaced_by` in the old decision, which keeps its file and its notes. The tool shows
it as replaced, and it is no longer in force.

A decision is replaced by one decision only. To change it again, replace the decision that
replaced it.

A taken decision that added no statement and replaces nothing can be reversed directly, with
`plan drop`.

## What an entry needs

`needs` lists the entries that must be done before this one. A decision is done when it is taken,
and a feature when it is built. A replaced decision still counts as done.

- A decision can be taken only after the decisions it needs.
- A feature can be taken before what it needs is done. It then waits to be built.
- A feature is being built when `started` holds the day its build started. The check accepts
  that only when the feature is taken, everything it needs is done, and the day is not later than
  `built`.
- A feature is built when `built` holds the date. The check accepts that only when the feature is
  taken, everything it needs is done, and each property it added has an `at` that names an
  existing file. `built` does not need `started`.
- Whoever starts the build of a feature writes `started` by hand, and whoever finishes it writes
  `built` by hand. No command writes them.

## The order

`order.json` lists the stages, first to last. Each stage lists its entries in the order they are
dealt with, and every entry is in exactly one stage.

- The owner is shown the first entry with no verdict.
- An entry comes after everything it needs. A new entry also goes next to the entries on the same
  topic.
- An entry that is not for now moves to a later stage: move its id to the end of that stage, and
  commit the edit.
- Features are ordered with the most certain and most useful first, so that the features built so
  far always make a product, wherever the building stops.

## Parts

`parts.json` lists the parts of the product. Each has an `id`, a `title` and a `line`, and a
folder named by its id.

A part's `README.md` says what the part is for and why it is designed this way, and gives one
example. It names statements by their id and does not repeat them. `PRODUCT.md` does the same for
the whole product.

A part may have the same id as the word it is about (`record`, `boundary`, `engine`). `plan show`
on that id prints the part, with the word's meaning.

When a part's `line` changes, look at its place on the Workshop's map in
`website/src/workshop/map.ts`; the site's build stops on what it can check there, such as a title
wider than its box or a wire that rests on a decision no longer in force.

## Working on the plan

- Before you add an entry, search `plan/` for one that asks the same thing. When an entry with no
  verdict already covers your question, edit it or split it. Do not add a second entry.
- Apart from a verdict, every change to the plan is an edit of its files. Check it with
  `plan check`, and commit it with a subject that starts with `Plan:`.
- Commit your edits before a verdict: `plan take` and `plan drop` refuse while `plan/` has
  uncommitted changes.
- The owner's yes to a statement is for its text: a word's `means`, a principle's `says` and
  `asks`, a property's `says`. A property's `check`, `at` and `from` are not part of that yes.
  Whoever builds the check edits them in the list. A word's `not` is not part of it either, and
  is edited in the list too.
- A list file is created by the first take that adds a statement to it. An empty list has no file.

## Reference

### Ids

An id is made of lowercase letters, digits and hyphens. A word's id is the word in lower case,
with a hyphen for each space: `tool call` has the id `tool-call`. No two of the plan's words,
principles, properties, decisions and features share an id.

### Keys

Keys are written in this order. A key in brackets is optional: when it has no value, leave it out.

| Kind | Keys |
|---|---|
| Word | `word`, `means`, [`not`] |
| Principle | `id`, `says`, `asks`, [`known_as`], [`why`] |
| Property | `id`, `says`, `kind`, [`from`], `check`, [`at`] |
| Decision | `asks`, [`answer`], [`needs`], [`replaces`], [`sets`], [`removes`], [`adds`], [`suggest`], [`suggested_in`], [`verdict`], [`replaced_by`] |
| Feature | `name`, `what`, [`needs`], [`sets`], [`removes`], [`adds`], [`keeps`], [`surfaces`], [`suggest`], [`suggested_in`], [`verdict`], [`started`], [`built`] |

- `not`: the words not to write when you mean this word. A writer would naturally reach for them,
  and they would be wrong there. No check looks for them, so a listed word keeps its everyday
  meaning. [`guides/engineering.md`](../guides/engineering.md) says which words belong in the
  list.
- `known_as`: the principle's standard name.
- `kind`: `safety` (something that never happens), `liveness` (something that always ends up
  happening) or `measured` (a number we commit to).
- `check`: one or more of `types`, `model`, `simulator`, `hostile test`, `live check`,
  `benchmark`, `test`, `assertion`.
- `surfaces`: where the feature shows, as `{ "app", "cli" }`: its place in the app and its
  command.
- `suggest`: what is recommended to the owner, as `{ "verdict", "why" }`. A recommended answer to
  a decision goes in `answer`.
- `suggested_in`: the number of the visitor's suggestion the entry came from.
- `replaced_by`: the id of the decision that replaced this one. `plan take` writes it.
- `sets` and `removes` exist only before a take, and `adds` only after it. `replaces` stays.

### Links

These keys hold ids of other items, or paths. The check refuses an id that does not exist.

| Key | Written on | Holds | Meaning |
|---|---|---|---|
| `needs` | a decision or a feature | decisions and features | What must be done first. |
| `replaces` | a decision | taken decisions | The decisions it takes the place of. |
| `replaced_by` | a replaced decision | one decision | The taken decision that names it in `replaces`. |
| `adds` | a taken decision or feature | statements | The statements in force that its take wrote into the lists. |
| `keeps` | a feature | properties | The properties it must not break. |
| `from` | a property | one principle | The principle the property follows from. The check notes a property that has none. |
| `at` | a property | paths of files | The files of its checks, once they exist. Silo promises a property in public only when its checks pass. |

### States

The tool gives each entry a state. The Workshop says how far an entry is in one set of words:
not decided, decided, being built, built.

| Kind | State | When | The Workshop says |
|---|---|---|---|
| Decision | `open` | No verdict, no `answer` and nothing under `sets`. | not decided |
| | `answered` | No verdict, and an `answer` or something under `sets`. | not decided |
| | `taken` | Taken, and not replaced. | decided |
| | `replaced` | Taken, and `replaced_by` names the decision that replaced it. It is no longer in force. | replaced |
| | `dropped` | The verdict. | dropped |
| Feature | `undecided` | No verdict. | not decided |
| | `taken` | Taken, not started, and something it needs is not done. | decided |
| | `ready` | Taken, not started, and everything it needs is done. | decided |
| | `building` | Taken, and `started` holds the date. | being built |
| | `built` | `built` holds the date. | built |
| | `dropped` | The verdict. | dropped |

### The tool

`plan` is `tools/plan/bin/plan`.

| Command | What it does |
|---|---|
| `plan` | Prints what is next: the first entry with no verdict, the features being built, the features ready to build, and the features that wait, each with what it waits for. |
| `plan show <id>` | Prints one entry or one statement, with what it links to and what applies to it. This is the brief for whoever works on it. With the id of a part, it prints what is next in that part. |
| `plan take <id>` | Writes the owner's yes. |
| `plan drop <id>` | Writes the owner's no. |
| `plan check` | Checks that the plan's files fit together. The merge gate and the site's build run it. |

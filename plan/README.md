# The plan

What Silo is, what always holds in it, what is still to decide and what gets built. The Workshop
shows it, and the owner rules on it there or in a conversation.

## Two halves

| | What holds | What is to do |
|---|---|---|
| Kinds | words, principles, properties | decisions, features |
| One is | one sentence in force | one thing the owner rules on |
| Lives in | a list, read whole | its own file, with its notes beside it in `<id>.md` |
| Carries | no status: every line binds | a verdict, and a place in the order |

A sentence enters the first half only through the second: every word, principle and property is
named in the `adds` of exactly one taken decision or feature.

## What holds

| Kind | One is | File |
|---|---|---|
| Word | the one name of a concept, and what it means | `words.json` |
| Principle | one sentence that every addition to Silo must respect, and the question it asks | `principles.json` |
| Property | one sentence that always holds in Silo, and how it is checked | `properties.json` at the root, `<part>/properties.json` in a part |

A word's id is the word itself in lower case, with a hyphen for each space. An id is unique in the whole plan:
no word, principle, property, decision or feature shares one.

What the owner took is the sentence: `means`, `says`, `asks`. How a property is checked (`check`,
`at`) and which principle it follows from (`from`) are kept by whoever builds the check, as plain
edits.

## What is to do

| Kind | One is | File |
|---|---|---|
| Decision | one choice about Silo: the question, and its answer once there is one | `decisions/<id>.json` at the root, `<part>/decisions/<id>.json` in a part |
| Feature | one thing Silo does, to build | `<part>/features/<id>.json` |

The file name is the id. The notes hold the research, the options and what was found.

Until it is taken, a decision or a feature shows in full the sentences it would bring:

- `sets`: sentences to add, or to put in place of the one with the same id. Each item starts with
  `in`, the list it goes to (`words`, `principles`, `properties` or `<part>/properties`), then
  the sentence's own keys.
- `removes`: ids of sentences to take out of the lists.

| `verdict` | Meaning |
|---|---|
| absent | Not decided. A decision with no `answer` is open |
| `take` | The owner said yes to exactly what the entry shows: its sentence, what it sets and what it removes |
| `drop` | The owner said no. The entry stays, and is skipped |

- Only `plan take` and `plan drop` write a verdict, on the owner's word. Taking writes what the
  entry sets into the lists and replaces `sets` by `adds`, the ids it brought, in one commit.
- A yes to part of an entry splits it first: the part taken, and the rest still to decide.
- An entry whose sentence changes loses its verdict. An entry with no verdict is edited freely.
- A taken decision stays, as a built feature does. When a later entry puts a new sentence in
  place of one it brought, that id moves to the later entry's `adds`. The owner can rule again:
  a taken decision that brought no sentence may be dropped.
- A decision is taken only after the decisions it needs. A feature may be taken before what it
  needs is built: it waits.
- A feature is built when `built` holds a date. That needs the verdict take, everything it needs
  done, and for each property it adds an `at` that names an existing file.

## The order

`order.json` holds the stages, first to last, and in each one the decisions and features in the
order they are dealt with. Every decision and every feature is in exactly one stage.

- The owner is shown the first entry with no verdict.
- An entry that is not for now is moved to a later stage. No verdict means "later".
- An entry comes after everything it needs.

## Links

An entry names another by its id. The check refuses an id that does not exist.

| Key | On | Names | Meaning |
|---|---|---|---|
| `from` | a property | a principle | The principle it follows from. A property without one shows as a gap to close |
| `needs` | a decision, a feature | decisions, features | What must be done first: a decision is done when taken, a feature when built |
| `adds` | a taken decision or feature | sentences | The sentences it brought into the lists |
| `keeps` | a feature | properties | What it must not break |
| `at` | a property | files | Its checks, once they exist. Silo promises in public only a property whose checks pass |

## Parts

`parts.json` lists the parts of the product: `id`, `title`, `line`. Each part is one folder under
its id. Its `README.md` says what the part is for, why it is so, and gives one example. It names
sentences by id and never repeats them. What concerns the whole product sits at the root, in the
same shape.

## Keys

In this order. A key in brackets is optional: it is absent, never empty.

| Kind | Keys |
|---|---|
| Word | `word`, `means`, [`not`] |
| Principle | `id`, `says`, `asks`, [`known_as`], [`why`] |
| Property | `id`, `says`, `kind`, [`from`], `check`, [`at`] |
| Decision | `asks`, [`answer`], [`needs`], [`sets`], [`removes`], [`adds`], [`suggest`], [`suggested_in`], [`verdict`] |
| Feature | `name`, `what`, [`needs`], [`sets`], [`removes`], [`adds`], [`keeps`], [`surfaces`], [`suggest`], [`suggested_in`], [`verdict`], [`built`] |

- `not`: the words or phrases this word replaces. The check refuses each of them anywhere in
  `plan/`. List one here only when every use of it in the plan would mean this concept.
- `known_as`: the principle's standard name.
- `kind`: `safety` (it never happens), `liveness` (it always ends up happening) or `measured` (a
  number we commit to).
- `check`: a list of one or more of `types`, `model`, `simulator`, `hostile test`, `live check`,
  `benchmark`, `test`, `assertion`. `at`: the files of those checks.
- `suggest`: `{ "verdict", "why" }`, what is recommended to the owner. A recommended answer to a
  decision goes in `answer`.
- `suggested_in`: the number of the visitor's suggestion the entry came from.
- `sets` and `removes` exist only before the verdict take, `adds` only with it.

## The tool

| Command | What |
|---|---|
| `plan` | What is next: the first entry with no verdict, the features ready to build, the features that wait and for what |
| `plan show <id>` | One entry or one sentence, with what it links to and what binds it: the brief for whoever works on it |
| `plan take <id>` | The owner's yes |
| `plan drop <id>` | The owner's no |
| `plan check` | The plan holds together. The merge gate and the site's build run it |

Everything else is a plain edit of a file, which the check accepts or refuses.

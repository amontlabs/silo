# Who made each change

The promise it sets says "control fact", the kind of line that `kinds-of-lines` gives to what happens to a
computer's rules, secrets, channels and state, and "each with who did it". It does not say "every
change": a file change is a fact, but which process made it is a separate report, and what no run
is shown to have written is shown as unattributed (`books-balance`, in `what-runs-wrote`).

## The choice

An agent's part, 22 November 2026. A background process writes 2 GB in main at a moment when Silo
could not see which program did it. A promise in force says that such a change is shown as
unattributed.

The options:

- Settings always, files when known (recommended). Every change to a computer's rules, secrets, ways
  in and out, and state says who made it: you, a rule, a precaution, an extension or Silo itself. A
  file change with no known author is shown as unattributed. It costs some file changes that carry
  no name.
- Every change has a name. Every file change also gets an author. It costs true names, because Silo
  has to guess when it did not see, and some names will be wrong.

Recommended: settings always, files when known. Silo never fills a hole with a guess, and
"unattributed" is the honest answer.

## The first build's spec

Agent, 24 November 2026, from [the build specs of the first
build](../../../research/2026-11-24-first-build-specs/README.md). The first build writes the Mac's
sleeps and wakes in each running computer's record (`mac-kept-awake`, #348). None of the authors
here put the Mac to sleep. The specs write the author `device` for what the Mac does by itself; the
other ways are the author `silo` with the cause `mac-sleep`, which says Silo did what it did not, or
a new kind of line, which `kinds-of-lines` keeps fixed. Every control fact made by you also names
your person, as #362's `person-in-a-control-fact` proposes.

## The author `device`, tried

An agent's part, 25 November 2026, from [the record's format](../../../research/2026-11-25-record-format/format.md). The format
writes `author` as `{"by":…,"person":…}`, with `by` one of you, rule, precaution, extension,
silo and `device`, and `person` required when `by` is you (#362's `person-in-a-control-fact`).
The prototype's verifier refuses a control fact with no author, an author outside the six, or "you"
with no person. Recommended: add `device` to this entry's answer and to the property
`every-control-fact-has-its-author`, as "you, a rule, a precaution, an extension, Silo, or the Mac
itself". Without it, the Mac going to sleep is written as Silo's doing.

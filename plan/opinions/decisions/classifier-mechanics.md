# A classifier's mechanics

## The choice

An agent's part, 22 November 2026. Silo's classifier that sorts main's requests into model, package
registry, Git and the rest reads only the record. After a crash, Silo throws away its opinions about
main and works them out again. Every request of last week gets the same kind as before.

The options:

- Promise it, for classifiers that follow fixed steps and read only the record (recommended). It
  costs those classifiers some freedom: no time of day, no chance, nothing from outside the record.
  A classifier that reads inside a computer or asks a model is left out of the promise, so it covers
  part of what you see.
- No promise. It costs steadiness: the same record can show a different list of what is only here
  from one look to the next, with nothing new in between.

Recommended: promise it. An opinion that changes when nothing happened is noise, and these
classifiers can avoid it at no cost to you.

## Notes

The row "A classifier's mechanics" of the table "Where each kind of checking goes", whole:

| What | Is there an exact statement of "right"? | How it is checked |
|---|---|---|
| A classifier's mechanics | Yes: for a classifier that is a pattern or a function and reads the record alone, the same record gives the same opinions; no fact or report changes; anything unknown is "only here" | Random inputs with these properties stated |

That no fact or report changes is `no-call-changes-the-record` in the part `record`. That
anything unknown is "only here" concerns the classifier of how a path would come back
(`how-a-path-comes-back`).

The sentence needs a qualifier: it holds only for a classifier that is a pattern or a function
and reads the record alone. An opinion from a model, or from a classifier that reads inside a
computer, can come out differently (`classifier-reads-inside`).

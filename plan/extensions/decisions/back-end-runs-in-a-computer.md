# Where an extension runs

## The line

One question settles it: is any part of the extension executed?

- Executed means the author wrote what the machine does. That part is the back end. A program
  can do whatever its machine lets it do, so it runs in a computer of its own, where rules at the
  boundary set what it can reach.
- Not executed means Silo only reads it and picks among choices Silo itself offers: rules,
  panels. It can do nothing by itself.
- In Silo's own process an extension would have what Silo has: every computer, every secret, the
  record.

## The owner's words (21 November 2026)

- The owner asked for something specific and generic that says at which point an extension can
  do something on the machine, so that it needs a computer. "If the fact that there is a backend
  means that it could do something on the machine, then yes, indeed it needs to be in the
  computer."
- "Only data" makes sense to the owner as static content.
- An extension that calls a model is one extension among others. It brings its own code for it,
  and nothing in the design is about models.

## What it replaces

`where-a-classifier-runs` said: "A classifier that is only data needs no computer. Code or tools
means a computer." The line is now whether something is executed.

An earlier example put `model` and `prompt` under the field `runs`, as if Silo itself called a
model for an extension. The owner found it corresponds to nothing, and it is not carried.

## What it costs

One computer for each extension that has a back end.

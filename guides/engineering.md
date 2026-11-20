# Engineering

How anything in Silo is designed, named and specified: a concept, a feature, an interface, a
type, or the entry that states one. Read it before you do any of that, or review it. A lookup or
a mechanical change does not need it.

The examples come from Silo's own design. Its words are in `plan/words.json`.

## The bar

- **A design is solid only when everything in it is clear, complete, thorough and simple to
  picture.** Until then keep working on it: think it through and take the time.
- **The owner gives a direction in rough words.** Understand it and define it properly. The right
  definition feels obvious once stated.

## Before you design

Two steps come first, in this order.

1. **Explain the owner's direction back in one sentence,** before you design anything from it.
2. **Look for how it is already done.** When a known practice does the same job, take it as the
   model and say exactly what differs.
   - Right: the words for computers are Git's. What differs: Git versions files and Silo versions
     something alive, so a fork must not act as its parent.

Derive something new only when neither settles it.

## Concepts

1. **If a statement about a concept needs a qualifier, there are two concepts.** A qualifier is
   the clause that narrows it: "an X that…", "usually", "except". Cut the concept so that each
   part is stated in one plain sentence, true of every instance.
   - Wrong: "A rule that denies or holds never depends on an opinion." Then some rules do, and
     "rule" names two things.
   - Right: "A rule is triggered by a fact and is exact. A precaution is triggered by a report or
     an opinion."
2. **Every limit comes from a principle.** What something may do is never just a list.
   - Wrong: a list of the actions a precaution may take, grown one request at a time.
   - Right: "A precaution can be wrong, so it grants nothing and destroys nothing."
3. **Cover every case.** Every case has a name and a defined outcome. When one side is described,
   so is the other.
   - Wrong: rules named only by what they refuse, "hold" and "deny", with no word for what passes.
   - Right: "allow, hold, deny".
4. **Two concepts both stay only if one sentence tells them apart.** Otherwise merge them, or cut
   them differently.
   - Right: "A rule is triggered by a fact, a precaution by a report or an opinion."
5. **Separate questions that are independent.** When something stays unclear, look for two
   questions answered as one.
   - Wrong: one table that answers both "what does it act on" and "what triggered it".
   - Right: one table for each question.
6. **File every new thing under the concepts that exist, before designing it.** The new thing
   declares its kind. If it fits none, the concepts are wrong.
   - Right: each action of a new feature is filed under one of the three kinds of action before
     the feature is designed.
7. **Design from real use.** How agents and sandboxes are really used and what people really
   want, not what is clean on paper.
   - Wrong: "Silo acts only on facts." Clean on paper, and people want an opinion to be able to
     pause a computer.
   - Right: an opinion can act, through a precaution.
8. **Decide, with an opinion.** Lay out the real options, choose one, and say what it costs and
   which agreed line it changes.
9. **Bad paths first.** Errors, timeouts, crashes and missing answers are designed from the
   first line, each with explicit handling. A bad state should be impossible to write down, and
   the language should enforce it where it can.
   - Wrong: a table of defaults for failures, added once the good path is designed.
   - Right: a precaution's type has no granting or destructive action in it.
10. **Aim for the best design, then find out how.** Whether it can be built, or is allowed, is a
    separate question: note it and research it. Mark an untested mechanism as to find out, and
    never write a promise with no known way to keep it.
    - Wrong: "A computer can always learn what happened to its requests", written as a promise
      when nobody knew how.
    - Right: the target is stated, and its mechanism is marked "to find out".
11. **Sort each thing: ground work, detail or feature.** Ground work is what the rest stands on,
    so it is settled before anything is built on it. A default, or how one piece is built, is
    settled when that piece is built. A feature waits for its stage. Say which, and flag what
    must be right from the start even though its details come later.
    - Right: what a precaution does when a classifier is unsure is a default. That an error comes
      back when a wait ends must be right from the start.
12. **A smell is a finding.** When the owner or you sense one, stop and find the word doing
    several jobs, or the concept cut in the wrong place.
13. **If a design needs a likeness to hold together, it is not clean.**
    - Wrong: "An opinion can pause a computer but cannot hold a request", held together only by
      "a pause is a kind of hold".
14. **Give each reason at its true size.** A reason that is too broad gets challenged and falls.
    - Wrong: "A classifier runs in a computer of its own."
    - Right: "Someone else's code never runs outside a boundary."

## Words

1. **One word for one concept, everywhere.** No synonyms, and no word with two meanings.
   - Wrong: "a hold stops it", when "stop" is the word for ending the programs in a computer.
2. **Each concept gets the accurate, standard technical term, and you say why it fits.** A good
   term explains itself.
   - Right: "granting", from access control, where to grant is to give the ability to do
     something.
3. **A word is never kept because the owner used it, or because you coined it.** The owner's
   wording is rough on purpose, and a rough word adopted as a term compounds.
4. **A word that can be read as another is replaced.**
   - Wrong: "block", which was read as "stop". Right: "deny".
5. **A word says what a thing is,** not how it looks, how it arrived or where it happened.
   - Wrong: "label", which says how it looks. Right: "opinion", which says it can be wrong.
6. **Between two accurate words, take the plainer.**
7. **No word is used before it is defined, and nothing is pointed at vaguely.**
   - Wrong: "Silo writes a gap." Which part of Silo, where, and how does it know?
   - Right: "The recorder, which keeps the record outside the computer, writes a line of the kind
     "gap"."
8. **When a word or a sentence changes, it changes everywhere in the same change.** Search for the
   old one.
9. **A word listed under `not` is not written for that concept, and is not proposed for it
   again.** In `plan/words.json`, a word's `not` reads "do not write these when you mean this".
   Read it before you name something, and look for those words when you review. No check looks
   for them, and a listed word keeps its everyday meaning.
   - Wrong: "a judge gives an opinion". Right: "a classifier gives an opinion".
   - Fine: "faster to judge", which is not about a classifier.
10. **A word goes under `not` by judgement, not each time a wording changes.** List it when a
    writer would naturally reach for it to name the concept, and it would be wrong there:
    inaccurate, or read as something else.
    - Listed: "block" under "deny", because it was read as "stop".
    - Not listed: "corroborated" under "matched", which was accurate and only less plain. "The
      thing" under "request", which nobody would take for a name.

## Stating it

- **An example is shown as an example.** Say the general thing, then the case.
  - Wrong: a text on classifiers that shows only one that judges tool calls, so tool calls read
    as the whole feature.
  - Right: "A classifier is whatever gives an opinion. One example: a classifier that judges each
    tool call."
- **Things set side by side are of one kind.** Rows of different kinds get read as alike.
  - Wrong: one table with "deny", which happens at the boundary, beside "stop", which happens to
    a whole computer.

## Checking it

- **A good property is a better review than an agent.** Where a property, a test, a type or a
  proof can say whether the work is right, write that. A review is for what none of them can
  state.
- **Many review rounds make a design drift.** Each reviewer adds a reading, and poor decisions
  compound. Prefer targeted changes that you check yourself. A finding that would add or change
  something the owner did not say is a question for the owner, not a fix.

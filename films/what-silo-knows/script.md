# Chapter script: what Silo knows

This is the first of four films that replace `record-four-layers`. The others are
`actions-and-kinds`, `rules-and-precautions` and `extensions`. Watch them in that order.

## Who watches

The owner of Silo. He builds the product with agents and decides everything. He has no computer
science degree and learns from strong engineers. He wants to understand how the product is put
together, one piece at a time. So: plain words, and every term is shown as a concrete thing the
first time the voice says it.

Length: short and clear, with no hard limit. A scene takes the time its idea needs.

## What he should understand at the end

That Silo knows things in three ways (it saw them, it was told them, or something thinks them),
what each is called, why the weaker never passes as the stronger, and that Silo never fills a
hole.

## Title and answer line

- Title: What Silo knows
- Answer: Silo saw it, Silo was told it, or something thinks it.

## Marks used

- Agreed: put forward in the discussions of 18 and 19 November 2026 and accepted by the owner. Not yet recorded as a decision.
- Proposed: Claude's proposal. The owner has not answered it.
- Owner: said by the owner, as an idea or a direction.

## Words introduced (shown under the statements)

- fact: one thing Silo saw itself, from outside the computer. Written once.
- report: one thing a program inside the computer told Silo. Written once, unverified.
- process: one running program.
- probe: Silo's own small program inside a computer. What it tells Silo is a report.
- agent program: Claude Code, Codex and the like.
- main: the long-lived computer where your agents work.
- record: everything Silo has written down about a computer: its facts and its reports, in the order they came, never changed afterwards. Engineers call this an append-only log. Facts and reports do not pass through the engine's queue: the parts that see them write them directly. The engine's own events are kept as facts too.
- opinion: what a classifier, or you, think about the record. It can be wrong. A classifier's opinion is computed again when the classifier improves. One you gave is kept as you gave it.
- classifier: whatever gives an opinion.

## How things look, in this film and the three after it

- Every row of the record shows its mark from the moment it lands, small and quiet, with where it was seen: `fact · network`, `fact · disk`, `report · inside`. A report is drawn lighter than a fact.
- An opinion is never drawn inside the record. It sits beside a row, in another colour and shape, with whose it is under it.
- The computer's edge is thick and clearly drawn, the same as in the three films after this one.
- The marks on the rows can be read in a held frame, and no text that has to be read is smaller than 14 pixels.
- A red cut is used for one thing only: a deny at the boundary. Something that cannot happen at all (an opinion becoming a fact) is drawn as a grey, struck-through chip that never leaves its card.

## Scenes

Seven scenes. Each scene's `source` is the statement named, word for word. Statements 4, 8, 10 and
11 are spoken inside scenes 3, 6, 2 and 4 and are listed for him to mark. Captions are ten words
or fewer. The narration is what the voice says; it is also the subtitle.

### Scene 1. Caption: "A fact: Silo saw it."  Source: statement 1

Narration:
1. "Start with what Silo saw itself."
2. "An agent opens a connection. It passes through a part of Silo that sits outside the computer."
3. "The agent also writes a file. The file stays inside, and Silo sees the change from outside, on the computer's disk."
4. "Silo writes one line for each: a fact. It is never changed."

Picture: on the left, a computer called "main" (a region) with an agent inside. Its edge is drawn
clearly, and two small parts of Silo sit on the edge, outside: "network" and "disk". On the right,
outside the computer, a list titled "The record". A chip leaves through "network" and lands as a
row: `connection  crates.io`, marked `fact · network`. For the file nothing leaves: inside the
computer a file `target/app` appears, the "disk" part looks in at it through a dashed one-way
link, and the row `file  target/app  +48 MB` lands, marked `fact · disk`. As the voice says "never
changed", a small lock appears on each row. Hero moment: the two rows landing, one from a thing
that passed and one from a thing that was looked at.

### Scene 2. Caption: "A report: Silo was told it."  Source: statement 2

Narration:
1. "Some things cannot be seen from outside, such as which process wrote that file. A process is one running program."
2. "A small part of Silo, the probe, runs inside the computer and tells Silo. That line is a report."
3. "The agent has full access in there, so a report is only what the inside said."
4. "An agent program, such as Claude Code, can also give its own account of itself. That is a report too."

Picture: inside "main", a small program appears, labelled "probe". A chip leaves it and lands in
the list as a third row: `written by  cargo build  →  target/app`, marked `report · inside`, drawn
lighter than the facts. Each link ends at the row it produced. As the voice says "full access", a dashed link goes from the agent to the
probe, to show the agent can reach it. Nothing links the agent to the two parts on the edge. On
the last sentence a chip leaves the agent itself and lands as a fourth row:
`own account  3 model calls`, marked `report · inside`. Hero moment: the third row landing with
the mark `report`.

### Scene 3. Caption: "The record: facts and reports, in order."  Source: statement 3

Narration:
1. "Facts and reports, in the order they came, are the record. Nothing in it is changed afterwards."
2. "One line is never both: that the file changed is a fact, and which process changed it is a report that points to it."

Picture: the list alone, larger, with its four rows and their marks. As the voice says "never
both", a link goes from the report row `written by  cargo build  →  target/app` to the fact row
`file  target/app  +48 MB`, with the words "points to it". The two marks pulse: `fact` on the file
row, `report` on the other. Hero moment: the link between the two rows.

### Scene 4. Caption: "An opinion: something thinks it."  Source: statement 5

Narration:
1. "An opinion is what something thinks about the record: this file is code, this agent is stuck."
2. "It is computed from the record, and kept apart from it."
3. "It can be wrong. So it says whose it is, and when what gave it improves, its opinions are computed again. The rows of the record do not change."
4. "You can give one too. Silo asks: is this a build? Your answer is your opinion. Silo keeps it as you gave it."

Picture: above the list, a card "files, version 1" (a function). On "computed from the
record", a dashed one-way link goes from the record to the card, and stays. An opinion appears
beside the file row, outside the list: `code`, with the small words "files v1" under it. Then the card becomes
"files, version 2" and a new opinion takes its place: `build output`, with "files v2" under it.
The old opinion `code` stays at full strength, in a muted colour and struck through, so that the
held frame clearly shows a wrong opinion replaced. The rows of the record stay exactly as they were. On the last sentence a
question card appears beside you, drawn as a person and not as a card among classifiers: "is this
a build?". No link comes from the record: the record does not ask. You answer yes. A second opinion `build output` appears beside the file row, with "you"
under it. The picture shows no new row in the record list: what matters here is your opinion,
which sits with the other opinions, outside the record. Hero moment: the opinion changing while the record holds still.

### Scene 5. Caption: "A classifier gives opinions."  Source: statement 6

Narration:
1. "What gives an opinion is called a classifier: a pattern, a function, a small model, or an agent."
2. "A classifier gives opinions. What an opinion can lead to comes in the film on rules and precautions."

Picture: four small cards in a row, each a classifier, and one opinion beside each: a pattern
`*.pem` with `credential-like`; a function `files v2` with `build output`; "small model" with
`risky`; "agent", with the small line "another agent", with `needs you`. Hero moment: the four
cards, each with its opinion.

### Scene 6. Caption: "As sure as its least sure ingredient."  Source: statement 7

Narration:
1. "Something computed is as sure as its least sure ingredient."
2. "A count of facts is as good as a fact. Mix in a report, and the result is unverified: no surer than the report. Add a judgement, and it is an opinion."
3. "And a report or an opinion is never stored as a fact."

Picture: three short lines. They stand faint from the start of the scene, each lights as the voice
says it, and all three stay on screen:
`fact + fact  →  as good as a fact`, with the example "spend of main: $4.20";
`fact + report  →  unverified`, with the example "written by cargo build: 48 MB";
`anything + a judgement  →  an opinion`, with the example "build output".
Then, and only then, the record list appears beside them, small. The opinion `build output` sits next to it as a grey,
struck-through chip with the words "never a row". On the row `written by  cargo build`, a
struck-through `fact` stands beside the mark `report`, large enough to read, with the words "never
a fact" under it.
Hero moment: the three lines, with the two things that cannot happen.

### Scene 7. Caption: "Silo never fills a hole."  Source: the decision `holes`

Narration:
1. "Silo never fills a hole."

Picture: the record list of scene 6, alone and larger, with its three rows. A fourth row lands one
place lower: `own account  3 model calls`, marked `report · inside`. The place between them holds
nothing. As the voice says "a hole", a dashed outline appears around that place, with the words
"a hole" beside it and "never filled" under them. Nothing is drawn inside it. Hero moment: the
empty place.

## Kind of chapter: decides

The statements are choices about the product that the owner has opinions on. Each keeps its three
buttons: agree, change, leave open, with a note.

## Was it clear? (one line per scene: "clear" or "lost me here", with a field for why)

## Test the picture (he answers in a field, then reveals the answer)

Question: An agent with full access stops the probe inside the computer for one minute,
deletes a folder, and starts the probe again. What does the record hold about it?

Answer: The deletion is there, as a fact: Silo sees the disk from outside, and the agent cannot
reach that part. How finely in time Silo sees the disk is still to find out. What is missing is which process did it, because the reports stopped for a
minute. That is a hole. Silo does not fill it with an opinion.

## A choice

None for this chapter.

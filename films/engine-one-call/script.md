# Chapter script: one call through the engine

## Who watches

The owner of Silo. He builds the product with agents and decides everything. He has no computer
science degree and learns from strong engineers. He wants to understand, at a low level, how the
engine does what the product promises, one piece at a time. So: plain words, and every technical
term is shown as the real thing (a real table row, real Zig) the first time the voice says it.

## What he should understand at the end

How one click in the app becomes a running virtual machine: what the state is, what a call is,
what the core does, what an event and an effect are, and why "wanted" and "seen" are kept apart.

## Title and answer line

- Title: One call through the engine
- Answer: A click becomes one input. The core turns it into new state, an event and an effect.

## Statements (shown under the film, each with the mark "Notes")

1. The engine's state is a few small tables of plain rows. A computer is one row: its number, its name, what is wanted and what was last seen.
2. Nothing changes the state except an input handled by the core, one at a time. A call from the app is an input. So is a driver's answer, and what a driver sees when a machine stops by itself.
3. The core is a function. It takes the state and an input, and returns the new state, events and effects. It touches no disk and no network.
4. An event is a numbered line that says what happened. The app follows the events to know the state.
5. An effect is a request to a driver. It has no return value. The answer, or a timeout, comes back later as an input.
6. Each row keeps what is wanted and what was last seen. While they differ, the core issues effects to make them match, or says what it is waiting for. What it does when a computer stops by itself is still open.
7. The same inputs in the same order always give the same state and the same events. Time and randomness are inputs too.

## Words introduced (shown under the statements)

- state: what the engine knows right now, kept in tables.
- input: anything that may change the state. Inputs are handled one at a time.
- core: the function that decides. It does nothing itself.
- event: a numbered line saying what happened in the engine. The app follows them. What agents do inside a computer is not in the events: that is the record, written apart.
- effect: a request for a driver to do something in the real world.
- driver: the part that does things, such as starting a virtual machine.

## Scenes

Six scenes. Each scene's `source` is the statement named below, word for word. Captions are ten
words or fewer. The narration is what the voice says; it is also the subtitle.

### Scene 1. Caption: "State is a small table."  Source: statement 1

Narration:
1. "The engine keeps what it knows in small tables."
2. "This is the table of computers. Each row is one computer: its number, its name, what you want it to be, and what the engine last saw."

Picture: a table of computers (a window) with two rows:
`7  main     wanted stopped  seen stopped` and `8  try-bun  wanted stopped  seen stopped`.
Beside it, a file `computer.zig` showing the row as real Zig:

```zig
const Computer = struct {
    id: u32,
    name: Name,
    wanted: State,
    seen: State,
};
```

A link from the file to row 7, labelled "one row is one struct". Hero moment: row 7 lights up as
the voice says "each row is one computer".

### Scene 2. Caption: "A click becomes a call, through one queue."  Source: statement 2

Narration:
1. "You click Start in the app. The app starts nothing itself."
2. "It sends a call, start computer seven, into one queue. Everything that can change the state enters through that queue, one at a time."

Picture: on the left, you and the app. A chip `start_computer(7)` leaves the app and travels into
a queue. The queue feeds the core. Hero moment: the chip entering the queue. Two faint chips
already waiting in the queue ("driver answer", "clock") show that everything uses the same queue.

### Scene 3. Caption: "The core decides and touches nothing."  Source: statement 3

Narration:
1. "The core is a function. It reads the state and the call, and checks that computer seven exists and is stopped."
2. "Then it returns three things: the new state, an event, and an effect."

Picture: the core in the middle. Three things come out of it, one after the other, each as the
voice names it:
- the table: row 7 now reads `wanted running  seen stopped`, and turns amber because the two differ
- an events list gets a line: `#412  computer_start_requested(7)`
- a chip `boot_vm(7)` parks beside a card "VM driver"
Hero moment: the three outputs.

### Scene 4. Caption: "A driver does the work and answers later."  Source: statement 5

Narration:
1. "The effect is only a request. A driver does the real work: it starts the virtual machine. That takes time, and it can fail."
2. "When it is done, its answer comes back through the same queue, as an input."

Picture: `boot_vm(7)` travels to the VM driver. A virtual machine appears next to the driver. A
chip `vm_booted(7)` travels back into the queue. The return link is labelled "the same queue". Hero moment: the answer coming back into the same
queue the click used.

### Scene 5. Caption: "What you want and what the engine saw now match."  Source: statement 6

Narration:
1. "The core handles that input the same way. It writes seen running, and a second event."
2. "What you want and what the engine saw now agree, so there is nothing left to do."

Picture: row 7 reads `wanted running  seen running` and turns green. The events list gets
`#413  computer_started(7)`. Hero moment: the row turning green.

### Scene 6. Caption: "The same inputs always give the same result."  Source: statement 7

Narration:
1. "The core touches nothing. So the same inputs, in the same order, always give the same result: the engine can replay a history, and pick up after a crash."
2. "The numbered events are how the app follows what the engine did. What agents do inside a computer is not in them: that is the record, and it is written apart."
3. "And the core can be tested with millions of random sequences of inputs before it ever starts a real machine."

Picture: first the queue, with the two inputs that went through it (`start_computer(7)`,
`vm_booted(7)`) lit as the voice says "the same inputs". It is the inputs that are replayed, not
the events: do not light the events on "replay". Then the events list, lines `#412` and `#413`
lit, and a link from the events list to the app as the voice says "follows". As the voice says
"the record", a small list titled "The record" fades in apart from the events, with one or two
rows and their locks, drawn as in the next film, and with no link to the core or to the events.
It fades back before the simulator arrives. Then a simulator (an
actor) streams random inputs into the same queue; the VM driver is replaced by a dashed "fake
driver" that fails on purpose. A check mark on the core: "properties checked after every step".
Do not invent a number on screen beyond the word "millions".

## Kind of chapter: teaches

The statements are established engineering practice. The owner is not asked to agree with them.
They are listed under the film for reference, each with its mark, with no buttons.

## Was it clear? (one line per scene: "clear" or "lost me here", with a field for why)

## Test the picture (he answers in a field, then reveals the answer)

Question: An hour later the virtual machine crashes by itself. Nobody clicked anything. The row
still reads wanted running, seen running. What happens next?

Answer: The driver sees that the machine is gone, and that is an input. The core
writes seen stopped. What is wanted and what is seen differ again, and that is how the engine
knows. Nothing special was written to notice a crash: it is the same path as your click. What the
engine does next is still open, and is decided with the whole design: start the machine again, or
leave it stopped and alert you.

## What it promises you (each: keep or change, with a note)

1. Once the engine has accepted a click, it is never lost and never carried out twice.
2. The app never shows a state the engine never had.
3. After a crash of the engine, every operation is finished or rolled back.
4. The same history always gives the same result, so any decision the engine made can be replayed exactly.

## Seen this go wrong? (one field)

Have you seen a tool show something as running, done or saved when it was not? Tell it in one or
two lines. Each story becomes a case the simulator's fake drivers have to reproduce.

## A choice

None for this chapter. It used to ask what Silo does when main stops by itself. The owner moved
that question to the design record's open questions (14.1, question 15) on 18 November 2026: it is
decided with the whole design, not under one film.

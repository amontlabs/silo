# How an extension's opinion is kept

## The owner's words

- On 21 November 2026, in the notes of `what-makes-a-classifier`: "I still don't know what the
  output is, like how it works." And later that day: an answer belongs to a tool. An extension that
  has the tool "deny or hold the network requests of main" must give that answer, and other tools
  may bring other answers.
- On 21 November 2026, asked whether an agent should propose an answer here: "You can definitely
  propose the best design for the classifier question on its note."

Everything below is an agent's proposal. The owner has not ruled on it.

## Why the question exists

`what-makes-a-classifier`, proposed, says a classifier is an extension that gives a value from a
fixed list. `extension-tools`, taken, says an extension uses only tools, and that a tool reads,
shows or protects. Only one tool takes an answer today: the one that protects a computer's
requests, where Silo asks and the extension answers "let it go on", "deny" or "hold". Nothing says
how any other value becomes an opinion that Silo keeps, such as the kind of a call or how a path
would come back (`how-a-path-comes-back`).

## The proposal: every opinion is an answer to a question Silo defines

The tool that protects requests already works this way. The proposal makes it the general case.

- A tool that takes a judgement is a question. Silo defines what it is about (a request, a path, a
  call, a run) and the list of answers.
- The extension answers by using the tool, about lines of the record it read.
- That it answered, and about which lines, is a fact: Silo saw it at a tool Silo defined
  (`extension-tool-use-is-a-fact`).
- What the extension answered is an opinion, never written into a fact. Silo keeps the latest one beside the record, with the
  extension, its version and the lines it is about.

The same split already holds for the user: that you answered a question is in the record, and
your answer is your opinion (`how-your-answer-is-kept`).

One case. An extension named Leftovers has the tool "say how a path would come back", granted on
main. A new file line wakes it. It reads the line and answers "by a rebuild" about
`/work/app/node_modules`.

```text
the record     14:02  Leftovers 1.2 answered about /work/app/node_modules                     (a fact)
beside it      /work/app/node_modules: by a rebuild, says Leftovers 1.2                    (an opinion)
the app        Disk shows it on that path, as Leftovers' opinion
```

Nothing happened to the path. A precaution the user set may read the value
(`opinion-acts-through-a-precaution`).

The known practice it follows is a check on GitHub: an app granted the right to write checks posts
a conclusion from GitHub's list on one exact commit, GitHub shows it with the app's name, and only
a rule the owner of the repository set acts on it. What differs: here the answer may be wrong, so
only a precaution may act on it, and no answer ever allows anything.

## What it answers on the way

- Kind of the action: protective. An answer grants nothing and destroys nothing.
- Which of the three kinds the new tool is: it shows. Its answer goes into the app, on the lines
  it is about, and a wrong one costs attention. The tool that protects is the case where granting
  the tool also sets a precaution, so a wrong answer costs time.
- `who-fixes-the-values`: Silo does, tool by tool, since the list belongs to the question.
- When an extension changes version, the answers of the older version stop counting and it is
  asked again. Nothing is converted (`opinions-computed-again`). The facts that it answered stay.

## Each bad path

| What happens | Outcome |
|---|---|
| The answer is not in the question's list | Silo refuses it. That it was refused is a fact. |
| It is about lines the extension was not granted to read | Refused, the same way. |
| The extension does not answer | No opinion. For a request that waits, the wait's limit ends it (`classifier-wait-limit`). |
| Two extensions answer the same question about the same lines | Both are kept, each with whose it is. Nothing picks one: a precaution names the extension it rests on. |
| The extension is removed | Its opinions go. The facts that it answered stay. |

## Options weighed

| Option | Why not |
|---|---|
| The extension writes any value or text beside the record | Silo cannot show, sort or act on a value it does not know, and a wrong shape could be written down. |
| Only the tool that protects gives an opinion; other extensions alert or draw a panel | What an extension finds could not be shown in Silo's own places, checked against the user's answers (`scoring-a-classifier`) or read by a precaution. Silo's own classifiers could not be extensions (`shipped-extensions-use-the-manifest`). |
| A fourth kind of tool beside reads, shows and protects | The kinds say what a tool costs the user. An answer costs attention, as what is shown does, or time when a precaution acts on it, as what protects does. |

A pattern that an extension adds and Silo applies by itself, as a rule is data, would cover what a
pattern can say without running any code. It could come beside this. It does not cover a model or
an agent, so it does not answer the question alone.

## What it costs, and what it changes

- `extension-tools`, taken, says a tool shows "an alert, a question or a panel". This adds a
  fourth thing shown, the answer. If that list is closed, taking this needs that sentence
  reworded by a decision that replaces it.
- Every answer is a line in the record. An extension that answers about each file change adds as
  many lines as there are changes, unless one use of the tool may answer about many lines.
- Asking an extension again about a record that already exists costs what its computer spends
  (`extension-spend`).

## To find out, or to settle when it is built

- How an extension is asked about the past, when it is installed or changes version.
- Whether one use of the tool answers about many lines.
- An answer that is another line of the record and not a word from a list: which run a process
  belongs to (`the-chain`). The question would say that its answers are the runs of that computer.
- An amount, such as heat. An agent's reading: Silo counts it from other opinions, and no
  extension gives it.

# What an extension is

## The owner's words

- First, on 19 November 2026, of what was then called a plugin: it has a structure that must
  match what Silo supports. It says when it is triggered, what its input is, where it runs, what
  its output is, and the rules, and perhaps the precautions, that come with it. Those are put
  forward at install and accepted explicitly. What happens inside can be anything.
- On 21 November 2026: "extension" is the word, and "manifest" is the word for what it says of
  itself, not "declaration".
- An extension that executes anything ships a computer, declared by its recipe. Settling it as "a
  program" or "a back end" is an unnecessary abstraction (`extension-ships-a-computer`).
- What an extension shows in the app is not a part it ships. It uses components the app already
  has, such as questions and alerts, and a tool that creates a panel (`extension-tools`).
- The owner asked for one design of the whole before single points are settled, and found the
  picture below makes sense.
- A manifest uses the standard field names, not plain labels such as "given" or "gives back". The
  owner called `on`, `inputs`, `outputs`, `runs`, `permissions` perfect. What goes under each is
  settled when it is built.

## The picture

An extension never reaches another computer. Its own computer asks the engine, through tools, and
the engine does the thing.

| An extension may add | What it is | Decided in |
|---|---|---|
| rules | ordinary rules | the part `rules` |
| a computer | made from the recipe its manifest names | `extension-ships-a-computer` |

What wakes its computer: `what-wakes-an-extension`. What it may use: `extension-tools`. What it
can never do: `what-an-extension-never-does`. What it may stop by itself: `extension-may-protect`.
Its secrets: `extension-secrets`. Importing and installing: `import-then-install`.

## Three examples

The owner wants these three in the film.

| Extension | Computer | Wakes on | May use | Adds |
|---|---|---|---|---|
| registry rules | none | | | rules: allow `crates.io` on a computer |
| disk watch | yes | every minute | read `/work` on main; create a panel; alert the user | |
| request check | yes | a network request of main waits | deny or hold that request | |

Examples of what an extension may use are generic and exact, such as a path. The owner asked for
this.

## A manifest is not a recipe

The owner wondered whether a manifest could be called a recipe too, and saw that it might
confuse. A recipe says how to make a computer. A manifest names a recipe, and an extension that
only adds rules has no computer at all. So the two words stay apart.

## Why "extension"

"Plugin" says something is plugged into the host, and nothing loads into the engine or the app
(`extension-ships-a-computer`). VS Code and browsers call "extension" what is installed while the host
runs, in its own process, with a limited set of tools and a manifest.

## What it replaces

`added-classifier` and `what-defines-a-classifier`, which described what is added as a
classifier. A classifier is now one case: `what-makes-a-classifier` in the part `opinions`.

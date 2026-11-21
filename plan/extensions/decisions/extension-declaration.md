# What an extension is

## The owner's words

- First, on 19 November 2026, of what was then called a plugin: it has a structure that must
  match what Silo supports. It says when it is triggered, what its input is, where it runs, what
  its output is, and the rules, and perhaps the precautions, that come with it. Those are put
  forward at install and accepted explicitly. What happens inside can be anything.
- On 21 November 2026: "extension" is the word. An extension is a back end, the program, and a
  front end, which is panels that fit in the app the way the app is designed. Silo can offer
  useful pieces for the front end, such as notifying and showing a panel in different ways.
- The owner asked for one design of the whole before single points are settled, and found the
  picture below makes sense.
- A declaration uses the standard names, not plain labels such as "given" or "gives back". The
  owner called `on`, `inputs`, `outputs`, `runs`, `permissions` perfect as field names. What goes
  under each is settled when it is built.

## The picture

An extension never reaches another computer. Its back end asks the engine, through tools, and the
engine does the thing.

| Part | What it is | Decided in |
|---|---|---|
| back end | what is executed, in a computer of its own | `back-end-runs-in-a-computer` |
| front end | panels the app draws | `panels-are-data` |
| rules | ordinary rules it adds | the part `rules` |

What wakes the back end: `what-wakes-a-back-end`. What it may use: `extension-tools`. What it can
never do: `what-an-extension-never-does`. What it may stop by itself: `extension-may-protect`.
Its secrets: `extension-secrets`. Importing and installing: `import-then-install`.

## Three examples

The owner wants these three in the film.

| Extension | Back end | Wakes on | May use | Adds |
|---|---|---|---|---|
| registry rules | none | | | rules: allow `crates.io` on a computer |
| disk watch | yes | every minute | read `/work` on main; show a panel; notify the user | |
| request check | yes | a network request of main waits | deny or hold that request | |

Examples of what an extension may use are generic and exact, such as a path. The owner asked for
this.

## Why "extension"

"Plugin" says something is plugged into the host, and nothing loads into the engine or the app
(`extended-from-outside`). VS Code and browsers call "extension" what is installed while the host
runs, in its own process, with a limited set of tools and a manifest. Read on Theia's pages on 21
November 2026: there the word names the opposite case, a module "installed at compile time" that
"can access the full API".

## What it replaces

`added-classifier` and `what-defines-a-classifier`, which described the added program as a
classifier. A classifier is now one case: `what-makes-a-classifier` in the part `opinions`.

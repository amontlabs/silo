# What you can add

Extensions: rules, a computer, or both, that someone made and you add to Silo.

## For

To let people watch a lot of agent work in their own way, without giving what they add any power
to open or delete something.

## Why it is so

- **An extension that executes anything ships a computer** (`extension-ships-a-computer`). Its
  manifest names the recipe. What Silo only reads, such as rules, is not executed.
- **Its computer reaches Silo only through the tools the manifest names**, and installing the
  extension grants them (`extension-tools`, `import-then-install`).
- **No tool grants or destroys** (`what-an-extension-never-does`). A tool reads or takes a
  protective action, and a protective one sets a precaution (`extension-may-protect`).

These are put forward and not in force yet. In force today: `added-classifier-cannot-act`,
`where-a-classifier-runs-is-shown`.

## Three examples

- **Registry rules.** No computer. It adds rules that allow `crates.io` on a computer. Importing
  it shows those rules, and installing it switches them on.
- **Disk watch.** Its computer wakes every minute, reads `/work` on main, creates a panel and
  alerts you. It can stop nothing.
- **Request check.** Its computer is asked when a network request of main waits at the boundary.
  It answers: let it go on, deny it, or hold it. It can never allow one that the rules deny.

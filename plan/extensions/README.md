# What you can add

Extensions: rules, a computer, or both, that someone made and you add to Silo.

## For

To let people watch a lot of agent work in their own way, without giving what they add any power
to open or delete something.

## Why it is so

- **An extension that executes anything ships a computer** (`extension-code-in-its-own-computer`,
  `extension-computer-is-shown`). Its manifest names the recipe. What Silo only reads, such as
  rules, is not executed.
- **No tool it can use grants or destroys** (`no-tool-grants-or-destroys`).
- **Importing it changes nothing** (`importing-changes-nothing`). Installing it switches on what
  it showed.

Put forward and not in force yet: which tools it may use (`extension-tools`), that it may protect
by itself (`extension-may-protect`), and what wakes it (`what-wakes-an-extension`).

## Three examples

- **Registry rules.** No computer. It adds rules that allow `crates.io` on a computer. Importing
  it shows those rules, and installing it switches them on.
- **Disk watch.** Its computer wakes every minute, reads `/work` on main, creates a panel and
  alerts you. It can stop nothing.
- **Request check.** Its computer is asked when a network request of main waits at the boundary.
  It answers: let it go on, deny it, or hold it. It can never allow one that the rules deny.

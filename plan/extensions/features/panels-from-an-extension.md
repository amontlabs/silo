# Panels from an extension

## The choice

An agent's part, 22 November 2026. An extension called Spend by repository shows a panel in the app
with this week's spend per repository: amontlabs/site 14 dollars, silo 31 dollars. A click on a row
wakes it, and it shows that repository's conversations.

The options:

- Build it (recommended). An extension can show what it found in a view of its own. Silo's app draws
  it from the extension's data, so no code of the extension runs in the app. It costs a set of
  building blocks, such as lists, tables and buttons, that Silo keeps stable for the people who
  write extensions, and one more place for you to look.
- Leave it out. An extension can only alert you, ask you a question or give opinions on lines of the
  record. It costs summaries like spend per repository, which have nowhere to go but an alert.

Recommended: build it. Without it, what is not urgent ends up in alerts, and Slack's message blocks
show that a fixed set of building blocks, drawn by the app, covers most of what outside apps need.

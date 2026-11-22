# The record path and the core

## For the owner

An agent's part, 22 November 2026. A build changes 50,000 files in a minute in main. During that
minute you take a secret back from main in the app.

The options:

- The record has a path of its own (recommended). The 50,000 lines go straight to the record, and
  your click is handled at once. It costs a record that can be a moment behind what the app shows.
- One path for everything. Every line passes where your click is handled. It costs your click
  waiting behind the 50,000 lines.

Recommended: a path of its own. What you do must never wait behind what an agent's build produces.

## Notes

"Core" is the engine's word, set by `engine-shape` in the part `engine`, which also holds why the
record has a path of its own. The record names no check for this sentence: `types` is put forward
here.

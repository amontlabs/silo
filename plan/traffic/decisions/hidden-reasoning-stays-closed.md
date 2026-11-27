# Does Silo open the model's hidden reasoning

## The choice

An agent's part, 22 November 2026. Codex's model thinks for 40 seconds before it deletes a folder in
main. OpenAI sends that thinking sealed, so that only OpenAI can read it, with a short summary in
plain text.

The options:

- Keep what is plain (recommended). The record keeps the summary with the rest of the reply. Of the
  sealed part it says that it passed, and its size. It costs disk for the summaries, and the record
  holds only what the provider chose to show.
- Keep no thinking at all. The record says only that thinking passed, and its size, even for the
  summary. It costs the best clue to why an agent did something, when you look back at a turn.

Recommended: keep what is plain. Silo never tries to open the sealed part, which it has no key for
and which OpenAI's policies forbid trying to extract.

## Notes

- OpenAI's terms forbid trying to extract the model's protected reasoning. (Read on 19 November 2026.)

## Not yet seen in a capture

An agent's part, 27 November 2026. The sentence above that OpenAI sends thinking sealed with a
short summary in plain text is an agent's statement. An agent searched the captures of the research and found no
reasoning block: the run on real providers kept sizes and counts, and every later run used stand-in
providers that sent none. How much reasoning is readable at the boundary, for each agent program,
is to find out, and `what-silo-cannot-see` depends on it.

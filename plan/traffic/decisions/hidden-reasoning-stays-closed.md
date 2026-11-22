# Does Silo open the model's hidden reasoning

## For the owner

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

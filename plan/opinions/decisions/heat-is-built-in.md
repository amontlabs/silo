# Heat

## For the owner

An agent's part, 22 November 2026. Claude Code, Codex and a second Claude Code all wrote
/work/site/package-lock.json in main today, and the map of hot files shows it as written by three
runs.

This is already settled by `hot-files`, which you took: Silo ships the map of which paths several
runs touch. The number on that map is counted from Silo's opinions on which run wrote each path, so
it is as sure as they are. You ruled on an earlier wording of this entry, so only you can close it.

The options:

- Counted from which run wrote each path (recommended). It costs nothing new.
- A classifier of its own for heat. It costs a second judgement of the same thing, which can
  disagree with the first: the map could say three runs wrote the file while the list of who wrote
  what names two.

Recommended: counted from which run wrote each path. Taking it changes nothing; it closes a question
you already answered.

## Notes

The eighth item of the list of built-in classifiers (`built-in-classifiers`). The record's list is
Proposed; the line's own mark is "Agreed: the map only, see 5.7" (record l.666, l.682). What the
owner agreed is the map, not that Silo ships a classifier for heat. The map, "Silo ships that map"
(record l.972–973), is the taken decision `hot-files`, in the part `views`.

# A flood

The record's principles for the record path say it again: "Everything has a limit. Under pressure
a source merges what it writes before it loses any, and marks it as merged. It never merges a fact
with a report, and lines already written never change." Its first sentence is the property
`nothing-grows-without-limit` in the part `engine`.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Keys an agent controls never shape the structure.** Keys that an untrusted caller supplies
  cannot choose a structure's depth or its collisions. Budget the entries and the work per source,
  and use an index whose cost stays bounded whatever the keys. Silo indexes what agents name: file
  paths, hosts, process names. An agent can create millions of nested paths or colliding names to
  slow the recorder or the helper. Each source has a budget of entries, and the index stays fast
  whatever names an agent picks.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/docs/context/phase-1-trie.md?plain=1#L325))

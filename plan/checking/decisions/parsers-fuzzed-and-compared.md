# How the code that reads what a computer sends is checked

## The choice

An agent's part, 25 November 2026, from #352 (file 11), with #358 and #363. `fuzz-untrusted-parsers` was taken
by agents, so this replacement is the agents' to take with a judge, and polarzero confirms it or not.

The case: an agent crafts a directory in main's disk that Silo's reader, outside the computer, reads
as one tree and macOS reads as another. Nothing crashes, so fuzzing for crashes finds nothing, and
Silo reports changes that are not the ones macOS sees. The boundary has the same case: a request with
two `Content-Length` headers is one request to Silo and another to the host (#358); #363's strict
reader refused 46 of 46 such requests.

The options:

- Add: a parser of a format another program also reads rejects any input that program could read
  differently (recommended). It costs a differential test against the other reader where one can
  run, and refusals of inputs a lenient reader would take.
- Fuzz for crashes, hangs and assertions only, as the earlier answer says.

Recommended: add the sentence. For these two readers, the dangerous bug is a different reading, not a
crash.

## More research on this (27 November 2026)

An agent's part, 27 November 2026. These reports bear on this entry, and its notes did not point to them. The answer is unchanged.

- `research/2026-11-25-untrusted-parsers/` (#401) is the comparison of the drafts' parsers with the programs they sit in front of, which this entry's added sentence comes from.
- `research/2026-11-25-parser-fixes/` fixes three of its findings, each with a test that fails first: the ext4 readers, the preview forwarder and the closed network's proxy.

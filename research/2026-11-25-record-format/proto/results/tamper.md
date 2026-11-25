| Change | Found | What verify says |
|---|---|---|
| One byte of a line changed | yes | a line does not match its CRC |
| A line changed and its CRC written again | yes | the line before it was changed, removed or moved |
| The last line changed and its CRC written again | yes | the head's line was changed |
| A line removed | yes | the line is not the next line of the chain |
| Two lines swapped | yes | the line is not the next line of the chain |
| A forged line inserted, with a valid CRC | yes | the line is not the next line of the chain |
| The last lines cut at a line's end | yes | the record ends before the head's line: lines were removed at the end |
| The last lines cut at a line's end, no head | no | every chain holds |
| The last segment deleted | yes | the record ends before the head's line: lines were removed at the end |
| A segment in the middle deleted | yes | segments are missing or out of order |
| A header byte changed | yes | the segment's header does not match its checksum |
| A segment of another computer put in its place | yes | the segment belongs to another computer |
| Garbage appended to the last segment | yes | the last line is unfinished: the recorder stopped while writing it, or it was changed |
| A line changed and every line after it chained again, old head kept elsewhere | yes | the head's line was changed |
| A line changed and every line after it chained again, head rewritten too | no | every chain holds |
| A blob's bytes changed | yes | a blob's bytes do not match its hash |
| A blob deleted, no fact says so | yes | a blob a line points to is missing, and no fact says it was removed |
| A blob deleted by the size limit, a fact names it | no | every chain holds |
| A line changed in the middle of the last segment, then the recorder starts | yes | the recorder refuses to start: record_damaged |

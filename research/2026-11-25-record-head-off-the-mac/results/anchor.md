| Change | #386 `silo verify` | Git anchor | Ledger | What the anchor check says | Note |
|---|---|---|---|---|---|
| Nothing changed | holds | holds | holds | 12 anchored heads hold; 0 lines after the newest one are not anchored yet |  |
| The last 18 lines cut at a line's end, head deleted | holds | found | found | the record ends at line 30, before line 32, anchored at 2026-11-25T02:41:07.000031000Z: lines were removed at the end |  |
| Line 10 changed, every line after it chained again, head rewritten | holds | found | found | line 12 is not the line anchored at 2026-11-25T02:41:07.000011000Z: the record was rewritten at or before it |  |
| The same, and the Git anchor attacked: force push, branch deleted, file replaced in a new commit | holds | found | found | line 12 is not the line anchored at 2026-11-25T02:41:07.000011000Z: the record was rewritten at or before it | force push: refused; delete: refused; new commit: accepted |
| The same, and forged heads posted to the ledger | holds | found | found | line 12 is not the line anchored at 2026-11-25T02:41:07.000011000Z: the record was rewritten at or before it | signed with another key: 403 not signed by this computer; an older line: 409 older than the last head held; signed with the Mac's key: 201 kept |
| Anchored up to line 40, then lines 43 to 48 cut and the head deleted | holds | holds | holds | 10 anchored heads hold; 2 lines after the newest one are not anchored yet | the window: lines written since the last head sent |
| Signed heads kept on the Mac, deleted with the rewrite | holds |  |  | 0 anchored heads hold; 48 lines after the newest one are not anchored yet | before they were deleted: found |

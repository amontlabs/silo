# Does a wait for a classifier's opinion end at Silo's limit

## For the owner

An agent's part, 22 November 2026. You installed an extension that gives its opinion on each tool
call of main before the call goes on. Its computer hangs while a call from main waits for it.

The options:

- Silo's own limit (recommended). Silo ends the wait itself, at a limit it keeps. The request is
  then denied or left to your rules, as the setting of that extension says. It costs a request that
  is refused, or passed to the rules unchecked, when a slow extension would have answered a moment
  later.
- The extension's own limit. Each extension says how long it may take. It costs you control, since
  an extension that is stuck or hostile keeps your agents waiting for as long as it wrote.

Recommended: Silo's own limit. A hold already ends this way, and nothing you installed should be
able to stop your agents for good.

## Notes

The record's wording, Proposed: "Waiting for a classifier's opinion is not a hold: a hold waits
for you. That wait ends at Silo's limit in the same way, and what then happens to the request is
the default of 14.1, question 17." That default is asked in `extension-that-does-not-answer` in `extensions`.

The property it sets is row 35 of the record's section 10, whole. For a hold, the part in force
is `hold-limit`.

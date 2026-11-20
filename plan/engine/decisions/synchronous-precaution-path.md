# Which process does what for a synchronous precaution

- The record also says: "Silo keeps the limit of that wait outside the core and ends it itself."
  Whether that wait has a limit is `classifier-wait-limit`, in the part `boundary`. What the core
  does meanwhile is `core-never-waits`.
- If the recorder stops, a request that waits for a synchronous precaution still ends its wait at
  the helper's limit (the table of `processes`).

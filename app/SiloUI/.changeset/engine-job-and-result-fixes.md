---
"silo-ui": patch
---

Silo now starts its engine when another copy of the app registered the engine's background job first, instead of waiting on "engine is not reachable" forever; an engine that is already running is never replaced. A computer's agent can now read the result of a merge or other job the app already acknowledged, so `silo merge` no longer ends with "The job was not found."

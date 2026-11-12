---
"silo-ui": patch
---

When Silo's engine starts after the app, the "Silo could not load" screen and the "System issue" banner now go away on their own once the engine answers, instead of waiting for Retry. Silo tries again every few seconds for about five minutes if no signal arrives.

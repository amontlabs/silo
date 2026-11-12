---
"silo-ui": patch
---

The limits on what a computer's agent can do now hold when it adds extra fields to a call, starts several computers at once, or starts and acknowledges jobs in a loop: the memory a start needs is read from the computer itself, the free-memory check and the reservation happen together, and results Silo still keeps for you count against the computer that started the job. Calls from a computer can no longer push your own entries out of the activity log; if a computer makes more calls than the log can take at once, the log shows one counted line for the ones it skipped.

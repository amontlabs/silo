---
"silo-ui": minor
---

Forks of a Mac computer now start quiet: the source's scheduled jobs, crontabs and scheduled launch agents are moved aside (services that keep running stay) before the fork first starts, the processes and paths a recipe names are stopped and deleted, and no secret or GitHub access is copied unless it is named. Forking a Mac without choosing a checkpoint now forks its current state from a "Fork point" checkpoint that stays while a fork made from it exists. A Mac fork that has not started yet is also no longer broken by Retry setup, and its first start now stops when you stop it.

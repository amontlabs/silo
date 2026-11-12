---
"silo-ui": patch
---

Computers created before this version are moved to the new, protected computer storage the first time Silo starts. Silo stops any computer that is still running, keeps a full copy of the computer storage beside the original (deleted with the pre-upgrade backup after 14 days, or earlier from the migration screen), and shows "Upgrading computer storage" while it works. A computer that cannot be moved keeps all its files, is marked "Needs attention" with the reason, cannot be started, and is tried again at the next launch.

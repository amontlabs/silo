---
"silo-ui": minor
---

An agent inside a computer can make and run as many computers as it judges right: the old limits of 32 computers and 3 running ones are gone. It can read what the device has left (memory, disk, running computers) and Silo stops it from freezing the device or filling its disk: below a safety floor of free memory or disk, a computer's agent cannot start or create a computer or take a checkpoint, and is told what the device has. The floors default to the larger of 4 GiB and 10 % of memory, and the larger of 10 GiB and 5 % of the disk, and can be changed in the engine settings. Your own actions are never held back by them.

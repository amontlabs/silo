---
"silo-ui": patch
---

A computer's network list now refuses a `*.` host over any shared name on the Public Suffix List, such as `*.duckdns.org` or `*.blogspot.com`, not only the common ones, and cloud bases such as `*.azure.com` or `*.windows.net`. Lists saved earlier keep working until you save them again. A command that prints more than about 960 KB to its error output is now stopped with "printed too much" instead of running on, and the hosts that GitHub access adds only grow when you accept a GitHub update, also for a computer that has no saved list yet. A folder watch ends if the computer sends output that is not a folder path.

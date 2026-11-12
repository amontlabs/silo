---
"silo-ui": patch
---

`silo secrets set` takes `--allow-shared-hosting`, which lets a secret's computers reach every site under a shared-hosting wildcard such as `*.surge.sh`, and prints the same plain warning the app shows. Without it nothing changes.

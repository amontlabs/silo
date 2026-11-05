---
"silo-ui": minor
---

Recipes: describe a clean computer in `.silo/recipes/<name>.yaml` (OS, packages, setup commands, services, ports), build it once with `silo recipe build`, and fork from it with `silo fork <name> --recipe <recipe>`. `silo up` makes `main` from a recipe, starts it and keeps a warm checkpoint, so forks of an unchanged `main` start faster. `linux-dev` and `macos-xcode` come built in.

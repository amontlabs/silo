---
"silo-ui": minor
---

`silo fold <fork> --promote` makes a fork the main computer: it takes main's name, published ports, secrets and GitHub access, and the old main is kept stopped as `main-before-<time>`. `silo undo main` swaps them back. Available on macOS computers; the fold tool in the MCP server takes `promote` too.

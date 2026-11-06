---
"silo-ui": minor
---

The app now carries the `silo` command and links it into `~/.local/bin` every time Silo starts (`silo-dev` for Silo Dev), so the command line and MCP server need no separate build. Silo replaces only its own earlier link; a `silo` file or link of your own is left alone and Connections shows that the command is not on PATH.

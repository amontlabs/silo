---
"silo-ui": patch
---

Silo's tools in a computer are put in place more carefully. A program you send with `silo tools enable --binary` stays until the computer next starts instead of being replaced ten minutes later. A look at a computer that finds everything in order no longer rewrites anything in it, and one that takes too long is cut off and shown as waiting; a computer that keeps failing is left alone until it starts again. Silo no longer overwrites a `silo` tool you set up yourself in Claude Code or Codex: it leaves yours as it is and says so in the computer's status. A fork of a computer whose tools are off has Silo's tools taken out too, and the files Silo writes in a computer can no longer be redirected through links.

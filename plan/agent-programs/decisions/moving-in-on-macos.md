# What does moving in keep when main is on macOS

## For the owner

An agent's part, 22 November 2026. On your Mac your user is ana. A setting of Claude Code starts an
MCP server at /Users/ana/.local/bin/linear-mcp, and a skill calls /opt/homebrew/bin/node.

The options:

- Your user name and paths (recommended). Main's user is ana, with the same home folder, so both
  paths work as they come in. Where main lacks a path, the first run lists the setting. It costs
  little: an agent in main sees your user name, as it already sees your files.
- One user name in every computer. Every computer has the same user, such as agent, and Silo
  rewrites the paths it finds as they come in. It costs every path it misses, which breaks the first
  time an agent uses it.

Recommended: your user name and paths. It lets settings and skills work in main without being
touched, which is much of the point of a macOS main.

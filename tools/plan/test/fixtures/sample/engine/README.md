# The engine

What runs the computers, and what it is built with.

## For

To run each computer on a VM engine that Silo controls, so that everything that crosses its
boundary passes through Silo.

## Why it is so

- **One core piece** (`engine-language`): the app, the command line and every tool only talk to
  the engine.

## One example

You start a computer. The engine boots it, and it has no channel until a rule opens one.

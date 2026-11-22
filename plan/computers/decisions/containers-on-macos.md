# Can a macOS computer run containers

## For the owner

An agent's part, 22 November 2026. An agent in main needs a Postgres database in Docker to run the
tests of `~/work/app`.

The options:

- Docker in a Linux computer beside main (recommended). Main's agents send their `docker` commands
  to that Linux computer over a channel you grant, and one Linux computer holds several containers.
  It costs the memory of a second computer, and a way from main into a computer where agents have
  full access. A container cannot reach main's folders, so mounting `./src` from main fails, and the
  code goes as a commit.
- No containers for main's agents. Work that needs Docker goes to an agent in a Linux computer. It
  costs the many projects whose tests start a database or a service in Docker, which agents in main
  then cannot run.

Recommended: Docker in a Linux computer beside main. A macOS computer cannot run containers itself,
and Docker works this way on any Mac, with a Linux machine underneath.

## Notes

To find out.

| | Linux computer | macOS computer |
|---|---|---|
| Containers inside | Yes | Expected no. |

A macOS computer is not expected to run virtual machines, so Docker would run in a Linux computer
beside main, as it does on any Mac.

The owner finds a Linux computer that holds several containers appealing.

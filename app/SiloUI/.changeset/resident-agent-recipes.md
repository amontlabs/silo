---
"silo-ui": minor
---

Resident agents: the built-in `openclaw` and `hermes` recipes install the agent in `main`, start its gateway as a service, and list the gateway under `quiet:` and its token file under `scrub:`, so a fork of `main` does not answer the same chat channels twice. Use them with `silo up --recipe openclaw`, then `ssh main.silo` and `silo guest-client install`.

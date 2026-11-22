# What happens to a request when a rule only alerts

## The choice

An agent's part, 22 November 2026. Main has two rules: allow crates.io, and alert on any new host. A
program in main connects to paste.rs.

The options:

- An alert decides nothing (recommended). No rule allows paste.rs, so it is denied, and you are
  alerted. It costs a second rule when you want to let a host through and hear about it: one that
  allows and one that alerts.
- A rule that only alerts lets the request pass. The connection to paste.rs goes through and you are
  alerted, as with a firewall set to log. It costs the meaning of "no rule means deny": a rule
  written to keep an eye on something opens it.

Recommended: an alert decides nothing. Rules have no order and no rule means deny, and both stay
simple only if an alert is never read as an allow.

## Notes

The owner asked this on 19 November 2026. The answer put forward follows from rules having no
order and from "no rule means deny" (`which-rule-wins`).

The example on the page of this part shows it: allow crates.io; alert on any new host; a
connection to paste.rs is denied, because no rule allows it, and you are alerted.

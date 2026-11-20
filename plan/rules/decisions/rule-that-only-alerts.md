# What happens to a request when a rule only alerts

The owner asked this on 19 November 2026. The answer put forward follows from rules having no
order and from "no rule means deny" (`which-rule-wins`).

The example on the page of this part shows it: allow crates.io; alert on any new host; a
connection to paste.rs is denied, because no rule allows it, and you are alerted.

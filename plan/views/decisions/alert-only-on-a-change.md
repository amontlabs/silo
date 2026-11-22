# When Silo alerts you

## The choice

An agent's part, 22 November 2026. At 14:02 main's disk passes 90% and stays there all afternoon. In
the same minute an install in main reaches 12 hosts that main never reached before.

The options:

- On a change (recommended). You get one alert at 14:02 for the disk, and one alert for 12 new
  hosts. The disk stays in your list of what needs you until it drops back under 90%. It costs
  reminders: once you set the alert aside, nothing tells you again while the disk stays full.
- Each time. An alert each time a rule is triggered, and each minute the disk stays above 90%.
  It costs dozens of alerts an hour, and the habit of ignoring them.

Recommended: on a change. Alerts that repeat what you already know teach you to stop reading them,
and the list of what needs you already keeps what is still true.

## Notes

Marked Notes in the design record.

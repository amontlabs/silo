# Keeping the Mac awake while computers run

## The choice

An agent's part, 24 November 2026. You start two agents in main at 23:00 and leave the MacBook on
its desk, plugged in. After a few minutes of no input, macOS puts the Mac to sleep, and main with
it. In the morning the agents did ten minutes of work.

The options:

- Hold the Mac awake while a computer runs on power, and say what it cannot do (recommended). A
  power assertion keeps the Mac from idle sleep. A closed lid still sleeps unless an external
  display is attached, so Silo says so. Each sleep and wake is in the record. It costs power at
  night.
- Leave it to you. It costs every first night of someone who did not change their energy settings.
- Hold it awake on battery too. It costs a drained battery.

Recommended: hold the Mac awake while a computer runs on power, and say what it cannot do.

## Notes

Agent, 24 November 2026. From draft #354, gap 2, and draft #360's notes on `main-on-several-devices`:
people who run agents overnight use a Mac mini that stays on. Whether Apple's framework already
holds the Mac awake while a virtual machine runs is not known; `pmset -g assertions` on a Mac tells.

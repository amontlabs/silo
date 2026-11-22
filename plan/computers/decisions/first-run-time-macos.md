# How long the first run takes with a macOS main

## The choice

An agent's part, 22 November 2026. You install Silo on a home connection of 100 Mbit/s. Before your
first agent can work in main, Silo downloads macOS from Apple, over 10 GB, and installs it.

The options:

- One wait, shown step by step (recommended). The download starts as soon as you say yes, and Silo
  shows each step and the time left. On that connection the download alone takes more than ten
  minutes, and the install comes after it; the full time is to measure. It costs that wait, once per
  Mac.
- Your agents start in a Linux computer while macOS installs. Their first work begins within
  minutes. It costs a first day spent where your agents will not live: their work moves to main as
  commits afterwards, and two computers run at the first start.

Recommended: one wait, shown. It comes once, and a first run that starts agents in one place and
then moves them is harder to follow than a wait.

## Notes

To find out.

Apple forbids shipping a pre-installed macOS image. Each Mac downloads and installs its own, so
the first run takes longer than with a Linux image.

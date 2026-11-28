# The Swift Mac app for MVP

## The choice

An agent's part, 28 November 2026. You want to use Silo every day on your Mac: create main, switch
its network between open, limited and closed, enter a secret without pasting it into a chat, approve
a browser sign-in, restore a checkpoint, fork main, inspect a receipt and copy or export it to give
to an agent for analysis.

The options:

- Build the views-only Swift Mac app early over the settled C interface (recommended). The Swift
  app holds no logic (`mac-app-language`) and calls the exact same Zig client (`mvp-api`) that
  `silo` calls. Every MVP feature lands in the app as it is built so you never need to touch the
  command line, while post-MVP screens (`now-in-the-app`, `disk-in-the-app`, `network-in-the-app`,
  extension panels and held-request queues) wait until after MVP. It costs building and maintaining
  a SwiftUI shell alongside the early engine features.
- Keep the Mac app minimal and tacked onto the end of MVP (`receipts-of-work`), driving setup,
  secrets and network controls from the command line. It costs touching the CLI for daily use and
  defers testing the C interface from Swift until the end of MVP.

Recommended: build the views-only Swift Mac app early over the settled C interface. Because all
state, validation, diffing and receipt export live in Zig, adding full MVP control to Swift adds
thin views rather than duplicate logic.

## polarzero's words, 28 November 2026

polarzero asked to be able to handle every MVP action from the Swift Mac app without having to
touch the command line, including copy and export controls so a full receipt and its relevant data
can be handed straight to an agent for analysis.

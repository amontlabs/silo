# How does an agent run its build on your iPhone

## The choice

An agent's part, 24 November 2026. An agent in main has fixed a bug that only shows on a real
iPhone, with its camera. It wants to install the build on your phone, which is plugged into your
Mac.

The options:

- From your Mac, after your yes (recommended). The agent asks. You see which computer, which app and
  which device. On your yes, the built app comes out through a channel, and Silo installs it with
  `xcrun devicectl device install app` on your Mac. The agent is told whether it worked. It costs
  the debugger: attaching to the app on the phone happens on your Mac, not in the computer.
- The device attached to the computer. macOS 27 adds USB devices for virtual machines, but on the
  macOS 27 beta an iPhone was reported not to connect while USB drives did. Reaching a paired phone
  over the network goes through your local network, which main's network does not reach. It costs
  what is not known to work, and a device that is in one computer at a time.
- Not at all. Builds stay in simulators. It costs the cases only a device has: the camera,
  push notifications, performance.

Recommended: from your Mac, after your yes. It works with what Apple ships today, and your phone
stays yours.

## Notes

An agent's research, 24 November 2026: `research/2026-11-24-mac-and-ios-developers/README.md`, section "Running on an
iPhone". Mac test 30 of that page would try the device attached to a computer on macOS 27, once the
kit's VM helper can attach a USB device.

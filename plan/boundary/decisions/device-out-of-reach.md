# Can a computer reach your device

## The choice

An agent's part, 22 November 2026. You run a model server on your Mac, on port 11434, and you want
an agent in main to use it. Another day an agent in main scans 192.168.1.x and finds the admin page
of your router.

The options:

- Out of reach unless a rule names it (recommended). An open network never includes your Mac, your
  local network or your VPN, so the scan finds nothing. A rule you set can open one address and
  port, such as the model server. It costs you that one service, which a hostile agent in that
  computer can then attack like any host you allowed.
- Out of reach, always. No rule can open anything on your Mac or your local network. It costs you
  the model server or the database on your Mac, which must move into a computer or onto a public
  host before an agent can use it.

Recommended: out of reach unless a rule names it. You already set the rules for every other
destination, and a model server or a database on the Mac is a common setup.

## Notes

The record's section 10 says the check is a hostile test "on the real boundary".

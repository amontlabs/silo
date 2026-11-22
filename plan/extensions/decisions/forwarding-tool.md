# Forwarding

## For the owner

An agent's part, 22 November 2026. You are away from your Mac. An agent in main pushes to
amontlabs/site, and a rule holds the push for you. Your phone shows: "main: push of 3 commits to
amontlabs/site waits for you".

The options:

- An extension that reads them (recommended). A forwarder, one Silo ships or one anyone writes,
  reads the alerts and asks of the computers you choose and sends them where its own rules allow,
  such as ntfy.sh. Before you install it, the app shows that main's alerts and asks can reach
  ntfy.sh. It costs a small computer that runs for the forwarder, and you answer the ask in Silo,
  not on the phone, since an extension may not say yes for you.
- Built into Silo. Silo sends alerts and asks itself to a service you name. It costs a way out of
  Silo that no computer's rules limit, and Silo's own code then has to talk to each phone service.

Recommended: an extension. Sending your alerts out is a way out like any other, and an extension's
computer has rules you set and is shown, so you see and limit where they go.

## Notes

None of the four calls of `extension-tools` forwards. Forwarding alerts and asks to a phone is the
later feature `forwarding-to-a-phone`.

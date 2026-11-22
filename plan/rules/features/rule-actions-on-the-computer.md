# Rules that act on the computer

## The choice

An agent's part, 22 November 2026. You put a fake token in main that no honest task uses. One night
a request carries it towards a host. A rule denies the request, stops main and alerts you.

The options:

- Build it (recommended). Whatever was sending the token is ended before it tries anything else. It
  costs what a stop ends: every program running in main, the honest ones included. A rule is exact,
  so it also fires when the cause was harmless, such as a script of yours that uploads its whole
  environment.
- Leave it out. A rule only decides on the request. The token is denied, and you stop main by hand
  when you see the alert. It costs the hours until you look.

Recommended: build it. A planted secret with an alarm on it is a known practice, and it is useful
only if something acts when you are not there.

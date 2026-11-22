# Stopping and pausing computers

## The choice

An agent's part, 22 November 2026. An agent in a fork pushes to a test server every second. You
pause the fork, look, and then stop it, and a server it had started in the background with `nohup`
ends too. Later, things go wrong in main and two forks at once, and one action pauses all three.

The options:

- Build it (recommended). A pause freezes a computer and loses nothing, a stop ends everything that
  runs in it, and one action pauses every computer. It costs a short wait for each stop, while
  programs inside get the chance to save.
- Leave it out. It costs every rule and precaution that pauses or stops a computer, and your one way
  to halt all agents at once.

Recommended: build it. Being able to freeze any computer at once is what makes full access inside it
acceptable.

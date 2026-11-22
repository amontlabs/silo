# Secrets the computer cannot read

## For the owner

An agent's part, 22 November 2026. An agent in main needs your GitHub token. You type it in Silo's
own prompt. Main holds only a placeholder. An agent that prints its environment or posts it to
paste.rs sends the placeholder, and the real token goes out only on requests to api.github.com.

The options:

- Build it (recommended). The real value never enters the computer or the record. It costs two
  things. Silo has to open the encrypted connection to the hosts that get a secret, with a
  certificate the computer trusts. And a program that computes something from the secret inside the
  computer, such as the signature AWS asks for on each request, cannot work with a placeholder. How
  to serve those is to find out.
- Leave it out. You paste secrets into the computer as you do today. It costs the secret: every
  agent in the computer can read it and send it to any host the rules allow.

Recommended: build it. The secret is the private data an agent must not carry out, and keeping it
outside is the one way to be sure it does not.

# Can a secret be limited

## The choice

An agent's part, 22 November 2026. You give a fork a GitHub token so that it can read a private
repository, and you do not want it to push. A Git fetch over HTTPS is sent as a POST request.

The options:

- Reading means GET, plus listed reads (recommended). The token goes out with GET requests and with
  the requests a rule lists as reads for that host, such as a Git fetch. It can also have an end
  time. It costs a list to keep for each such host, and a wrong line in it lets a write through.
- Reading means GET only. It costs the case above: the fork cannot fetch the repository with a token
  limited to reading.
- No limit to reading in Silo. You make a read-only token at GitHub, and Silo keeps only the end
  time. It costs every service that offers no read-only token.

Recommended: reading means GET, plus listed reads. A limit that refuses a Git fetch would be
switched off on the first day.

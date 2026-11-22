# The kind of each call

## For the owner

An agent's part, 22 November 2026. In one hour main sent 1,240 requests: 900 to api.anthropic.com, a
model; 300 to registry.npmjs.org, a package registry; 30 to github.com, Git; and 10 POST requests to
api.vercel.com, which change something elsewhere. Network shows them in these groups, and the 10
that change something stand out.

The options:

- Build it (recommended). Each request gets its kind, as Silo's opinion. It costs some wrong kinds:
  a request to an API Silo does not know shows as other, and a POST that only searches can show as
  changing something.
- Leave it out. Network lists hosts and counts only. It costs the sorting: you tell the model and
  the package registry from a deploy yourself, and no precaution can hold what changes something
  elsewhere.

Recommended: build it. The few requests that change something elsewhere are what you want to see
among a thousand that only fetch.

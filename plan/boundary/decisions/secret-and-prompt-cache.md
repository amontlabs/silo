# Does swapping in a secret change a request to a model provider

## The choice

An agent's part, 22 November 2026. An agent in main sends its model provider a conversation of 100
000 tokens on every turn, and Silo puts the real API key in on the way. The provider charges far
less when the request starts exactly as the one before did.

The options:

- Promise what Silo controls (recommended). Silo changes only the key, and every other byte leaves
  as the agent program wrote it. It costs a promise that does not name the cache, though the cache
  is the reason for it.
- Promise that the cache hits. The sentence says the provider's prompt cache still hits. It costs a
  promise Silo cannot keep alone, since the provider decides what its cache holds and for how long.

Recommended: promise what Silo controls. It gives the same result on your bill, and it is a sentence
a test can show true.

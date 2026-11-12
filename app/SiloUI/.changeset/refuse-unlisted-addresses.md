---
"silo-ui": patch
---

A computer that only reaches a list of sites now gets an immediate "connection refused" for any address that is not on the list, on every port except plain DNS, instead of a blocked-request page or a connection that opens and then stays silent. Nothing is ever sent to the unlisted address. Lookups through another resolver are answered by Silo's own resolver.

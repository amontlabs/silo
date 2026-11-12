---
"silo-ui": patch
---

A computer that only reaches a list of sites now gets an immediate "connection refused" for any address that is not on the list, on every port, instead of a blocked-request page on web ports or a connection that opens and then stays silent on others. Nothing is ever sent to the unlisted address.

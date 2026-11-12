---
"silo-ui": patch
---

A computer's network list now accepts a `*.` host over a shared-hosting name such as `*.netlify.app` or `*.duckdns.org`, and warns that anyone can publish there, in the network settings and in the approval that adds it. A secret's `*.` host over shared hosting is added only after you tick "Let computers reach every site under these" when saving the secret, or approve a computer's request that showed the warning; until then the network settings say it is not added. Names a whole registry covers, such as `*.com` or `*.co.uk`, are still refused. Restarting, undoing or restoring a running computer no longer needs free memory for the memory it already holds, only for any extra a larger saved state would take. While a running computer restarts, its memory stays set aside until it is running again.

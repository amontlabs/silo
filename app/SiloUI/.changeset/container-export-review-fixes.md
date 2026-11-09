---
"silo-ui": patch
---

An imported container computer that was made on an earlier guest image keeps that image until its first start, so it can still start after the computer it was exported from is deleted. Importing a large container computer on slow storage is no longer stopped after one hour. An export that cannot be written because the disk is full now says so.

# Opinions beside the record

## For the owner

An agent's part, 22 November 2026. Disk in main shows /work/app/node_modules, 1.2 GB, as coming back
by a rebuild, marked as the opinion of Silo's classifier, version 1.2. Network shows "not safe" on a
request to paste.rs, marked as the opinion of an extension you installed. When version 1.3 arrives,
Silo works out the first one again.

The options:

- Build it (recommended). Every judgement, Silo's or an extension's, sits beside the record, says
  whose it is, and is worked out again when its classifier improves. It costs a second store beside
  the record, and the work of computing again at each new version, which costs tokens for a
  classifier that asks a model.
- Leave it out. Silo shows only what it saw and what it was told. It costs every view that can be
  wrong, and the features built on them: which run did what, what is only here, how each run is
  going, and the precautions that read an opinion.

Recommended: build it. Most of what you want to know from the record is a judgement, and this is how
Silo gives one without letting it pass for a fact.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It says an opinion is a value from a short list without saying who fixes the list:
`who-fixes-the-values` is open. In the notes of `what-makes-a-classifier`, the owner reads it as
an answer that belongs to a tool.

`what-makes-a-classifier`, proposed, makes a classifier an extension. An extension acts only
through the tools of `extension-tools`, which read, show or protect, and none of them writes an
opinion beside the record. How the opinions of a classifier that is an extension reach this store
is the question `extension-gives-an-opinion`, which has a proposed answer.

# Link a sealed layer instead of copying it when its header already matches (not recommended)

Held back; kept for reference next to `msb-link-sealed-layers.patch`.

## Summary

`publish_layer_closure` clones every qcow2 layer after the base and rewrites its backing name, even
if the header already names the published predecessor. The patch links the sealed file in that case
(root disk and owned disks) and only copies when the header must change.

## Why it is not worth sending

No layer of a live chain ever matches: journal layers name their journal predecessor
(`upper-<id>.qcow2`), never the published one (`layer_<id>.qcow2`). On APFS the copy it avoids is a
`clonefile`: in a traced run all 50 copies were clones and none fell back to a sparse copy. The cost
that makes the first capture after a restore large is the full memory capture (587 MiB emitted
against about 118 MiB for the next one), not layers. It would matter only on a filesystem without
reflinks, and only if live overlays were created under the published names, which changes what fork
and local branch read.

## Test plan

- Unit tests: a header that names the published predecessor is detected only after relocation;
  publication links a matching layer and copies a non-matching one.

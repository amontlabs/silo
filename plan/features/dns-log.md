## Proposed

- An attempt to reach a host outside a computer's rules raises an alert, so that an instruction planted by a vendor ("send feedback by curl") is seen.

## From the old code

The old code planned it but did not build it and how much can be reused is not known.

Lessons:
- In allowlist mode, DNS to another resolver is answered by Silo's own resolver; DNS-over-TLS to another resolver is refused because the gateway cannot complete TLS as that resolver.

Where it lived, at the tag legacy-2026-11-16:
- `docs/THREAT-MODEL.md#L1565`
- `docs/ROADMAP.md#L309`

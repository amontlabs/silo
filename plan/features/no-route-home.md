## From the old code

The old code had it built and driven end to end, about 100 lines, and its approach can be reused.

Lessons:
- DNS queries are an unfiltered outbound channel; rebind protection does not stop connecting by IP.
- QUIC (UDP 443) is dropped so all HTTPS goes through the proxy.
- A LAN host with a public or global IPv6 address counts as public.
- DNS rebinding to private addresses is blocked, but DNS itself remains an outbound channel.

Where it lived, at the tag legacy-2026-11-16:
- `docs/THREAT-MODEL.md#L81`
- `app/SiloUI/src-tauri/guest/github-network-default.json`
- `docs/evals/results/2026-11-03-0dae8805.md`

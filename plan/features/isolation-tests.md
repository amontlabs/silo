## From the old code

The old code had it built and driven end to end and its approach can be reused.

Lessons:
- Every blocked attempt is paired with one that must succeed, or the verdict is inconclusive, never pass.
- The canary and secret are sent hex-encoded so the scanner cannot find its own arguments.
- Only two runs exist (2026-11-03, Dev 0.11.1, Linux guests); macOS computers and the allowlist were never evaluated.
- GitHub's hosted macOS runners are VMs that cannot start nested VMs, so they cannot run a Silo computer.

Where it lived, at the tag legacy-2026-11-16:
- `docs/evals/README.md`
- `docs/evals/silo_eval.py`
- `docs/evals/results/2026-11-03-0dae8805.md`
- `docs/evals/test_silo_eval.py`
- `docs/THREAT-MODEL.md#L1565`
- `docs/ROADMAP.md#L428`

"""Score the local readers with #400's scorer, plus time and tokens per call on this machine.

    python3 score_local.py > ../results.txt

The scoring is #400's `score.py`, unchanged, pointed at this folder's answers. Columns for attacks
not run here (repeat, a1-a3, a5-a7) show "-". At temperature 0 a second asking gives the same
answer, so "repeat" was not run.
"""
import json, os, statistics, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "..", "2026-11-25-summary-injection", "harness"))
import score  # noqa: E402

score.ANS = os.path.join(HERE, "..", "answers")


def timing(model):
    out = []
    for d in ("inline", "extract"):
        p = os.path.join(score.ANS, f"{d}-{model}.jsonl")
        if not os.path.exists(p):
            continue
        rs = [json.loads(l) for l in open(p)]
        for step in sorted({r["step"] for r in rs}):
            xs = [r for r in rs if r["step"] == step and "error" not in r]
            bad = sum(1 for r in xs if r.get("answer") is None)
            ms = sorted(r["ms"] for r in xs)
            out.append(f"  {d}/{step:<8} calls {len(xs):4}  no JSON answer {bad:3}  tokens in {statistics.mean(r['in'] for r in xs):5.0f}"
                       f"  out {statistics.mean(r['out'] for r in xs):4.0f}  seconds median {ms[len(ms) // 2] / 1000:5.1f}  p90 {ms[int(len(ms) * .9)] / 1000:5.1f}")
    return "\n".join(out)


if __name__ == "__main__":
    for m in sys.argv[1:] or ["q4b", "q8b"]:
        print(score.report(m))
        print("\nCalls on this machine (4 vCPU, no GPU, llama.cpp, Q4_K_M):")
        print(timing(m))
        print("\n")

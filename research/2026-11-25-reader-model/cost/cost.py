"""What the summary reader costs on the user's own key, from the tokens #400's calls used.

    python3 cost.py

#400 called each reader through `claude -p`, which adds about 1,550 tokens of its own to every
call (measured: an empty call with a one-line system prompt read 1,560 tokens). A classifier
extension calling the provider's API sends only the reader's prompt, so those are taken off.
The tokens are Anthropic's count; other providers' tokenizers differ by some percent.
Prices are list prices per million tokens (input, output), read from OpenRouter's catalogue on
25 November 2026, which resells at the provider's own price.
"""
import json, os, statistics

HERE = os.path.dirname(os.path.abspath(__file__))
ANS = os.path.join(HERE, "..", "..", "2026-11-25-summary-injection", "answers")
CLI_OVERHEAD = 1550

PRICES = {  # USD per million tokens: input, output
    "Claude Haiku 5.5": (0.10, 0.50),
    "Claude Sonnet 5.5": (2.00, 10.00),
    "GPT-5 nano": (0.05, 0.40),
    "GPT-5 mini": (0.25, 2.00),
    "Gemini 3.1 Flash-Lite": (0.25, 1.50),
    "Gemini 3.5 Flash": (1.50, 9.00),
}


def tokens(defense, model, step):
    ins, outs = [], []
    for line in open(os.path.join(ANS, f"{defense}-{model}.jsonl")):
        r = json.loads(line)
        if r.get("variant") == "clean" and r.get("step") == step and "error" not in r:
            ins.append(max(r["in"] - CLI_OVERHEAD, 0))
            outs.append(r["out"])
    return statistics.mean(ins), statistics.mean(outs), max(ins), len(ins)


def main():
    calls = {"inline": tokens("inline", "small", "judge"), "extract": tokens("extract", "small", "extract"),
             "judge": tokens("extract", "small", "judge")}
    print("Tokens per call, #400's 75 clean summaries (small model's calls, Anthropic count, CLI overhead removed)")
    for k, (i, o, mx, n) in calls.items():
        print(f"  {k:<8} in {i:6.0f} (max {mx})  out {o:4.0f}  n={n}")
    tin = sum(v[0] for v in calls.values())
    tout = sum(v[1] for v in calls.values())
    print(f"  two readings, three calls: in {tin:.0f}, out {tout:.0f}")
    print()
    print("Price of one summary read twice (three calls), USD. batch: the providers' batch price (half)")
    print(f"  {'model':<24}{'one summary':>12}{'batch':>10}{'4 runs':>10}{'50 runs':>10}{'50 runs x 30':>14}")
    for m, (pi, po) in PRICES.items():
        one = (tin * pi + tout * po) / 1e6
        print(f"  {m:<24}{one:>12.5f}{one / 2:>10.5f}{4 * one:>10.4f}{50 * one:>10.3f}{50 * 30 * one:>14.2f}")
    print()
    # A real night's receipt is longer than the corpus's: #371's receipt of claude #1 is about 3,600 (measured from its transcript)
    # characters; a long run of 200 commands and 50 files is about 25,000. Tokens ~ characters / 3.5.
    for label, chars in (("#371's receipt of claude #1", 3600), ("a long run: 200 commands, 50 files", 25000)):
        extra = chars / 3.5
        one = {m: ((tin + 2 * extra) * pi + tout * po) / 1e6 for m, (pi, po) in PRICES.items()}
        print(f"  with {label} (~{extra:.0f} tokens, read by two of the three calls): " +
              ", ".join(f"{m} {v:.4f}" for m, v in one.items()))


if __name__ == "__main__":
    main()

"""Run #400's summary readers on a model running on this machine, through llama.cpp.

    python3 local.py <defense> <model> [--set clean|attacked|heldout] [--only a4,a8]

defense  inline | extract   (#400's prompts, unchanged)
model    q4b | q8b          (the GGUF files named in MODELS, Q4_K_M, CPU only)

This reuses #400's harness (`2026-11-25-summary-injection/harness/run.py`): the same corpus, the
same attacks, the same system prompts and user texts. Only the call changes: one llama.cpp chat
completion at temperature 0 instead of `claude -p`. Answers go to ../answers/<defense>-<model>.jsonl
in #400's format, with the prompt and answer tokens as llama.cpp counts them, the time the call
took on this machine, and a price of 0.
"""
import argparse, json, os, re, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
INJ = os.path.join(HERE, "..", "..", "2026-11-25-summary-injection", "harness")
sys.path.insert(0, INJ)
import run  # noqa: E402  (#400's runner; brings defenses, attacks, approaches, catch_rate)

MODELS = {
    # Qwen3 4B Instruct 2507, Q4_K_M, from unsloth/Qwen3-4B-Instruct-2507-GGUF (2.5 GB)
    "q4b": ("q4b.gguf", ""),
    # Qwen3 8B, Q4_K_M, from unsloth/Qwen3-8B-GGUF (5.0 GB); its thinking is switched off with /no_think
    "q8b": ("q8b.gguf", " /no_think"),
}
GGUF_DIR = os.environ.get("GGUF_DIR", "/tmp/claude-0/llm")
LLM = {}


def llm(model):
    if model not in LLM:
        from llama_cpp import Llama
        LLM[model] = Llama(os.path.join(GGUF_DIR, MODELS[model][0]), n_ctx=8192, n_threads=int(os.environ.get("THREADS", os.cpu_count())),
                           seed=1, verbose=False)
    return LLM[model]


def call(model, system, text):
    m = llm(model)
    t0 = time.time()
    r = m.create_chat_completion(messages=[{"role": "system", "content": system},
                                           {"role": "user", "content": text + MODELS[model][1]}],
                                 temperature=0, max_tokens=600)
    ms = int((time.time() - t0) * 1000)
    raw = r["choices"][0]["message"]["content"] or ""
    raw = re.sub(r"<think>.*?</think>", "", raw, flags=re.S).strip()
    mm = re.search(r"\{.*\}", raw, re.S)
    try:
        ans = json.loads(mm.group(0)) if mm else None
    except ValueError:
        ans = None
    u = r.get("usage", {})
    return {"answer": ans, "raw": raw if ans is None else None, "cost_usd": 0.0,
            "in": u.get("prompt_tokens", 0), "out": u.get("completion_tokens", 0), "ms": ms}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("defense", choices=["inline", "extract"])
    ap.add_argument("model", choices=list(MODELS))
    ap.add_argument("--set", default="clean", choices=["clean", "attacked", "heldout"])
    ap.add_argument("--only")
    a = ap.parse_args()
    run.MODELS.update({k: k for k in MODELS})
    run.call = call
    out = os.path.join(HERE, "..", "answers")
    os.makedirs(out, exist_ok=True)
    path = os.path.join(out, f"{a.defense}-{a.model}.jsonl")
    done = run.load_done(path)
    todo = run.jobs_for(a.defense, [a.set])
    if a.only:
        todo = [j for j in todo if j[2] in a.only.split(",")]
    t0 = time.time()
    for i, (c, ev, v) in enumerate(todo):
        run.one(a.defense, a.model, c, ev, v, done, path)
        if i % 10 == 0:
            print(f"{a.defense}-{a.model} {a.set}: {i}/{len(todo)} {time.time() - t0:.0f}s", file=sys.stderr, flush=True)


if __name__ == "__main__":
    main()

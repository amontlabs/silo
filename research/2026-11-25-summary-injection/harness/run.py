"""Run a defense over the corpus, with and without the attacks, one model call per case.

    python3 run.py <defense> <model> [--set clean|attacked|all] [--jobs 8]

defense  inline | fenced | extract   (query reuses extract's first step; no call of its own)
model    small | mid                 (the model aliases are in MODELS)

Each call is `claude -p` with the defense's system prompt in place of Claude Code's, no tools, and
no session saved: the same shape as a classifier extension's one call. Answers are appended to
answers/<defense>-<model>.jsonl, one line per (case, variant, step), with the call's tokens and
price as the CLI reports them; a line already there is not asked again.

clean     every case of #387's fresh corpus (75), with the words as they are
attacked  every false claim (57) with each line of attacks.py added at the end of its words
repeat    every case again with the same words, to measure how much one reader varies on its own
heldout   every false claim with three of the twelve held-out lines, in turn
"""
import argparse, concurrent.futures as cf, json, os, re, subprocess, sys, threading

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import attacks, defenses  # noqa: E402
import approaches, catch_rate  # noqa: E402  (on the path through defenses)

MODELS = {"small": "haiku", "mid": "sonnet"}
OUT = os.path.join(HERE, "..", "answers")
LOCK = threading.Lock()


def words_of(case, ev, variant):
    if variant in ("clean", "repeat"):
        return case["claim"]
    return case["claim"].rstrip() + " " + attacks.ATTACKS[variant](case, ev)


def call(model, system, text):
    p = subprocess.run(["claude", "-p", "--model", MODELS[model], "--system-prompt", system, "--tools", "",
                        "--no-session-persistence", "--output-format", "json"],
                       input=text, capture_output=True, text=True, timeout=300, cwd="/tmp")
    try:
        meta = json.loads(p.stdout)
    except ValueError:
        return {"error": (p.stdout + p.stderr)[-400:]}
    raw = meta.get("result", "")
    m = re.search(r"\{.*\}", raw, re.S)
    try:
        ans = json.loads(m.group(0)) if m else None
    except ValueError:
        ans = None
    u = meta.get("usage", {})
    return {"answer": ans, "raw": raw if ans is None else None, "cost_usd": meta.get("total_cost_usd"),
            "in": u.get("input_tokens", 0) + u.get("cache_creation_input_tokens", 0) + u.get("cache_read_input_tokens", 0),
            "out": u.get("output_tokens", 0), "ms": meta.get("duration_api_ms")}


def jobs_for(defense, sets):
    cases = catch_rate.load("fresh")
    out, nfalse = [], 0
    for case in cases:
        ev = approaches.evidence(case, case.get("order"))
        variants = []
        if "clean" in sets:
            variants.append("clean")
        if "repeat" in sets:
            variants.append("repeat")
        if "attacked" in sets and case["truth"] == "false":
            variants += attacks.NAMES
        if "heldout" in sets and case["truth"] == "false":
            variants += attacks.heldout_for(nfalse)
        nfalse += case["truth"] == "false"
        for v in variants:
            out.append((case, ev, v))
    return out


def load_done(path):
    done = {}
    if os.path.exists(path):
        for line in open(path):
            r = json.loads(line)
            if "error" not in r:
                done[(r["case"], r["variant"], r["step"])] = r
    return done


def one(defense, model, case, ev, variant, done, path):
    words = words_of(case, ev, variant)
    key = (case["id"], variant)
    steps = []
    if defense == "inline":
        steps = [("judge",) + defenses.inline(case, words, ev)[0]]
    elif defense == "fenced":
        steps = [("judge",) + defenses.fenced(case, words, ev)[0]]
    elif defense == "extract":
        steps = [("extract",) + defenses.extract_step(case, words, ev)]
    for name, system, text in steps:
        if key + (name,) in done:
            continue
        r = dict(case=case["id"], variant=variant, step=name, **call(model, system, text))
        with LOCK:
            open(path, "a").write(json.dumps(r) + "\n")
        done[key + (name,)] = r
    if defense == "extract" and key + ("extract",) in done and key + ("judge",) not in done:
        claims_ = defenses.parse_claims(done[key + ("extract",)].get("answer"))
        r = dict(case=case["id"], variant=variant, step="judge",
                 **call(model, defenses.JUDGE_SYSTEM, defenses.judge_text(case, ev, claims_)))
        with LOCK:
            open(path, "a").write(json.dumps(r) + "\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("defense", choices=["inline", "fenced", "extract"])
    ap.add_argument("model", choices=list(MODELS))
    ap.add_argument("--set", default="all", choices=["clean", "repeat", "attacked", "heldout", "all"])
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--only", help="comma-separated variants to keep, e.g. a2,a4,a5,a8")
    a = ap.parse_args()
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, f"{a.defense}-{a.model}.jsonl")
    done = load_done(path)
    sets = ["clean", "repeat", "attacked", "heldout"] if a.set == "all" else [a.set]
    todo = jobs_for(a.defense, sets)
    if a.only:
        todo = [j for j in todo if j[2] in a.only.split(",")]
    with cf.ThreadPoolExecutor(a.jobs) as ex:
        futs = [ex.submit(one, a.defense, a.model, c, ev, v, done, path) for c, ev, v in todo]
        for i, f in enumerate(cf.as_completed(futs)):
            f.result()
            if i % 50 == 0:
                print(f"{a.defense}-{a.model}: {i}/{len(todo)}", file=sys.stderr, flush=True)


if __name__ == "__main__":
    main()
